# PRD 0006 — Histórico local de medições

| Campo | Valor |
|---|---|
| ID | 0006 |
| Status | Proposto |
| Responsável | `cuotai-core`, Desktop e TUI |
| Consumidores | `apps/desktop` (detalhes), `crates/cuotai-tui` (detalhes e `--history`), PRDs 0007, 0008, 0012 e 0014 |
| Depende de | nenhum |
| Data | 2026-10-19 |

## 1. Contexto

`RefreshCycle` (`crates/cuotai-core/src/refresh.rs`) guarda só o último `Report` de cada conta em memória (`slots[index] = Some(report)`) e o `Snapshot` montado a partir deles. Ao fechar o aplicativo, nada sobrevive: na próxima abertura todos os cartões ficam vazios até a primeira consulta terminar. Não há série temporal, gráfico de tendência nem como saber quanto de uma quota foi consumido ontem.

O core já persiste dados locais em SQLite (`rusqlite` com `bundled`): o catálogo de modelos vive em `<dados locais do SO>/cuotai/models/models.sqlite3`, com permissões restritas, isolamento por digest da configuração e da conta, e erro de persistência exposto (PRD 0002, RF3).

A análise de mercado de 2026-10-19 mostrou histórico e tendência no CodexBar (gráficos e burn-down a partir de snapshots gravados), no OpenQuota (hoje, ontem e 30 dias) e no OpenUsage (blocos de gasto de 30 dias).

## 2. Problema

- Não dá para ver a tendência de uma quota, nem se o consumo desta semana está acima do normal.
- O cartão fica vazio a cada abertura do app, mesmo que a última leitura tenha sido feita minutos antes.
- Funcionalidades futuras dependem de dados gravados: ritmo com inclinação recente (PRD 0007), alertas que reconhecem a mesma janela entre reinícios (PRD 0008) e widgets com gráfico (PRD 0014).

## 3. Objetivos

- Gravar, por conta e por métrica, as leituras bem-sucedidas em um banco local compartilhado entre TUI, desktop e CLI que apontam para o mesmo TOML.
- Expor no core consultas de série, de última leitura e de janelas, sem lógica de negócio nas telas.
- Mostrar tendência nos detalhes da conta e permitir exportar e apagar os dados.
- Manter retenção e tamanho limitados e previsíveis.

## 4. Não-objetivos

- Histórico de erros, de notas livres ou de qualquer texto do provedor: só valores numéricos e metadados da janela.
- Sincronização entre máquinas, nuvem ou contas de usuário.
- Faturamento retroativo: o histórico começa na primeira leitura gravada; nunca se reconstrói o passado.
- Alterar `Report`, `Snapshot`, o agendamento ou a saída `--json` atual.

## 5. Requisitos funcionais

### RF1 — Armazenamento (prioridade alta; pré-requisito)

- **RF1.1** Criar `history::HistoryStore` em `cuotai-core`, em SQLite separado dos demais, em `<dados locais do SO>/cuotai/history/history.sqlite3`, com permissões `0700` na pasta e `0600` no arquivo em Unix. Erro de abertura ou de escrita é exposto, nunca tratado como sucesso. Nenhum segredo, corpo HTTP, URL ou nota entra no banco.
- **RF1.2** Isolar por digest do caminho normalizado do TOML e por digest da identidade da conta, com a mesma derivação do catálogo de modelos (PRD 0002, RF3.2). Mover uma conta de posição preserva a série; renomear ou trocar credencial, produto ou região abre uma série nova; remover a conta marca a série para poda.
- **RF1.3** Esquema versionado (`PRAGMA user_version`) com migração para frente. Tabela `samples`: `account_digest`, `metric_key`, `taken_at` (UTC), `used`, `limit`, `remaining`, `unit`, `kind`, `window`, `resets_at`. `metric_key` é o digest de `kind + window + label + unit`. Valores ausentes ficam `NULL`, nunca zero.
- **RF1.4** O banco usa `WAL` e `busy_timeout`; duas instâncias (TUI e desktop) gravam sem corromper. Escritas são idempotentes por `(account_digest, metric_key, taken_at)`.
- **Critério de aceite:** testes com banco temporário cobrem criação, migração, permissões, duas instâncias gravando, valores ausentes, conta movida, renomeada e removida, e falha de disco simulada (erro exposto).

### RF2 — Gravação a partir do refresh (prioridade alta)

- **RF2.1** `Monitor::with_history(HistoryStore)` segue o padrão de `with_model_cache`: sem armazenamento configurado, nada muda no comportamento atual.
- **RF2.2** Depois de cada `Report` com `Status::Ready`, gravar uma amostra por métrica elegível. Relatórios de erro, autenticação, rate limit e `NeedsConfiguration` não gravam nada.
- **RF2.3** Compactação na entrada: não gravar uma amostra idêntica à anterior da mesma métrica a menos de 15 minutos dela, salvo mudança de `resets_at` além da tolerância de 2 minutos (nova janela). Isso limita o crescimento quando o intervalo de consulta é curto.
- **RF2.4** A gravação roda fora do caminho crítico do refresh (tarefa separada com fila limitada); lentidão do disco não atrasa o ciclo nem o prazo da conta.
- **Critério de aceite:** testes com `wiremock` e relógio fixo: uma consulta `Ready` grava N amostras; consulta com erro não grava; amostra repetida dentro de 15 minutos é omitida; mudança de janela grava; fila cheia descarta sem travar.

