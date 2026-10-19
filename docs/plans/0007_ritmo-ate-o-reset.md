# PRD 0007 — Ritmo de consumo até o reset

| Campo | Valor |
|---|---|
| ID | 0007 |
| Status | Proposto |
| Responsável | `cuotai-core`, Desktop e TUI |
| Consumidores | `apps/desktop` (cartões, detalhes, bandeja), `crates/cuotai-tui`, PRDs 0008, 0012 e 0014 |
| Depende de | nenhum; o PRD 0006 melhora a projeção, mas não é exigido |
| Data | 2026-10-19 |

## 1. Contexto

Cada `Metric` traz `used`, `limit`, `remaining`, `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other`) e `resets_at` (`crates/cuotai-core/src/model.rs`). `metrics::classify_metric` produz um `MetricState` (`Available { ratio, uncapped }`, `Exhausted`, `Unmeasured`) que só diz quanto resta, não se o ritmo atual vai durar até o reset. Os cartões mostram "em 4h 10m" para o reinício e a barra de consumo, mas nada compara os dois.

O CodexBar publica o conceito de ritmo ("6% in reserve · Expected 47% used · Lasts until reset"), o OpenUsage e o OpenQuota exibem indicadores de ritmo, e o QuotaTempo faz do ritmo o produto inteiro (saldo semanal W, alvo P e diferença ±).

## 2. Problema

Saber que restam 40% não diz se isso é folga ou perigo: depende de quanto da janela já passou. Quem usa Claude (5 h e semana), Codex, Copilot ou Kimi precisa decidir se começa uma tarefa longa agora. Sem ritmo, o usuário faz a conta de cabeça a cada consulta.

## 3. Objetivos

- Calcular no core, por métrica elegível, o consumo esperado em ritmo uniforme, a diferença para o consumo real e se o ritmo atual esgota a janela antes do reset.
- Exibir isso de forma curta nos cartões, nos detalhes, na bandeja e na CLI.
- Nunca inventar ritmo quando faltar dado: omitir é preferível a estimar mal.

## 4. Não-objetivos

- Prever consumo futuro por aprendizado, sazonalidade ou horário de trabalho.
- Alertar (PRD 0008) ou recomendar troca de provedor.
- Métricas de saldo, gasto acumulado ou rate limit curto, que não têm janela de consumo uniforme.
- Mudar `Report`, `Snapshot` ou a saída `--json` atual; o ritmo é derivado na leitura.

## 5. Requisitos funcionais

### RF1 — Cálculo puro no core (prioridade alta; pré-requisito)

- **RF1.1** Criar `pace::assess(metric: &Metric, now: DateTime<Utc>, recent: Option<&[Point]>) -> Option<Pace>`, função pura, sem rede e sem relógio interno. `Pace` é `Serialize` e traz: `elapsed_fraction`, `expected_used_fraction`, `used_fraction`, `delta_points` (real menos esperado, em pontos percentuais), `stage`, `projected_exhaustion_at: Option<DateTime<Utc>>` e `lasts_until_reset: bool`.
- **RF1.2** Elegibilidade: `kind = Quota`, `window` em `window_5h`, `daily`, `weekly` ou `monthly`, `limit` finito e positivo, `used` finito, `resets_at` futuro. `RateLimit`, `Balance`, `Credits`, `Spend`, `Usage` e `other` ficam de fora. Ausência de qualquer dado devolve `None`, nunca um valor padrão.
- **RF1.3** Duração da janela: `window_5h` = 5 h, `daily` = 24 h, `weekly` = 7 dias. `monthly` = intervalo do mês civil que termina em `resets_at` (28 a 31 dias). O início é `resets_at − duração`. Se o provedor devolver um início conhecido em `period`, ele tem precedência quando coerente.
- **RF1.4** Consumo esperado = `elapsed / duração`. Estágios, com histerese de 5 pontos percentuais:
  - `Reserve` (`delta_points ≤ −5`): abaixo do ritmo;
  - `OnPace` (entre −5 e +5);
  - `Ahead` (`delta_points ≥ +5`): acima do ritmo;
  - `Exhausted` (`used ≥ limit`).
