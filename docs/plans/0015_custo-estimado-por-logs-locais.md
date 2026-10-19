# PRD 0015 — Custo estimado a partir de logs locais

| Campo | Valor |
|---|---|
| ID | 0015 |
| Status | Proposto |
| Responsável | `cuotai-core`, Desktop e TUI |
| Consumidores | `apps/desktop`, `crates/cuotai-tui`, PRD 0012 (campo no contrato) |
| Depende de | PRD 0005 (rótulo de acesso `LocalCli`/estimativa); aproveita o PRD 0006 |
| Data | 2026-10-19 |

## 1. Contexto

Todas as consultas do Cuotai são **remotas**: quotas, saldos e faturas informados pelo provedor. Nada mede o que as ferramentas de linha de comando do próprio usuário (Claude Code, Codex CLI e similares) gastaram localmente. Essas ferramentas gravam transcrições de sessões em arquivos do usuário, de onde projetos como o ccusage derivam tokens e custo estimado por modelo.

Na análise de mercado de 2026-10-19, o CodexBar tem varredura de custo em logs locais do Codex e do Claude (`codexbar cost`), com período configurável, base SQLite com teto de entradas e de tamanho, e preço por modelo atualizado dinamicamente; o ClaudeBar mostra custo e tokens de hoje ao lado de ontem; o OpenUsage calcula Hoje, Ontem e 30 dias a partir de logs de CLI com tabela de preços atualizável; o OpenQuota registra histórico de tokens e gasto estimado.

## 2. Problema

Assinaturas com janela (Claude, Codex) não informam quanto uma sessão custaria em preço de API. Quem quer comparar o valor da assinatura com o custo equivalente, ou entender quais projetos e modelos consomem mais, não tem como no Cuotai. Ao mesmo tempo, ler logs locais é sensível: transcrições contêm código e prompts.

## 3. Objetivos

- Calcular, localmente e de forma opcional, tokens e custo estimado por dia, por modelo e por ferramenta a partir de logs de CLI, **sem nunca ler nem guardar o conteúdo das conversas**.
- Rotular o resultado como estimativa em preço de API, separado do faturamento real dos provedores.
- Funcionar offline, sem credenciais e sem rede (exceto atualização opcional da tabela de preços).

## 4. Não-objetivos

- Mostrar, indexar ou exportar prompts, respostas, caminhos de projeto ou nomes de arquivos de código.
- Custo de ferramentas que não gravam logs locais legíveis.
- Misturar a estimativa com `Spend` de provedores ou somar valores de moedas diferentes.
- Varredura contínua do disco: só os diretórios conhecidos, quando o recurso está ligado.

## 5. Requisitos funcionais

### RF1 — Fontes e consentimento (prioridade alta; pré-requisito)

- **RF1.1** Recurso desligado por padrão. Ao ligar, a interface mostra exatamente quais diretórios serão lidos, para que serve e que só números (modelo, contagens de tokens, horário) são extraídos. O consentimento é por fonte.
- **RF1.2** Fontes iniciais, com caminhos padrão **a confirmar na implementação** contra a versão atual de cada ferramenta e sobrescrevíveis em configuração: logs de sessão do Claude Code e do Codex CLI. Uma fonte é um adaptador com `id`, caminhos, padrão de arquivos e parser, atrás de um trait, para acrescentar outras depois.
- **RF1.3** Nenhum adaptador lê campos de conteúdo: o parser extrai apenas `timestamp`, `model`, contagens de tokens (entrada, saída, cache de leitura e de escrita) e um identificador de sessão **anônimo** (digest). Linhas inválidas são ignoradas e contadas.
- **RF1.4** Permissões do sistema: ler somente arquivos do próprio usuário; nunca elevar privilégios; erros de acesso (`EACCES`, volume ausente) viram estado "sem acesso", sem tentar contornar.
- **Critério de aceite:** testes com logs sintéticos que contêm "conteúdo sensível" de marcação; nenhum valor desses campos aparece no banco, no JSON, nos logs de depuração nem na interface.

### RF2 — Indexação incremental (prioridade alta)

