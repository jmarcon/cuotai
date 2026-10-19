# PRD 0010 — Idade da medição e leitura desatualizada

| Campo | Valor |
|---|---|
| ID | 0010 |
| Status | Proposto |
| Responsável | `cuotai-core`, Desktop e TUI |
| Consumidores | `apps/desktop` (cartões, detalhes, bandeja), `crates/cuotai-tui`, PRDs 0012 e 0014 |
| Depende de | PRD 0006 (RF3: `latest`) para sobreviver ao reinício; o restante funciona sem ele |
| Data | 2026-10-19 |

## 1. Contexto

Cada `Report` tem `checked_at` (`crates/cuotai-core/src/model.rs`). Hoje:
- a TUI mostra `checked_at` apenas como hora absoluta em UTC nos detalhes (`dashboard.rs`);
- o desktop não exibe a idade da medição em nenhum lugar;
- `RefreshCycle::finish_account` substitui o relatório da conta pelo novo **mesmo quando ele é uma falha** (`slots[index] = Some(report)`). Uma consulta com erro de rede, autenticação ou rate limit apaga as métricas boas anteriores da tela e, no desktop, o cartão fica vazio exatamente quando o usuário mais precisa do último valor conhecido.

Na análise de mercado de 2026-10-19: o Quotum mostra há quanto tempo cada conta foi medida e agrupa as contas com quota semanal esgotada num bloco "Recarregando" com a hora de retorno; o CodexBar preserva a última leitura válida com a idade original quando uma consulta falha e nunca empresta dados de outra conta; o QuotaTempo declara que a atualização não contorna esperas do provedor.

## 2. Problema

- Não se sabe quão velho é o número exibido: um cartão pode mostrar o valor de 40 minutos atrás sem indicação.
- Uma falha transitória apaga o último valor útil, e o usuário não distingue "a conta está zerada" de "a consulta falhou".
- Contas esgotadas misturam-se com as demais, embora o que interesse nelas seja só quando voltam.

## 3. Objetivos

- Mostrar a idade da medição por conta, em todas as interfaces, calculada pelo core.
- Preservar a última leitura válida quando uma consulta falha, marcada como desatualizada e com a idade original, sem nunca promovê-la a "atualizada".
- Agrupar contas esgotadas com o horário de retorno.

## 4. Não-objetivos

- Reconsultar mais depressa por causa de dados velhos: o agendamento e o piso de 300 s do AWS Bedrock permanecem.
- Inventar valores ou emprestar a leitura de outra conta ou de outro produto.
- Mudar o formato de `Report` ou `--json` atual (a leitura desatualizada entra pelo contrato versionado do PRD 0012).

## 5. Requisitos funcionais

### RF1 — Idade e frescor no core (prioridade alta; pré-requisito)

- **RF1.1** Criar `freshness::assess(checked_at, interval, now) -> Freshness` (`Fresh`, `Aging`, `Stale`) e `age(checked_at, now) -> Duration`. Limiares derivados do intervalo efetivo da conta (`Account::effective_refresh_seconds`, já com o piso para provedores cobrados por consulta): `Fresh` até 1,5 vez o intervalo, `Aging` até 3 vezes, `Stale` acima disso. Relógio injetado, sem rede.
- **RF1.2** Idade negativa (relógio do sistema andou para trás) é tratada como zero e marcada como "relógio ajustado", nunca como erro.
- **Critério de aceite:** testes de tabela com intervalos de 30 s, 300 s e 3600 s e relógio fixo.

### RF2 — Última leitura válida (prioridade alta)

