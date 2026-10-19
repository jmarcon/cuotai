# PRD 0008 — Alertas de consumo por limiar

| Campo | Valor |
|---|---|
| ID | 0008 |
| Status | Proposto |
| Responsável | `cuotai-core`, Desktop (TUI opcional) |
| Consumidores | `apps/desktop`, `crates/cuotai-tui`, PRD 0013 (eventos) |
| Depende de | PRD 0003 (RF2 e RF3: entregador `Notifier`, permissão, `notified.json` e preferências); o PRD 0006 é opcional |
| Data | 2026-10-19 |

## 1. Contexto

O PRD 0003 trata do aviso de **reinício** de quota e deixa explicitamente para um PRD próprio os "avisos de consumo (abaixo de 20%)". Hoje a única indicação de aproximação do limite é visual: `metrics::classify_report` marca uma conta como `Exhausted` e os cartões ganham a classe `exhausted` no desktop. Nada avisa antes de a quota acabar, nem enquanto o app está escondido na bandeja.

O PRD 0003 prepara a infraestrutura reaproveitável: `Notifier` atrás de um trait, plugin de notificação com permissão, agendador no backend Tauri, persistência anti-duplicação em `notified.json` e a seção **Configurações → Notificações**. Na análise de mercado de 2026-10-19, vários concorrentes avisam por limiar: o Usagebar em 50/75/90% e o QuoPeek com limiares e repetição configuráveis.

## 2. Problema

Quem trabalha com janelas de 5 horas e semanais descobre que a quota acabou quando o trabalho para. Alertar ingenuamente gera ruído:
- uma métrica oscila ao redor do limiar a cada consulta;
- várias métricas da mesma conta cruzam limiares juntas;
- reiniciar o app reenviaria avisos antigos;
- métricas sem teto (saldo, gasto) não têm "percentual" e não podem disparar.

## 3. Objetivos

- Avisar, uma vez por janela e por limiar, quando o consumo de uma métrica com teto cruza um limiar escolhido pelo usuário.
- Avisar quando a quota se esgota e quando volta a ter folga na janela seguinte, sem repetir após reinício do app.
- Reaproveitar o entregador, a permissão e o armazenamento do PRD 0003, sem nova consulta ao provedor.

## 4. Não-objetivos

- Aviso de reinício (PRD 0003) e aviso baseado em ritmo (PRD 0007), que podem virar uma opção futura.
- E-mail, push remoto, webhooks (PRD 0013) e avisos com o aplicativo encerrado.
- Alertar sobre saldo, gasto acumulado ou rate limit curto.
- Modo "não perturbe" por horário nesta versão.

## 5. Requisitos funcionais

### RF1 — Avaliador de limiares no core (prioridade alta; pré-requisito)

- **RF1.1** Criar `alerts::evaluate(prev: Option<&Report>, next: &Report, prefs: &AlertPrefs, state: &AlertState, now) -> Vec<AlertEvent>`, função pura e sem relógio interno. `AlertEvent` traz a chave da conta, o rótulo e a janela da métrica, o limiar cruzado, a fração consumida, `resets_at` e o tipo (`Threshold`, `Exhausted`, `Recovered`).
- **RF1.2** Elegibilidade: `kind = Quota` com `MetricState::Available { uncapped: false }` ou `Exhausted` com teto positivo. Métricas sem teto, `Balance`, `Credits`, `Spend`, `Usage` e `RateLimit` nunca disparam. Valores ausentes ou não finitos são ignorados, nunca tratados como zero.
- **RF1.3** Limiares configuráveis entre 10 e 99% de consumo (padrão 75 e 90). O aviso de esgotamento (100%) é sempre avaliado quando os alertas estão ligados.
- **RF1.4** Identidade e deduplicação: a chave é conta + métrica + instância da janela + limiar, com a mesma tolerância de 2 minutos para `resets_at` do PRD 0003, RF1.3. Um limiar dispara uma vez por instância de janela. Cruzar vários limiares numa única consulta emite só o mais alto.
- **RF1.5** Histerese: um limiar só rearma quando o consumo cai 5 pontos abaixo dele ou quando começa uma nova instância de janela. Oscilar em torno do limiar não gera avisos repetidos.
- **RF1.6** `Recovered`: quando uma métrica esgotada ganha folga numa nova janela, emite um único aviso, desde que o usuário tenha recebido o de esgotamento daquela métrica.
- **RF1.7** Primeira leitura: sem `prev`, o avaliador não emite aviso de limiar já ultrapassado no passado (evita rajada ao abrir o app). Emite apenas se a leitura é nova e dentro de 10 minutos do cruzamento conhecido (usando o histórico do PRD 0006, quando existir).
- **Critério de aceite:** testes de tabela com relógio fixo cobrem métrica sem teto, valor ausente, cruzar um e vários limiares, oscilação, rearme por queda e por nova janela, deriva de `resets_at`, `Recovered`, primeira leitura e conta removida.

