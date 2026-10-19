# PRD 0003 — Notificações de reinício de quota

| Campo | Valor |
|---|---|
| ID | 0003 |
| Status | Proposto |
| Responsável | `cuotai-core` e Desktop (TUI opcional) |
| Consumidores | `apps/desktop`, `crates/cuotai-tui` |
| Data | 2026-10-19 |

## 1. Contexto

Cada `Metric` pode trazer `resets_at: Option<DateTime<Utc>>` e `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other`) em `crates/cuotai-core/src/model.rs`. Os cartões já mostram "em 4h 10m", mas nada avisa quando o momento chega. No desktop, fechar a janela só a esconde (`lib.rs`: `CloseRequested` → `prevent_close` + `hide`) e `Backend::run_scheduler` continua rodando com o ícone na bandeja. Ou seja, o processo fica vivo para disparar avisos. Hoje o desktop depende só de `tauri-plugin-dialog`, não tem permissão nem plugin de notificação (`capabilities/default.json`), e as preferências dele ficam em `desktop.json` (`settings.rs`: `user_dir`, `config_file`).

## 2. Problema

Quem usa assinaturas com janela (Claude 5h/semana, Codex, Copilot, Kimi…) precisa voltar ao app para saber se a quota já reiniciou. Avisar ingenuamente gera ruído e falsos alarmes:
- janelas de rate limit reiniciam a cada minuto;
- `resets_at` de janelas móveis muda alguns segundos ou minutos entre consultas;
- reiniciar o app pode reenviar avisos antigos;
- muitas contas podem reiniciar no mesmo minuto.

## 3. Objetivos

- Ligar e desligar notificações do sistema operacional sobre o reinício de quota, globalmente e por conta, no desktop.
- Avisar no horário do reinício ou com antecedência escolhida, uma única vez por reinício, mesmo após reiniciar o app.
- Usar só dados já consultados: nenhuma chamada extra ao provedor por causa de uma notificação.

## 4. Não-objetivos

- Consultar o provedor no momento do reinício para confirmar o novo saldo. Há custo por consulta (AWS Cost Explorer) e rate limits; o ciclo normal de refresh continua o mesmo.
- Notificar com o app encerrado, iniciar com o sistema, push remoto ou e-mail.
- Avisos de consumo ("abaixo de 20%"), que ficam para um PRD próprio, e mudanças em `Report`, `Snapshot` ou na saída `--json`.
- Inventar horário de reinício quando o provedor não informa `resets_at`.

## 5. Requisitos funcionais

### RF1 — Planejador de reinícios no core (prioridade alta; pré-requisito)

- **RF1.1** Criar em `cuotai-core` uma função pura, sem rede e sem relógio interno, do tipo `resets::upcoming(config, reports, now, &Prefs) -> Vec<ResetEvent>`. Cada `ResetEvent` traz índice da conta, chave estável da conta, rótulo e janela da métrica, `resets_at` e o horário do aviso (`resets_at − antecedência`). TUI e desktop usam a mesma lógica.
- **RF1.2** Elegibilidade:
  - a métrica tem `resets_at` futuro, `kind = quota` e janela `window_5h`, `daily`, `weekly` ou `monthly`;
  - `other` só entra se o usuário habilitar;
  - `rate_limit` e `balance` nunca entram;
  - filtro "quando avisar": **Só se usada** (padrão: `used > 0` ou `remaining < limit`), **Só se esgotada** ou **Sempre**;
  - valor ausente não conta como usado nem como esgotado.
- **RF1.3** Identidade e deduplicação:
  - a chave do reinício é a chave da conta + rótulo da métrica + `resets_at`;
  - dois `resets_at` da mesma métrica com até 2 minutos de diferença são o mesmo reinício (deriva das janelas móveis);
  - se uma consulta posterior adiar o `resets_at` além da tolerância, o aviso antigo é cancelado e o novo é agendado;
  - métricas da mesma conta que reiniciam no mesmo minuto viram um único evento agrupado.
- **Critério de aceite:** testes de tabela com relógio fixo cobrem sem `resets_at`, `resets_at` passado, rate limit, janela `other` desligada e ligada, os três filtros, valores ausentes, deriva menor e maior que a tolerância, agrupamento e conta desligada.

### RF2 — Agendamento e entrega no desktop (prioridade alta)

- **RF2.1** O backend Tauri agenda os avisos, não o JavaScript: a webview escondida pode ser suspensa. Depois de cada refresh (conta ou ciclo), de mudanças de configuração e das preferências, o backend recalcula a próxima ocorrência e dorme até ela com `tokio::time::sleep_until`, junto do `run_scheduler` existente. A entrega fica atrás de um trait `Notifier` para testes com relógio e entregador falsos.
- **RF2.2** Entrega pelo `tauri-plugin-notification` da linha 2.x (compatível com Tauri 2.12; a 3.x ainda é alpha) e permissão correspondente em `capabilities/default.json`.
  - Título: "Claude Code: quota de 5h reiniciada" (ou "reinicia em 15 min").
  - Corpo: nome da conta e, quando houver, as métricas agrupadas.
  - Nunca incluir e-mail, tokens, URLs ou notas do provedor.
  - Textos em `pt-BR`, `en-US` e `es`, no idioma escolhido no app.
