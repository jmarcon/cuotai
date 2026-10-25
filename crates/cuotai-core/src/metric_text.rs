//! PRD 0034 RF2: display texts for metric labels, units and periods.
//!
//! `Metric::label`, `Metric::unit` and `Metric::period` stay the strings
//! the adapters write: they are part of the metric's identity in the
//! history (`history::metric_key`), the reset ledger, the journal digests
//! and the `metrics = [...]` selectors of alert rules, so rewording them
//! would break those for existing users. Front ends render them through
//! [`metric_text`] instead, which recognizes the adapters' texts (exact or
//! with `{param}` slots) and returns a [`Localized`] with an English
//! `fallback`; the pt-BR catalog holds the adapter's text, so the
//! Portuguese rendering is unchanged. A text it does not recognize (a
//! provider-supplied name, a model id) comes back verbatim.

use crate::text::{Localized, ParamValue};
use std::borrow::Cow;
use std::collections::BTreeMap;

/// `(adapter text, id, English)`. `{name}` slots match any non-empty text
/// and become params; a slot named `label` is itself rendered through
/// [`metric_text`].
pub(crate) const TABLE: &[(&str, &str, &str)] = &[
    ("créditos", "metric.unit.credits", "credits"),
    ("solicitações", "metric.unit.requests", "requests"),
    ("requisições", "metric.unit.requisitions", "requests"),
    ("vozes", "metric.unit.voices", "voices"),
    ("unidades", "metric.unit.units", "units"),
    ("consultas", "metric.unit.queries", "queries"),
    ("membros", "metric.unit.members", "members"),
    (
        "ciclo de cobrança",
        "metric.period.billing_cycle",
        "billing cycle",
    ),
    (
        "saldo atual",
        "metric.period.current_balance",
        "current balance",
    ),
    ("semanal", "metric.period.weekly", "weekly"),
    ("mensal", "metric.period.monthly", "monthly"),
    ("diário", "metric.period.daily", "daily"),
    (
        "ciclo de assinatura",
        "metric.period.subscription_cycle",
        "subscription cycle",
    ),
    (
        "mês corrente",
        "metric.period.current_month",
        "current month",
    ),
    ("janela de 5h", "metric.period.window_5h", "5h window"),
    ("janela de 7d", "metric.period.window_7d", "7d window"),
    (
        "janela semanal",
        "metric.period.weekly_window",
        "weekly window",
    ),
    (
        "janela corrente do rate limit",
        "metric.period.rate_limit_window",
        "current rate-limit window",
    ),
    (
        "janela mensal do plano",
        "metric.period.plan_monthly_window",
        "plan's monthly window",
    ),
    (
        "janela do plano",
        "metric.period.plan_window",
        "plan window",
    ),
    (
        "janela móvel",
        "metric.period.rolling_window",
        "rolling window",
    ),
    (
        "ciclo mensal",
        "metric.period.monthly_cycle",
        "monthly cycle",
    ),
    (
        "ciclo de quota Qoder",
        "metric.period.qoder_cycle",
        "Qoder quota cycle",
    ),
    (
        "mês da assinatura gratuita",
        "metric.period.free_month",
        "free subscription month",
    ),
    ("últimos 10 min", "metric.period.last_10_min", "last 10 min"),
    (
        "30 dias concluídos (UTC)",
        "metric.period.last_30_days_utc",
        "30 completed days (UTC)",
    ),
    ("ciclo {cycle}", "metric.period.cycle", "cycle {cycle}"),
    ("Semanal", "metric.label.weekly", "Weekly"),
    (
        "Janela móvel",
        "metric.label.rolling_window",
        "Rolling window",
    ),
    (
        "Saldo em caixa (pode ser negativo)",
        "metric.label.cash_balance_signed",
        "Cash balance (may be negative)",
    ),
    (
        "Saldo em caixa",
        "metric.label.cash_balance",
        "Cash balance",
    ),
    (
        "Saldo disponível (Alibaba Cloud)",
        "metric.label.available_balance_alibaba",
        "Available balance (Alibaba Cloud)",
    ),
    (
        "Saldo disponível (KIMI_API_KEY)",
        "metric.label.available_balance_kimi",
        "Available balance (KIMI_API_KEY)",
    ),
    (
        "Saldo disponível",
        "metric.label.available_balance",
        "Available balance",
    ),
    (
        "Saldo da plataforma",
        "metric.label.platform_balance",
        "Platform balance",
    ),
    (
        "Saldo de créditos pré-pagos",
        "metric.label.prepaid_credit_balance",
        "Prepaid credit balance",
    ),
    (
        "Saldo de créditos",
        "metric.label.credit_balance",
        "Credit balance",
    ),
    (
        "Saldo — organização",
        "metric.label.balance_organization",
        "Balance — organization",
    ),
    (
        "Limite de crédito Alibaba (CreditAmount)",
        "metric.label.alibaba_credit_limit",
        "Alibaba credit limit (CreditAmount)",
    ),
    (
        "Valor devido (dívida)",
        "metric.label.amount_owed",
        "Amount owed (debt)",
    ),
    ("Crédito", "metric.label.credit", "Credit"),
    (
        "Créditos do plano",
        "metric.label.plan_credits",
        "Plan credits",
    ),
    (
        "Créditos extras usados",
        "metric.label.extra_credits_used",
        "Extra credits used",
    ),
    (
        "Custo do uso extra",
        "metric.label.extra_usage_cost",
        "Extra usage cost",
    ),
    (
        "Créditos restantes",
        "metric.label.credits_left",
        "Credits left",
    ),
    (
        "Créditos usados (total)",
        "metric.label.credits_used_total",
        "Credits used (total)",
    ),
    (
        "Créditos de prompt usados no ciclo",
        "metric.label.prompt_credits_cycle",
        "Prompt credits used in the cycle",
    ),
    (
        "Créditos adicionais usados no ciclo",
        "metric.label.add_on_credits_cycle",
        "Add-on credits used in the cycle",
    ),
    (
        "Créditos de IA utilizados (mês corrente)",
        "metric.label.ai_credits_month",
        "AI credits used (current month)",
    ),
    (
        "Créditos consumidos no mês (Perplexity Computer)",
        "metric.label.perplexity_computer_credits",
        "Credits consumed in the month (Perplexity Computer)",
    ),
    (
        "Créditos Codex — saldo restante",
        "metric.label.codex_credits_left",
        "Codex credits — remaining balance",
    ),
    (
        "Cursor plano (incluído)",
        "metric.label.cursor_plan_included",
        "Cursor plan (included)",
    ),
    (
        "Cursor modelos nomeados",
        "metric.label.cursor_named_models",
        "Cursor named models",
    ),
    (
        "Cursor plano baseado em requisições",
        "metric.label.cursor_request_plan",
        "Cursor request-based plan",
    ),
    (
        "Sob demanda (on-demand)",
        "metric.label.on_demand",
        "On demand",
    ),
    (
        "Gasto sob demanda no ciclo",
        "metric.label.on_demand_spend_cycle",
        "On-demand spending in the cycle",
    ),
    (
        "Gasto total no ciclo (inclui uso incluído)",
        "metric.label.total_spend_cycle",
        "Total spending in the cycle (includes included usage)",
    ),
    (
        "Gasto total da chave",
        "metric.label.key_total_spend",
        "Key's total spending",
    ),
    (
        "Gasto no mês",
        "metric.label.month_spend",
        "Spending in the month",
    ),
    (
        "Gasto na semana",
        "metric.label.week_spend",
        "Spending in the week",
    ),
    (
        "Gasto 30 dias — Outros",
        "metric.label.spend_30d_other",
        "30-day spending — Other",
    ),
    (
        "Gasto 30 dias — {name}",
        "metric.label.spend_30d",
        "30-day spending — {name}",
    ),
    ("Limite da chave", "metric.label.key_limit", "Key limit"),
    (
        "Limite atingido",
        "metric.label.limit_reached",
        "Limit reached",
    ),
    (
        "Plano de tokens MiMo",
        "metric.label.mimo_token_plan",
        "MiMo token plan",
    ),
    (
        "Grok CLI: semanal SuperGrok",
        "metric.label.grok_weekly_supergrok",
        "Grok CLI: SuperGrok weekly",
    ),
    (
        "Grok CLI: sob demanda",
        "metric.label.grok_on_demand",
        "Grok CLI: on demand",
    ),
    (
        "Grok CLI: pré-pago",
        "metric.label.grok_prepaid",
        "Grok CLI: prepaid",
    ),
    (
        "Grok CLI: mensal incluído",
        "metric.label.grok_monthly_included",
        "Grok CLI: monthly included",
    ),
    (
        "Gemini (Semanal)",
        "metric.label.gemini_weekly",
        "Gemini (Weekly)",
    ),
    (
        "Claude & GPT (Semanal)",
        "metric.label.claude_gpt_weekly",
        "Claude & GPT (Weekly)",
    ),
    (
        "Alocação de prompt da equipe (por assento × assentos)",
        "metric.label.team_prompt_allocation",
        "Team prompt allocation (per seat × seats)",
    ),
    (
        "Membros na equipe",
        "metric.label.team_members",
        "Team members",
    ),
    (
        "Sessão (5 horas)",
        "metric.label.session_5h",
        "Session (5 hours)",
    ),
    (
        "Ollama Cloud: sessão",
        "metric.label.ollama_session",
        "Ollama Cloud: session",
    ),
    ("Qoder: usuário", "metric.label.qoder_user", "Qoder: user"),
    (
        "Qoder: organização",
        "metric.label.qoder_org",
        "Qoder: organization",
    ),
    ("Requisições", "metric.label.requests", "Requests"),
    (
        "Requisições de modelo (completions)",
        "metric.label.model_requests",
        "Model requests (completions)",
    ),
    (
        "Tokens de entrada (mensagens)",
        "metric.label.input_tokens_messages",
        "Input tokens (messages)",
    ),
    (
        "Tokens de saída (mensagens)",
        "metric.label.output_tokens_messages",
        "Output tokens (messages)",
    ),
    (
        "Tokens de entrada (completions)",
        "metric.label.input_tokens_completions",
        "Input tokens (completions)",
    ),
    (
        "Tokens de saída (completions)",
        "metric.label.output_tokens_completions",
        "Output tokens (completions)",
    ),
    (
        "Modelos gratuitos (:free): requisições hoje",
        "metric.label.free_models_today",
        "Free models (:free): requests today",
    ),
    (
        "Solicitações premium utilizadas (mês corrente)",
        "metric.label.premium_requests_month",
        "Premium requests used (current month)",
    ),
    (
        "Custo líquido de solicitações premium (mês corrente)",
        "metric.label.premium_net_cost_month",
        "Net cost of premium requests (current month)",
    ),
    (
        "Custo líquido de créditos de IA (mês corrente)",
        "metric.label.ai_credits_net_cost_month",
        "Net cost of AI credits (current month)",
    ),
    (
        "Custo do recurso Azure (mês corrente)",
        "metric.label.azure_resource_cost",
        "Azure resource cost (current month)",
    ),
    (
        "Custo AWS Bedrock (mês corrente)",
        "metric.label.bedrock_cost",
        "AWS Bedrock cost (current month)",
    ),
    (
        "Custo no mês corrente (USD)",
        "metric.label.month_cost_usd",
        "Cost in the current month (USD)",
    ),
    (
        "Custo no mês corrente ({currency})",
        "metric.label.month_cost",
        "Cost in the current month ({currency})",
    ),
    (
        "Uso de completion no mês corrente ({currency})",
        "metric.label.completion_month",
        "Completion usage in the current month ({currency})",
    ),
    (
        "Uso pós-pago previsto no ciclo corrente",
        "metric.label.postpaid_forecast",
        "Postpaid usage forecast for the current cycle",
    ),
    (
        "BYOK no mês (cobrado pelo provedor)",
        "metric.label.byok_month",
        "BYOK in the month (billed by the provider)",
    ),
    (
        "Orçamento do provedor — {name}",
        "metric.label.provider_budget_named",
        "Provider budget — {name}",
    ),
    (
        "Orçamento do provedor ({period})",
        "metric.label.provider_budget_period",
        "Provider budget ({period})",
    ),
    (
        "Orçamento do provedor",
        "metric.label.provider_budget",
        "Provider budget",
    ),
    (
        "Orçamento — {scope}",
        "metric.label.budget_scope",
        "Budget — {scope}",
    ),
    (
        "Orçamento ({scope})",
        "metric.label.budget_paren",
        "Budget ({scope})",
    ),
    (
        "{prefix} — janela de {n} semana(s) (%)",
        "metric.label.window_weeks",
        "{prefix} — {n}-week window (%)",
    ),
    (
        "{prefix} — janela de {n} hora(s) (%)",
        "metric.label.window_hours",
        "{prefix} — {n}-hour window (%)",
    ),
    (
        "{prefix} — janela de {n}s (%)",
        "metric.label.window_seconds",
        "{prefix} — {n}s window (%)",
    ),
    (
        "{label} (rate limit)",
        "metric.label.rate_limit_of",
        "{label} (rate limit)",
    ),
];