### RF2 — Persistência e entrega (prioridade alta)

- **RF2.1** O estado dos avisos já emitidos (chaves e datas) é gravado no mesmo `notified.json` do PRD 0003, com poda após 8 dias; só digests, nunca valores de uso, e-mails ou nomes de conta.
- **RF2.2** O backend Tauri chama o avaliador depois de cada `Report` aplicado ao `RefreshCycle`, não o JavaScript. A entrega usa o `Notifier` do PRD 0003 e agrupa em uma única notificação os eventos da mesma conta no mesmo ciclo.
- **RF2.3** Conteúdo: "Claude: 90% da janela de 5 h usada · reinicia 14:30". Nunca inclui e-mail, token, URL ou notas do provedor. Textos em `pt-BR`, `en-US` e `es`.
- **Critério de aceite:** testes do agendador com relógio e entregador falsos: um aviso por limiar e janela, nenhum aviso repetido após reiniciar o app, agrupamento por conta, alertas desligados no meio da espera e permissão negada.

### RF3 — Preferências no desktop (prioridade alta)

- **RF3.1** Em **Configurações → Notificações**: chave **Alertas de consumo** (desligada por padrão), lista de limiares (chips editáveis de 10 a 99), **Avisar esgotamento** e **Avisar retorno**. Preferências em `desktop.json`, nunca no `cuotai.toml`.
- **RF3.2** Por conta e por métrica: silenciar uma conta (sino do PRD 0003, RF3.2) e silenciar uma métrica específica nos detalhes. Chaves pela identidade estável da conta, podadas ao remover.
- **RF3.3** Reaproveitar o pedido de permissão e o botão de teste do PRD 0003; este PRD não duplica o fluxo.
- **RF3.4** A bandeja e o cartão destacam a conta com alerta ativo, mantendo o estado visual atual de `exhausted`.
- **Critério de aceite:** smoke no app empacotado (macOS e, quando disponível, Windows e Linux) com permissão concedida e negada, limiares personalizados e preferências persistentes após reiniciar.

### RF4 — Aviso na TUI (prioridade baixa; opcional)

- **RF4.1** Com `--notify` (o mesmo flag do PRD 0003, RF4), a TUI mostra a faixa de aviso e toca `BEL` para eventos do avaliador, sem persistência entre sessões.
- **Critério de aceite:** smoke em tmux com métrica de teste cruzando um limiar; `--once` e `--json` não mudam.

## 6. Requisitos não funcionais

- **Privacidade:** o texto da notificação aparece na tela bloqueada. Só nome do provedor, da conta e da métrica, nunca dados sensíveis.
- **Custo:** nenhum alerta dispara consulta; os avisos só reagem a relatórios já obtidos.
- **Ruído:** limite de no máximo uma notificação por conta por ciclo e uma por minuto no total, com excedente agrupado.

## 7. Ordem de execução e verificação

1. **RF1:** avaliador puro com testes de tabela; sem mudança visível.
2. **RF2:** integração ao backend, persistência compartilhada e agrupamento.
3. **RF3:** preferências, silenciamento e smoke no app empacotado.
4. **RF4 (opcional):** TUI.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando, com o app escondido na bandeja, uma métrica que cruza 90% gera **um** aviso por janela, oscilar em torno do limiar não repete, reiniciar o app não reenvia avisos antigos, desligar os alertas cancela os pendentes e nenhuma consulta extra é feita ao provedor.