- **RF1.5** Projeção: `projected_exhaustion_at = início + elapsed × limit / used` quando `used > 0`. Se `used` é zero, ou a projeção cai depois de `resets_at`, `lasts_until_reset = true` e não há data de esgotamento. Com `recent` (PRD 0006), usar a inclinação das últimas 2 horas (ou 10% da janela, o que for maior) quando houver pelo menos 3 amostras; caso contrário, a média desde o início da janela. A fonte usada é registrada em `Pace`.
- **RF1.6** Janela recém-iniciada: se passou menos de 5% da duração ou menos de 10 minutos, não há avaliação (`None`), para evitar projeções absurdas de um único prompt.
- **Critério de aceite:** testes de tabela com relógio fixo cobrem janelas de 5 h, diária, semanal e mensal (fevereiro e mês de 31 dias), `used = 0`, `used ≥ limit`, `limit` zero ou ausente, `resets_at` passado, valores não finitos, janela recém-iniciada, projeção antes e depois do reset, histerese nos limiares e inclinação recente versus média.

### RF2 — Rótulos localizados e contrato de exibição (prioridade alta)

- **RF2.1** O core emite `Localized` com ids estáveis (`pace.reserve`, `pace.on_pace`, `pace.ahead`, `pace.exhausted`, `pace.lasts_until_reset`, `pace.runs_out_at`) e parâmetros (`delta`, `at`, `margin`). Textos de exemplo: "No ritmo · dura até o reset", "8 pontos de reserva", "Esgota às 16:40, 2 h antes do reset". Catálogos `en-US` e `es` completos; português no `fallback`.
- **RF2.2** O estágio tem tom semântico (`good`, `neutral`, `warn`, `bad`) em `Pace`, para a interface não decidir cor por texto. Tom e texto nunca dependem só da cor.
- **Critério de aceite:** teste de catálogo e teste de que `Pace` serializa de forma estável.

### RF3 — Desktop (prioridade média)

- **RF3.1** Comando Tauri que devolve o ritmo de cada métrica elegível de uma conta (ou de todas), calculado pelo core com o relógio do backend.
- **RF3.2** Cartão: uma linha curta sob a barra da métrica mais restritiva elegível (ver PRD 0009). Detalhes: ritmo de cada métrica, com a marca do consumo esperado desenhada sobre a barra e o horário projetado de esgotamento.
- **RF3.3** Bandeja: o tooltip e a visão em lista incluem "dura até o reset" ou "esgota às HH:MM".
- **RF3.4** Em **Configurações → Aparência**, opção de mostrar ou ocultar o ritmo (ligado por padrão). Preferência em `desktop.json`.
- **Critério de aceite:** smoke em três idiomas e nos temas claro e escuro, com uma métrica em cada estágio; a marca esperada não se sobrepõe ao texto em larguras estreitas.

### RF4 — TUI e CLI (prioridade média)

- **RF4.1** Detalhes da conta na TUI: coluna ou linha de ritmo por métrica; no painel, ícone de estágio discreto. Respeita larguras pequenas.
- **RF4.2** `--once` imprime a linha de ritmo sob cada métrica elegível. `--json` permanece igual; o ritmo entra no contrato de exportação versionado do PRD 0012.
- **Critério de aceite:** testes de renderização com `TestBackend` e snapshot textual de `--once`.

## 6. Requisitos não funcionais

- **Honestidade:** ritmo é uma estimativa linear. O texto e a documentação dizem isso; não prometem esgotamento exato. Janelas de rate limit rolantes e métricas sem início conhecido ficam fora.
- **Determinismo:** `assess` depende só de seus argumentos.
- **Custo:** nenhuma consulta nova ao provedor.

## 7. Ordem de execução e verificação

1. **RF1 e RF2:** cálculo e textos no core, com testes de tabela.
2. **RF3:** desktop com smoke.
3. **RF4:** TUI e CLI.

Em cada etapa: `cargo fmt`, `clippy -D warnings`, testes do workspace e do desktop e smoke da interface alterada.

O requisito só está entregue quando uma métrica semanal consumida acima do ritmo mostra a diferença e o horário de esgotamento nos três idiomas, uma janela recém-aberta não mostra nada, métricas sem janela ou sem `resets_at` não mostram ritmo inventado, e nenhuma consulta extra é feita.