- **RF2.1** Índice em SQLite próprio (`cost.sqlite3`, no diretório de dados com permissão `0600`), com tabela de uso agregado por `(fonte, dia local, modelo)` e uma tabela de controle por arquivo (digest do caminho, tamanho e posição já lidos). Reler só o que cresceu; arquivo reduzido ou trocado reinicia aquele arquivo.
- **RF2.2** Limites: teto de arquivos por varredura, teto de linhas por arquivo, tempo máximo por ciclo, prioridade baixa de CPU e de E/S, e pausa em modo de economia de energia, para não competir com o trabalho do usuário. A varredura roda em tarefa separada e nunca atrasa o refresh de quotas.
- **RF2.3** Período configurável (hoje, 7 dias, 30 dias, mês corrente, tudo o que está disponível) e retenção do índice (padrão 120 dias).
- **RF2.4** Apagar índice e desligar a fonte removem tudo o que foi derivado.
- **Critério de aceite:** testes de leitura incremental (crescimento, truncamento, rotação), idempotência, limites de tempo e de tamanho, e remoção completa.

### RF3 — Preços e cálculo (prioridade alta)

- **RF3.1** Tabela de preços por modelo embutida no binário (USD por milhão de tokens: entrada, saída, cache), com data de vigência e fonte. Modelo sem preço conhecido aparece como "custo desconhecido", nunca como zero.
- **RF3.2** Atualização opcional da tabela por download de um arquivo assinado ou por revisão manual do usuário em `pricing.json` (preferência do usuário vence a embutida). Sem atualização automática por padrão.
- **RF3.3** Cálculo em `cost::estimate(usage, prices) -> Estimate` no core, função pura. O resultado traz total, custo por modelo, tokens, parcela com preço desconhecido e a data da tabela usada.
- **RF3.4** O texto sempre diz "estimativa a preço de API, não é cobrança". Valores nunca somam moedas diferentes; tudo em USD.
- **Critério de aceite:** testes de tabela com tokens e preços conhecidos, modelo desconhecido, cache, arredondamento e tabela do usuário sobrescrevendo a embutida.

### RF4 — Exibição (prioridade média)

- **RF4.1** Desktop: cartão "Uso local" por fonte (hoje, ontem, período), no mesmo lugar para todas as contas daquela ferramenta; nos detalhes, detalhamento por dia e por modelo (gráfico de barras acessível com tabela equivalente). **Configurações → Custo local** controla fontes, caminhos, período, tabela de preços e apagar dados.
- **RF4.2** TUI: seção nos detalhes da conta e `cuotai --cost [--days N] [--json]`, com a mesma estimativa; códigos de saída como as demais saídas não interativas.
- **RF4.3** O resultado entra no contrato de exportação (PRD 0012) como `local_cost`, separado dos relatórios de provedores.
- **Critério de aceite:** smoke em três idiomas com fonte ligada, desligada e sem acesso; estado vazio explicado.

## 6. Requisitos não funcionais

- **Privacidade:** nenhum conteúdo é lido, guardado ou transmitido; o índice contém só agregados. O arquivo e a configuração ficam no computador do usuário, e a exportação é explícita.
- **Honestidade:** custo é estimativa em preço de API; assinaturas de preço fixo podem custar menos ou mais. Janelas incompletas são rotuladas.
- **Desempenho e energia:** varredura incremental, com limites e pausa; não degradar o sistema.
- **Riscos:** formatos de log mudam entre versões das ferramentas; cada adaptador tem testes de fixtures versionados e falha de forma isolada.

## 7. Ordem de execução e verificação

1. **RF1 e RF2:** adaptador de uma fonte, índice e limites, com logs sintéticos.
2. **RF3:** preços e cálculo.
3. **RF4:** desktop, TUI e CLI.
4. Segunda fonte, quando a primeira estiver estável.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando, com o recurso ligado, o app mostra tokens e custo estimado de hoje e do período sem ler conteúdo de conversas, modelos sem preço aparecem como desconhecidos, apagar os dados os remove, e desligar a fonte interrompe qualquer leitura de disco.