- **RF2.1** `RefreshCycle` passa a manter, por conta, a última leitura `Ready` (`last_good: Option<Report>`) **ao lado** do relatório corrente, sem alterar o que `snapshot()` e `account_report()` já devolvem. Uma nova API (`last_good_report(index)`) expõe essa leitura. Remoção, edição de identidade e reordenação seguem as regras já existentes de `Edit` (a leitura acompanha a conta movida e é descartada quando a identidade muda).
- **RF2.2** A leitura preservada só é oferecida quando o relatório corrente tem `Status` diferente de `Ready`. Ela nunca substitui o relatório corrente, nunca atualiza `checked_at` e nunca é usada para decidir alertas (PRD 0008) ou histórico (PRD 0006).
- **RF2.3** Com o histórico do PRD 0006, a última leitura válida sobrevive ao reinício do app e é carregada na abertura (`latest`), exibida como desatualizada até a primeira consulta terminar. Sem histórico, vale só durante a sessão.
- **RF2.4** Falha de autenticação invalida a leitura preservada da conta (o dado pertence a uma credencial que não vale mais); `RateLimited` e erros de rede a mantêm.
- **Critério de aceite:** testes do ciclo: falha após sucesso mantém a leitura com a idade original; sucesso seguinte a substitui; edição de credencial a descarta; mover a conta a preserva; falha de autenticação a descarta; relatório corrente e `snapshot()` permanecem inalterados.

### RF3 — Exibição no desktop (prioridade alta)

- **RF3.1** Cada cartão mostra "há 3 min" (texto relativo atualizado a cada minuto pelo front, calculado a partir de `checked_at` entregue pelo backend) e um indicador de frescor (`Fresh`, `Aging`, `Stale`) com ícone e texto, não só cor.
- **RF3.2** Quando há `last_good` e o relatório corrente falhou, o cartão exibe as métricas antigas com selo "Desatualizado · há 42 min" e a causa da falha (mensagem localizada já existente). As barras ficam atenuadas e sem animação.
- **RF3.3** Bandeja e popover repetem a idade e o selo. O tooltip do ícone indica a idade da conta mais antiga.
- **Critério de aceite:** smoke em três idiomas com conta saudável, envelhecendo, em falha com leitura preservada e em falha sem leitura; o selo desaparece na consulta seguinte bem-sucedida.

### RF4 — Grupo "recarregando" (prioridade média)

- **RF4.1** Contas com `AccountClass::Exhausted` (ou bloqueadas segundo o PRD 0009, quando existir) formam um grupo recolhível "Recarregando" com o horário de retorno (`blocked_until`). Contas sem `resets_at` conhecido ficam no grupo com "retorno desconhecido".
- **RF4.2** O agrupamento é uma opção em **Configurações → Aparência** (desligada por padrão), preserva a ordem manual dentro de cada grupo e nunca altera a ordem salva em `cuotai.toml`.
- **Critério de aceite:** smoke com duas contas esgotadas e uma livre; o grupo recolhe e expande por teclado e mouse; a ordem manual persiste.

### RF5 — TUI e CLI (prioridade média)

- **RF5.1** Painel e detalhes mostram a idade relativa; contas em falha com leitura preservada marcam "(desatualizado)". Hora absoluta continua disponível nos detalhes, agora também no fuso local.
- **RF5.2** `--once` imprime a idade; `--json` permanece igual.
- **Critério de aceite:** testes de renderização com relógio fixo.

## 6. Requisitos não funcionais

- **Honestidade:** dado antigo é sempre rotulado como antigo. Uma leitura preservada nunca vira `Ready`, nunca recalcula `checked_at` e nunca aparece no JSON atual como se fosse nova.
- **Privacidade:** a leitura preservada em disco (RF2.3) segue as regras do PRD 0006.
- **Custo:** nenhuma consulta nova nem mudança de agenda.

## 7. Ordem de execução e verificação

1. **RF1:** frescor puro no core, com testes.
2. **RF2:** `last_good` no ciclo, com testes de edição, remoção e reordenação.
3. **RF3:** desktop com smoke.
4. **RF5 e RF4:** TUI, CLI e agrupamento.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando uma consulta com erro após um sucesso mostra as métricas anteriores com a idade original e a causa da falha, uma consulta bem-sucedida remove o selo, a falha de autenticação descarta o valor antigo, e nenhum relatório atual, snapshot ou saída `--json` muda de forma.