### RF3 — Consultas no core (prioridade alta)

- **RF3.1** API pura e sem rede:
  - `latest(account) -> Option<StoredReport>`: última leitura por métrica, com a idade original (insumo do PRD 0010);
  - `series(account, metric_key, range) -> Vec<Point>`;
  - `windows(account, metric_key, range) -> Vec<WindowUsage>`: agrupa amostras por instância de janela (mesma `resets_at` dentro da tolerância de 2 minutos) e informa uso inicial, final e pico;
  - `bounds()` com a primeira e a última data gravadas.
- **RF3.2** Detecção de reinício: queda de `used` maior que 20% do `limit` ou avanço de `resets_at` além da tolerância encerra a janela atual. Valores não finitos são ignorados.
- **Critério de aceite:** testes de tabela com séries sintéticas (crescente, reinício, deriva de `resets_at`, buracos de dias, métrica sem limite).

### RF4 — Retenção, exportação e limpeza (prioridade alta)

- **RF4.1** Retenção padrão de 90 dias, configurável de 7 a 365 dias em `desktop.json` (preferência do dispositivo) e por `--history-days` na CLI. Poda ao abrir e depois a cada 24 horas, em lote, sem bloquear leituras.
- **RF4.2** Redução de resolução: amostras com mais de 7 dias são reduzidas a uma por hora (a de maior `used`), preservando o pico de cada janela.
- **RF4.3** Teto de tamanho (padrão 64 MiB). Ao ultrapassar, poda as amostras mais antigas primeiro e registra um aviso visível, nunca silencioso.
- **RF4.4** Exportar em CSV e JSON (conta, métrica, intervalo) e apagar o histórico de uma conta ou de tudo, com confirmação na interface. O apagamento remove também as séries órfãs.
- **Critério de aceite:** testes de poda por idade e por tamanho, redução de resolução preservando o pico, exportação com valores ausentes e apagamento parcial e total.

### RF5 — Exibição no desktop (prioridade média)

- **RF5.1** Comando Tauri `account_history(index, metric_key, range)` devolve a série calculada pelo core, sem segredo no IPC; o descarte de respostas de identidade antiga segue o padrão do catálogo de modelos.
- **RF5.2** Nos detalhes da conta, cada métrica com histórico exibe um gráfico compacto de área (24 h, 7 dias, 30 dias) com ponto final destacado e marcadores de reinício. Acessível: tabela equivalente oculta para leitores de tela e `prefers-reduced-motion` respeitado (PRD 0004).
- **RF5.3** Em **Configurações → Histórico**: ligar e desligar a gravação (ligado por padrão), retenção, tamanho em disco, exportar e apagar. Desligar para de gravar e mantém os dados até o usuário apagar.
- **Critério de aceite:** smoke em três idiomas com histórico de vários dias, conta nova sem histórico (estado vazio explicado), gravação desligada e apagamento.

### RF6 — Exibição na TUI e na CLI (prioridade média)

- **RF6.1** Nos detalhes da conta da TUI, linha com sparkline em blocos Unicode (`▁▂▃▄▅▆▇█`) por métrica no intervalo escolhido (teclas para 24 h, 7 d e 30 d). Largura reduzida degrada para texto com mínimo, máximo e último valor.
- **RF6.2** `cuotai --history [CONTA] [--history-days N] [--json]` imprime a série e sai com 0, 2 ou 1 como as demais saídas não interativas. Sem histórico, informa que não há dados em vez de falhar.
- **Critério de aceite:** testes de renderização em `TestBackend` e smoke da CLI com banco de exemplo.

## 6. Requisitos não funcionais

- **Privacidade:** os dados são numéricos e ficam no computador do usuário. O arquivo de histórico nunca entra no TOML nem em logs; a exportação é explícita.
- **Desempenho:** gravação em lote, com índice em `(account_digest, metric_key, taken_at)`; leitura de 30 dias de uma métrica abaixo de 50 ms em hardware comum.
- **Compatibilidade:** sem alterar `Report`, `Snapshot` ou `--json`; contas sem histórico continuam funcionando.
- **Riscos:** o `label` das métricas pode mudar entre versões de um adaptador e quebrar a continuidade da série; isso é documentado e a série nova começa limpa.

## 7. Ordem de execução e verificação

1. **RF1 e RF3:** armazenamento e consultas com testes de tabela; nenhuma mudança visível.
2. **RF2:** gravação ligada ao `Monitor`; teste de que o ciclo não atrasa.
3. **RF4:** retenção, exportação e limpeza.
4. **RF5 e RF6:** desktop, TUI e CLI, um de cada vez, com smoke.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando, após fechar e reabrir o aplicativo, a série dos últimos dias está disponível nas três superfícies, o crescimento do arquivo respeita a retenção e o teto, apagar o histórico o remove por completo, e nenhum relatório de erro ou segredo é gravado.