/// The display text of a metric label, unit or period.
pub fn metric_text(raw: &str) -> Localized {
    for (template, id, english) in TABLE {
        if let Some(captures) = capture(template, raw) {
            let mut params = BTreeMap::new();
            for (name, value) in captures {
                let value = if name == "label" {
                    ParamValue::l10n(metric_text(value))
                } else {
                    ParamValue::Text(value.to_string())
                };
                params.insert(name.to_string(), value);
            }
            return Localized::with_params(*id, params, *english);
        }
    }
    Localized {
        id: Cow::Borrowed("metric.verbatim"),
        params: BTreeMap::new(),
        fallback: Cow::Owned(raw.to_string()),
    }
}

/// Match `raw` against `template`, whose `{name}` slots take any
/// non-empty text; the slots' values in order, or `None`.
fn capture<'t, 'r>(template: &'t str, raw: &'r str) -> Option<Vec<(&'t str, &'r str)>> {
    let mut parts = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let close = rest[open..].find('}')? + open;
        parts.push((&rest[..open], Some(&rest[open + 1..close])));
        rest = &rest[close + 1..];
    }
    parts.push((rest, None));
    let mut captures = Vec::new();
    let mut input = raw;
    let mut pending: Option<&str> = None;
    for (literal, slot) in parts {
        match pending.take() {
            None => input = input.strip_prefix(literal)?,
            Some(name) => {
                let at = if literal.is_empty() {
                    input.len()
                } else if slot.is_none() {
                    // The last literal anchors at the end.
                    input.strip_suffix(literal)?.len()
                } else {
                    input.find(literal)?
                };
                if at == 0 {
                    return None;
                }
                captures.push((name, &input[..at]));
                input = &input[at + literal.len()..];
            }
        }
        pending = slot;
    }
    if let Some(name) = pending {
        if input.is_empty() {
            return None;
        }
        captures.push((name, input));
        input = "";
    }
    input.is_empty().then_some(captures)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_matches_literals_and_slots() {
        assert_eq!(capture("abc", "abc"), Some(vec![]));
        assert_eq!(capture("abc", "abcd"), None);
        assert_eq!(
            capture("Budget ({x})", "Budget (USD)"),
            Some(vec![("x", "USD")])
        );
        assert_eq!(
            capture("{a}: {b}", "Copilot: chat"),
            Some(vec![("a", "Copilot"), ("b", "chat")])
        );
        assert_eq!(capture("{a} (x)", " (x)"), None);
        assert_eq!(capture("x {a}", "x "), None);
    }

    #[test]
    fn unknown_texts_come_back_verbatim() {
        let text = metric_text("gpt-5 requests");
        assert_eq!(text.resolve("pt-BR"), "gpt-5 requests");
        assert_eq!(text.resolve("es"), "gpt-5 requests");
    }

    /// Every adapter text is in every catalog with its slots, and pt-BR
    /// renders the adapter's own text (unchanged for Portuguese users).
    #[test]
    fn every_entry_is_translated_and_pt_br_is_the_adapter_text() {
        let mut ids = std::collections::BTreeSet::new();
        for (template, id, english) in TABLE {
            assert!(ids.insert(*id), "duplicate id {id}");
            assert!(id.starts_with("metric."), "{id}");
            let markers = crate::text::portuguese_markers(english);
            assert!(markers.is_empty(), "{id}: {markers:?} in {english:?}");
            for locale in ["pt-BR", "es", "it"] {
                let catalog = crate::text::core_catalog(locale).unwrap();
                let Some(text) = catalog.get(*id) else {
                    panic!("{locale}: {id} missing");
                };
                assert_eq!(slots(text), slots(template), "{locale}: slots of {id}");
                if locale == "pt-BR" {
                    assert_eq!(text, template, "pt-BR of {id}");
                }
            }
            assert_eq!(slots(english), slots(template), "English slots of {id}");
        }
    }

    fn slots(text: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let Some(close) = rest[open..].find('}') else {
                break;
            };
            out.push(&rest[open + 1..open + close]);
            rest = &rest[open + close + 1..];
        }
        out.sort_unstable();
        out
    }
}
