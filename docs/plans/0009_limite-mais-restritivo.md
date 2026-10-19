# PRD 0009 — Indicador do limite mais restritivo

| Campo | Valor |
|---|---|
| ID | 0009 |
| Status | Proposto |
| Responsável | `cuotai-core`, Desktop e TUI |
| Consumidores | `apps/desktop` (bandeja, cartões, popover), `crates/cuotai-tui` (`dashboard.rs`), PRDs 0007, 0012 e 0014 |
| Depende de | nenhum |
| Data | 2026-10-19 |

## 1. Contexto

Uma conta pode ter várias métricas de quota ao mesmo tempo (janela de 5 horas, semana, mês, limites por modelo). `metrics::classify_report` já devolve `AccountClass::Exhausted` quando **qualquer** métrica mensurável está consumida, e `classify_metric` calcula o estado de cada barra (`MetricState`). Porém não existe uma noção única de "qual limite está me travando agora". Cada interface escolhe sozinha o que destacar: a bandeja do desktop mostra anéis (donuts) e a TUI mostra a lista de métricas na ordem do adaptador.

O README do WhatsMyUsage descreve o problema de forma concreta: barras que exibem só a janela de 5 horas marcavam 0% enquanto a conta estava bloqueada pelo limite semanal em 100%. O app exibe sempre o pior limite. O CodexBar também prioriza a "quota geral mais restritiva" nos widgets.

## 2. Problema

- O resumo de uma conta pode mostrar "livre" enquanto a conta está bloqueada por outra janela.
- Cada interface teria que reimplementar a regra de escolha (proibido pelas diretrizes do projeto: regras de negócio ficam no core).
- Sem um valor único por conta, bandeja, ordenação e widgets não têm o que exibir de forma compacta.

## 3. Objetivos

- Definir no core, por conta, a métrica **vinculante** (a que mais restringe o uso agora) e o instante em que a conta volta a ser utilizável.
- Usar o mesmo resultado na bandeja, nos cartões, na TUI, na CLI e nas exportações.
- Nunca esconder uma métrica bloqueante nem inventar folga quando faltam dados.

## 4. Não-objetivos

- Mudar a classificação existente (`AccountClass`, `MetricState`) ou o que cada adaptador reporta.
- Somar ou fazer média de métricas de natureza diferente.
- Ritmo (PRD 0007) e alertas (PRD 0008), que consomem este resultado, mas não fazem parte dele.

## 5. Requisitos funcionais

### RF1 — Seleção da métrica vinculante no core (prioridade alta; pré-requisito)

- **RF1.1** Criar `metrics::binding(report: &Report) -> Option<Binding>`, função pura, usando a mesma elegibilidade de `classify_metric`. `Binding` é `Serialize` e traz: índice e rótulo da métrica, `window`, fração consumida, `remaining`, `resets_at` e o motivo (`Exhausted` ou `Tightest`).
- **RF1.2** Elegibilidade: `kind = Quota` com teto finito e positivo e sinal finito (`used` ou `remaining`). `RateLimit` entra somente se nenhuma `Quota` for elegível na conta. `Balance`, `Credits`, `Spend` e `Usage` nunca entram.
- **RF1.3** Escolha:
  - se alguma métrica elegível está esgotada, a conta está **bloqueada**: o resultado é a métrica esgotada cujo retorno (`resets_at`) é o mais tardio, porque a conta só volta a ser utilizável quando todas as esgotadas reiniciarem; `blocked_until` recebe esse instante;
  - caso contrário, vence a métrica com menor fração restante (maior consumo); empate vai para a janela mais curta e, depois, para a ordem do adaptador;
  - `resets_at` ausente numa métrica esgotada deixa `blocked_until` como desconhecido, nunca como "agora".
- **RF1.4** Contas `Unmeasured` ou com `Status` diferente de `Ready` devolvem `None`; o estado da conta (erro, autenticação) continua sendo a informação principal.
- **RF1.5** Ao ordenar ou resumir várias contas, `binding_ratio(report)` fornece a fração restante da vinculante; contas sem valor ficam por último.
- **Critério de aceite:** testes de tabela: 5 h livre e semana esgotada (resultado: semana, bloqueada, retorno da semana); duas esgotadas com retornos diferentes; nenhuma esgotada com três janelas; empate; rate limit só; métricas sem teto; valores ausentes e não finitos; `used = 0, limit = 0` como sentinela desconhecida.

### RF2 — Desktop (prioridade alta)

- **RF2.1** Comando Tauri que devolve a vinculante de cada conta junto do estado atual. A bandeja, no modo anéis, usa a vinculante como valor principal do anel de cada conta; o modo lista mostra "bloqueada até HH:MM" quando aplicável.
- **RF2.2** O cartão exibe um chip com o rótulo da janela vinculante ("Semana · 100%") ao lado do estado, e destaca esse item na lista de métricas. As demais métricas continuam visíveis.
- **RF2.3** Ordenação opcional "mais restritivas primeiro" nas Configurações; a ordem manual (arrastar e soltar, sincronizada com a TUI) continua sendo o padrão e nunca é sobrescrita.
- **Critério de aceite:** smoke em três idiomas com conta bloqueada por janela semanal e livre na de 5 h; o anel e o chip mostram a semana.

### RF3 — TUI e CLI (prioridade média)

- **RF3.1** A linha da conta no painel mostra a vinculante em vez da primeira métrica; nos detalhes, a vinculante recebe marcador.
- **RF3.2** `--once` imprime uma linha de resumo por conta: `Claude  BLOQUEADA até 14:30 (semana 100%)` ou `Claude  semana 62%`. `--json` permanece igual; a vinculante entra no contrato versionado do PRD 0012.
- **RF3.3** Códigos de saída não mudam.
- **Critério de aceite:** testes de renderização em `TestBackend` e snapshot textual de `--once`.

### RF4 — Textos (prioridade média)

- **RF4.1** O core emite `Localized` para o resumo ("bloqueada até {at} pela janela {window}", "mais restritiva: {window} {used}%"), com ids estáveis e catálogos `en-US` e `es`.
- **Critério de aceite:** teste de catálogo completo.

## 6. Requisitos não funcionais

- **Honestidade:** a vinculante nunca esconde dados; é uma ênfase, não um filtro. Ausência de dado não vira zero nem "100% livre".
- **Consistência:** todas as superfícies leem o mesmo resultado do core.
- **Custo:** nenhuma consulta nova.

## 7. Ordem de execução e verificação

1. **RF1 e RF4:** função e textos com testes de tabela.
2. **RF2:** desktop.
3. **RF3:** TUI e CLI.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando uma conta com a janela de 5 horas livre e a semanal esgotada aparece como bloqueada, com o horário de retorno correto, na bandeja, no cartão, na TUI e em `--once`, e nenhuma interface contém regra própria de escolha.