- **RF2.3** Persistência anti-duplicação:
  - guardar em `<dados do SO>/cuotai/notified.json` as chaves já avisadas (só digests e datas, poda após 8 dias);
  - ao abrir o app, um reinício passado há mais de 10 minutos não é avisado ("perdido enquanto fechado"); dentro da janela, é avisado uma vez;
  - mudar o relógio do sistema ou acordar de suspensão não gera avisos em rajada: no máximo um evento agrupado por conta.
- **Critério de aceite:** testes do agendador com relógio falso cobrem aviso no horário, antecedência, cancelamento ao adiar, reinício do app antes e depois do horário, suspensão longa, conta removida e notificações desligadas no meio da espera.

### RF3 — Preferências e permissão no desktop (prioridade alta)

- **RF3.1** Criar a seção **Configurações → Notificações** com:
  - **Notificar reinício de quota** (desligado por padrão);
  - antecedência **No horário / 5 / 15 / 60 min**;
  - filtro **Só se usada / Só se esgotada / Sempre**;
  - **Incluir janelas "outras"**.
  
  Tudo fica em `desktop.json` (preferência do dispositivo, nunca no `cuotai.toml`), com escrita atômica como os demais campos de `settings.rs`.
- **RF3.2** Ligar e desligar por conta com um ícone de sino no cartão e nos detalhes. O estado fica em `desktop.json`, pela chave estável da conta (provedor + nome). Renomear a conta perde a preferência, e isso vai documentado. Contas removidas têm a preferência podada ao salvar a configuração. Ícone com `aria-pressed` e rótulo acessível.
- **RF3.3** Pedir a permissão do sistema operacional só quando o usuário liga a opção global. Se ela for negada, mostrar o estado, uma orientação para abrir os ajustes do sistema e manter a opção desligada. O botão **Enviar notificação de teste** valida o fluxo.
- **RF3.4** A bandeja e o tooltip mostram o próximo reinício avisado ("Próximo aviso: Claude 5h às 14:30") quando houver.
- **Critério de aceite:** smoke no app empacotado no macOS (permissão concedida e negada, aviso real chegando com a janela escondida), Windows e Linux quando disponíveis; preferências sobrevivem ao reinício; nos três idiomas.

### RF4 — Aviso na TUI (prioridade baixa; opcional)

- **RF4.1** Com `--notify` (desligado por padrão), enquanto a TUI estiver aberta, reusar o planejador do RF1:
  - faixa no cabeçalho ("Claude Code: 5h reiniciou");
  - sino do terminal (`BEL`);
  - opcionalmente, notificação por OSC 9 nos terminais que a suportam (iTerm2, WezTerm, Ghostty).
  
  Sequências desconhecidas não podem sujar a tela. Sem persistência: o aviso vale só para a sessão aberta.
- **Critério de aceite:** smoke em tmux com métrica de teste reiniciando em 1 minuto; `--once`/`--json` não mudam.

## 6. Requisitos não funcionais

- **Privacidade:** o conteúdo das notificações aparece na central do sistema e na tela bloqueada. Só nome do provedor, nome da conta e rótulo da métrica, nunca credenciais, e-mails ou respostas.
- **Custo e honestidade:** notificação não dispara consulta. O texto diz "reiniciada pelo horário informado pelo provedor"; o novo saldo só aparece no próximo refresh.
- **Energia:** um único temporizador até o próximo evento, sem polling por segundo no backend.
- **Riscos a verificar antes do RF2:**
  - no macOS, a notificação só tem nome e ícone corretos no app empacotado (`dev.cuotai.desktop`); em `tauri dev` ela pode não aparecer;
  - clique na notificação para abrir o cartão tem suporte limitado no desktop pelo plugin; se não for confiável, fica fora do escopo.

## 7. Ordem de execução e verificação

1. **RF1:** planejador puro no core com testes de tabela; nenhuma mudança visível.
2. **RF2:** agendador e entregador atrás do trait, persistência anti-duplicação, plugin e permissão; testes com relógio falso.
3. **RF3:** preferências, sino por conta, permissão, teste e bandeja; smoke no app empacotado.
4. **RF4 (opcional):** `--notify` na TUI.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop, e smoke da interface alterada antes da versão.

O requisito só está entregue quando, com o app escondido na bandeja, uma conta com quota usada gera **um** aviso no horário (ou na antecedência) escolhido, nenhum aviso se repete após reiniciar o app, e desligar a opção global ou o sino da conta cancela os pendentes, sem nenhuma consulta extra ao provedor.
