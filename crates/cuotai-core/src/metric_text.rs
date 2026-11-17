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
//! Portuguese rendering is unchanged, except for a slot named `count`: that
//! entry is a plural object in the catalogs (PRD 0035 RF4), so pt-BR reads
//! "1 hora" and "5 horas" where the adapter wrote "hora(s)". A text it does
//! not recognize (a provider-supplied name, a model id) comes back verbatim.

use crate::text::{Localized, ParamValue};
use std::borrow::Cow;
use std::collections::BTreeMap;

/// `(adapter text, id, English)`. `{name}` slots match any non-empty text
/// and become params; every slot but `count` is itself rendered through
/// [`metric_text`], and a slot named `count` selects the plural form.
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
        "últimos {days} dias",
        "metric.period.last_days",
        "last {days} days",
    ),
    ("cota gratuita", "metric.period.free_tier", "free tier"),
    ("modelos", "metric.unit.models", "models"),
    ("chamadas", "metric.unit.calls", "calls"),
    (
        "Modelos com cota gratuita",
        "metric.label.free_tier_models",
        "Models with free-tier quota",
    ),
    (
        "Chamadas bem-sucedidas",
        "metric.label.successful_calls",
        "Successful calls",
    ),
    ("Tokens totais", "metric.label.total_tokens", "Total tokens"),
    (
        "Tokens de entrada",
        "metric.label.input_tokens",
        "Input tokens",
    ),
    (
        "Tokens de saída",
        "metric.label.output_tokens",
        "Output tokens",
    ),
    (
        "30 dias concluídos (UTC)",
        "metric.period.last_30_days_utc",
        "30 completed days (UTC)",
    ),
    ("ciclo {cycle}", "metric.period.cycle", "cycle {cycle}"),
    // After the exact "janela de 5h"/"janela de 7d", which match first.
    (
        "janela de {span}",
        "metric.period.window_span",
        "{span} window",
    ),
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
        "Créditos de IA da organização (cota incluída)",
        "metric.label.jetbrains_ai.org_tariff",
        "Organization AI credits (built-in quota)",
    ),
    (
        "Créditos de IA da organização (top-up)",
        "metric.label.jetbrains_ai.org_topup",
        "Organization AI credits (top-up)",
    ),
    (
        "Créditos de IA — {tool}",
        "metric.label.jetbrains_ai.tool",
        "AI credits — {tool}",
    ),
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
    ("Gasto hoje", "metric.label.today_spend", "Spending today"),
    ("Gasto total", "metric.label.total_spend", "Total spending"),
    (
        "Orçamento da chave",
        "metric.label.key_budget",
        "Key budget",
    ),
    (
        "Orçamento da equipe",
        "metric.label.team_budget",
        "Team budget",
    ),
    (
        "Workspace padrão",
        "metric.label.default_workspace",
        "Default workspace",
    ),
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
        "Token Plan: diário",
        "metric.label.token_plan_daily",
        "Token Plan: daily",
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
    // PRD 0007 re-validation (V-05): Claude's weekly windows (OAuth usage
    // and the Claude Code feed), read by the pace line in every language.
    ("Semana (7 dias)", "metric.label.week_7d", "Week (7 days)"),
    (
        "Semana — Opus (7 dias)",
        "metric.label.week_opus_7d",
        "Week — Opus (7 days)",
    ),
    (
        "Semana — Sonnet (7 dias)",
        "metric.label.week_sonnet_7d",
        "Week — Sonnet (7 days)",
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
        "{prefix} — janela de {count} semana(s) (%)",
        "metric.label.window_weeks",
        "{prefix} — {count}-week window (%)",
    ),
    (
        "{prefix} — janela de {count} hora(s) (%)",
        "metric.label.window_hours",
        "{prefix} — {count}-hour window (%)",
    ),
    (
        "{prefix} — janela de {n}s (%)",
        "metric.label.window_seconds",
        "{prefix} — {n}s window (%)",
    ),
    // PRD 0037 re-validation (V-02): the labels, units, periods and slot
    // values the source scan of `label_snapshot` found verbatim.
    (
        "Vozes personalizadas",
        "metric.label.voices_custom",
        "Custom voices",
    ),
    (
        "Vozes profissionais",
        "metric.label.voices_professional",
        "Professional voices",
    ),
    (
        "Token Plan: semanal",
        "metric.label.token_plan_weekly",
        "Token Plan: weekly",
    ),
    (
        "Token Plan: mensal",
        "metric.label.token_plan_monthly",
        "Token Plan: monthly",
    ),
    (
        "Coding Plan: semanal",
        "metric.label.coding_plan_weekly",
        "Coding Plan: weekly",
    ),
    (
        "Coding Plan: mensal",
        "metric.label.coding_plan_monthly",
        "Coding Plan: monthly",
    ),
    (
        "Custo Alibaba Cloud (BSS, PreTaxAmount)",
        "metric.label.alibaba_cost_bss",
        "Alibaba Cloud cost (BSS, PreTaxAmount)",
    ),
    ("desconhecida", "metric.word.unknown", "unknown"),
    (
        "Saldo — conta pessoal",
        "metric.label.balance_personal",
        "Balance — personal account",
    ),
    (
        "Ollama Cloud: semanal",
        "metric.label.ollama_weekly",
        "Ollama Cloud: weekly",
    ),
    (
        "Ollama Cloud: mensal",
        "metric.label.ollama_monthly",
        "Ollama Cloud: monthly",
    ),
    (
        "Ollama: requisições",
        "metric.label.ollama_requests",
        "Ollama: requests",
    ),
    (
        "Ollama: requisições ({period})",
        "metric.label.ollama_requests_range",
        "Ollama: requests ({period})",
    ),
    (
        "Ollama: tokens de entrada",
        "metric.label.ollama_input_tokens",
        "Ollama: input tokens",
    ),
    (
        "Ollama: tokens de entrada ({period})",
        "metric.label.ollama_input_tokens_range",
        "Ollama: input tokens ({period})",
    ),
    (
        "Ollama: tokens de saída",
        "metric.label.ollama_output_tokens",
        "Ollama: output tokens",
    ),
    (
        "Ollama: tokens de saída ({period})",
        "metric.label.ollama_output_tokens_range",
        "Ollama: output tokens ({period})",
    ),
    (
        "Ollama: gasto",
        "metric.label.ollama_spend",
        "Ollama: spend",
    ),
    (
        "Ollama: gasto ({period})",
        "metric.label.ollama_spend_range",
        "Ollama: spend ({period})",
    ),
    ("Mensal", "metric.label.monthly", "Monthly"),
    ("Gasto", "metric.label.spend", "Spend"),
    (
        "Tokens de entrada em cache",
        "metric.label.input_tokens_cached",
        "Cached input tokens",
    ),
    (
        "Tokens de entrada sem cache",
        "metric.label.input_tokens_uncached",
        "Uncached input tokens",
    ),
    (
        "Tokens de entrada (lote)",
        "metric.label.input_tokens_batch",
        "Input tokens (batch)",
    ),
    (
        "Tokens de entrada em cache (lote)",
        "metric.label.input_tokens_cached_batch",
        "Cached input tokens (batch)",
    ),
    (
        "Tokens de entrada sem cache (lote)",
        "metric.label.input_tokens_uncached_batch",
        "Uncached input tokens (batch)",
    ),
    (
        "Tokens de saída (lote)",
        "metric.label.output_tokens_batch",
        "Output tokens (batch)",
    ),
    (
        "Imagens geradas",
        "metric.label.images_generated",
        "Images generated",
    ),
    (
        "Imagens processadas",
        "metric.label.images_processed",
        "Images processed",
    ),
    (
        "Buscas de grounding",
        "metric.label.grounding_searches",
        "Grounding searches",
    ),
    (
        "Transcrição de áudio",
        "metric.label.audio_transcription",
        "Audio transcription",
    ),
    (
        "Quadros de vídeo processados",
        "metric.label.video_frames",
        "Video frames processed",
    ),
    ("tokens", "metric.unit.tokens", "tokens"),
    ("imagens", "metric.unit.images", "images"),
    ("quadros", "metric.unit.frames", "frames"),
    (
        "mês corrente ({start} a {end}, {tz})",
        "metric.period.current_month_range",
        "current month ({start} to {end}, {tz})",
    ),
    ("chave de API", "metric.scope.api_key", "API key"),
    ("usuário", "metric.scope.user", "user"),
    ("usuário da equipe", "metric.scope.team_user", "team user"),
    (
        "equipe compartilhada",
        "metric.scope.team_pooled",
        "shared team",
    ),
    (
        "organização (aviso)",
        "metric.scope.org_soft",
        "organization (warning)",
    ),
    (
        "organização (limite rígido)",
        "metric.scope.org_hard",
        "organization (hard limit)",
    ),
    ("cota", "metric.word.quota", "quota"),
    ("limite", "metric.word.limit", "limit"),
    ("Assentos", "metric.label.seats", "Seats"),
    ("Tokens", "metric.label.tokens", "Tokens"),
    (
        "{label}: uso recente {quota} ({location})",
        "metric.label.recent_usage_of",
        "{label}: recent usage {quota} ({location})",
    ),
    (
        "{label}: {quota} / {limit_name} ({location})",
        "metric.label.quota_limit_of",
        "{label}: {quota} / {limit_name} ({location})",
    ),
    (
        "{label} (rate limit)",
        "metric.label.rate_limit_of",
        "{label} (rate limit)",
    ),
    // PRD 0023 re-validation (V-04, R-01): the paid extra usage of Claude
    // and Kiro, named in the reader's language.
    (
        "Uso extra — utilizado",
        "metric.label.extra_usage_used",
        "Extra usage — used",
    ),
    (
        "unid. provedor",
        "metric.unit.provider_units",
        "provider units",
    ),
    (
        "{label} (excedente cobrado)",
        "metric.label.overage_charged_of",
        "{label} (overage charged)",
    ),
    (
        "{label} (excedente)",
        "metric.label.overage_of",
        "{label} (overage)",
    ),
    (
        "{label} (teste gratuito)",
        "metric.label.free_trial_of",
        "{label} (free trial)",
    ),
    // PRD 0041 RF2.1: the default label of a budget metric.
    (
        "Orçamento · {label}",
        "metric.label.budget_of",
        "Budget · {label}",
    ),
    // A percent window of a named limit ("Sessão (5 horas) (%)"). Last:
    // the first match wins, so every specific "… (%)" template above it
    // keeps its own text.
    // A day count (Ollama's usage `range`); its English singular is in
    // [`ENGLISH_ONE`]. After "últimos {days} dias", which matches first.
    ("{count} dias", "metric.period.days", "{count} days"),
    ("{label} (%)", "metric.label.percent_of", "{label} (%)"),
];

/// PRD 0036 re-validation V-10: the quota labels 9router writes as free
/// English text, mapped to the localized ids of the same windows. `(label,
/// id, English)`, matched exactly as 9router writes them (lowercase) after
/// [`TABLE`] (whose adapter texts win); the English of an id [`TABLE`] also
/// holds is the same text. The raw label stays the metric's identity.
pub(crate) const AGGREGATOR_LABELS: &[(&str, &str, &str)] = &[
    (
        "session (5h)",
        "metric.label.session_5h",
        "Session (5 hours)",
    ),
    // Codex's 5-hour window is named `session` alone.
    ("session", "metric.label.session_5h", "Session (5 hours)"),
    ("weekly (7d)", "metric.label.week_7d", "Week (7 days)"),
    (
        "weekly Opus (7d)",
        "metric.label.week_opus_7d",
        "Week — Opus (7 days)",
    ),
    (
        "weekly Sonnet (7d)",
        "metric.label.week_sonnet_7d",
        "Week — Sonnet (7 days)",
    ),
    // Any other model's weekly window.
    (
        "weekly {label} (7d)",
        "metric.label.week_model_7d",
        "Week — {label} (7 days)",
    ),
    ("weekly", "metric.label.weekly", "Weekly"),
    ("monthly", "metric.label.monthly", "Monthly"),
];

/// PRD 0037 re-validation (PRD 0038 review F-01): the labels other adapters
/// write as English text, mapped to localized ids, `(label, id, English)`,
/// matched exactly after [`AGGREGATOR_LABELS`]. Every label a person reads
/// is localized; only brand names, model ids, symbols and codes stay
/// verbatim. Like [`AGGREGATOR_LABELS`], pt-BR gets its own wording. A slot
/// is rendered through [`metric_text`], so a quota key no entry names stays
/// as the provider sent it.
pub(crate) const ENGLISH_LABELS: &[(&str, &str, &str)] = &[
    // MiniMax's voucher balance.
    ("Voucher", "metric.label.voucher", "Voucher"),
    // Kimi's coding rate limit.
    ("Rate limit", "metric.label.rate_limit", "Rate limit"),
    // Cursor's spend limit read from the agent feed.
    ("Spend limit", "metric.label.spend_limit", "Spend limit"),
    // GitHub Copilot's quotas (9router's contract) and their known names.
    (
        "Copilot: {quota}",
        "metric.label.copilot_quota",
        "Copilot: {quota}",
    ),
    (
        "Premium requests",
        "metric.label.copilot_premium_requests",
        "Premium requests",
    ),
    ("Chat", "metric.label.copilot_chat", "Chat"),
    (
        "Completions",
        "metric.label.copilot_completions",
        "Completions",
    ),
    // CodeBuddy's bonus packs and refill cadences ("Daily 2" is the second
    // daily refill).
    (
        "CodeBuddy: Bonus Pack {index}",
        "metric.label.codebuddy_bonus_pack",
        "CodeBuddy: Bonus Pack {index}",
    ),
    (
        "CodeBuddy: {cadence} {index}",
        "metric.label.codebuddy_cadence_n",
        "CodeBuddy: {cadence} {index}",
    ),
    (
        "CodeBuddy: {cadence}",
        "metric.label.codebuddy_cadence",
        "CodeBuddy: {cadence}",
    ),
    ("Daily", "metric.label.daily", "Daily"),
    ("Weekly", "metric.label.weekly", "Weekly"),
    ("Monthly", "metric.label.monthly", "Monthly"),
];

/// The display text of a metric label, unit or period.
pub fn metric_text(raw: &str) -> Localized {
    for (template, id, english) in TABLE {
        if let Some(captures) = capture(template, raw) {
            return localized(id, english, captures);
        }
    }
    for (template, id, english) in AGGREGATOR_LABELS.iter().chain(ENGLISH_LABELS) {
        if let Some(captures) = capture(template, raw) {
            return localized(id, english, captures);
        }
    }
    Localized {
        id: Cow::Borrowed("metric.verbatim"),
        params: BTreeMap::new(),
        fallback: Cow::Owned(raw.to_string()),
    }
}

/// `(id, English singular)` of the [`TABLE`] entries whose `{count}` slot
/// changes the English wording (the entry's English is the plural).
const ENGLISH_ONE: &[(&str, &str)] = &[("metric.period.days", "{count} day")];

fn localized(id: &'static str, english: &'static str, captures: Vec<(&str, &str)>) -> Localized {
    let mut params = BTreeMap::new();
    for (name, value) in captures {
        // PRD 0037 re-validation (R-03): every slot but `count` is itself
        // rendered through [`metric_text`] (Kimchi's scope, Vertex's quota
        // fallbacks, Ollama's range); a verbatim slot stays plain text,
        // except `label`, which has always been a nested text.
        let value = match name {
            "count" => ParamValue::Text(value.to_string()),
            "label" => ParamValue::l10n(metric_text(value)),
            _ => {
                let text = metric_text(value);
                if text.id == "metric.verbatim" {
                    ParamValue::Text(value.to_string())
                } else {
                    ParamValue::l10n(text)
                }
            }
        };
        params.insert(name.to_string(), value);
    }
    match ENGLISH_ONE.iter().find(|(one_id, _)| *one_id == id) {
        Some((_, one)) => Localized::plural(id, params, one, english),
        None => Localized::with_params(id, params, english),
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
    /// PRD 0007 re-validation (V-05): Claude's 5-hour and weekly labels, as
    /// the OAuth adapter and the Claude Code feed write them, are named in
    /// the reader's language (they lead the pace line), never left in
    /// Portuguese.
    #[test]
    fn claude_window_labels_are_translated() {
        let cases = [
            (
                "Sessão (5 horas) (%)",
                "Session (5 hours) (%)",
                "Sesión (5 horas) (%)",
            ),
            (
                "Semana (7 dias) (%)",
                "Week (7 days) (%)",
                "Semana (7 días) (%)",
            ),
            (
                "Semana — Opus (7 dias) (%)",
                "Week — Opus (7 days) (%)",
                "Semana — Opus (7 días) (%)",
            ),
            (
                "Semana — Sonnet (7 dias) (%)",
                "Week — Sonnet (7 days) (%)",
                "Semana — Sonnet (7 días) (%)",
            ),
            ("Semana (7 dias)", "Week (7 days)", "Semana (7 días)"),
        ];
        for (raw, english, spanish) in cases {
            let text = metric_text(raw);
            assert_ne!(text.id, "metric.verbatim", "{raw}");
            assert_eq!(text.resolve("en-US"), english, "{raw}");
            assert_eq!(text.resolve("es"), spanish, "{raw}");
            assert_eq!(text.resolve("pt-BR"), raw, "{raw}");
            assert!(!text.resolve("ja").contains("Semana"), "{raw}");
        }
        // A specific "… (%)" template still wins over the generic one.
        assert_eq!(
            metric_text("Claude — janela de 5 hora(s) (%)").id,
            "metric.label.window_hours"
        );
    }

    /// PRD 0036 re-validation V-10: 9router's window labels are named in
    /// the reader's language through the ids of the same windows; a label
    /// it does not know stays verbatim.
    #[test]
    fn nine_router_window_labels_are_translated() {
        let cases = [
            (
                "session (5h)",
                "metric.label.session_5h",
                "Session (5 hours)",
            ),
            ("session", "metric.label.session_5h", "Session (5 hours)"),
            ("weekly (7d)", "metric.label.week_7d", "Week (7 days)"),
            (
                "weekly Opus (7d)",
                "metric.label.week_opus_7d",
                "Week — Opus (7 days)",
            ),
            (
                "weekly Sonnet (7d)",
                "metric.label.week_sonnet_7d",
                "Week — Sonnet (7 days)",
            ),
            (
                "weekly Gemini (7d)",
                "metric.label.week_model_7d",
                "Week — Gemini (7 days)",
            ),
            ("weekly", "metric.label.weekly", "Weekly"),
            ("monthly", "metric.label.monthly", "Monthly"),
        ];
        for (raw, id, english) in cases {
            let text = metric_text(raw);
            assert_eq!(text.id, id, "{raw}");
            assert_eq!(text.resolve("en-US"), english, "{raw}");
            for locale in ["pt-BR", "de", "ja", "ru"] {
                let shown = text.resolve(locale);
                assert_ne!(shown, raw, "{locale}: {raw}");
                assert!(!shown.contains('{'), "{locale}: {shown}");
            }
        }
        assert_eq!(
            metric_text("weekly Gemini (7d)").resolve("de"),
            "Woche — Gemini (7 Tage)"
        );
        assert_eq!(metric_text("requests (daily)").id, "metric.verbatim");
        // Exact text only: another casing is not 9router's label.
        assert_eq!(metric_text("Session (5h)").id, "metric.verbatim");
        // An id TABLE also holds keeps its English text.
        for (_, id, english) in AGGREGATOR_LABELS {
            if let Some((_, _, table)) = TABLE.iter().find(|(_, tid, _)| tid == id) {
                assert_eq!(english, table, "{id}");
            }
            for locale in crate::text::catalog_locales() {
                let catalog = crate::text::core_catalog(locale).unwrap();
                assert!(catalog.get(*id).is_some(), "{locale}: {id} missing");
            }
        }
    }

    /// PRD 0034 re-validation V-12: counted texts are real plurals, so
    /// Russian picks one, few or many by the count instead of a faked
    /// "(s)" form or a "label: number" workaround.
    #[test]
    fn counted_texts_take_the_plural_form_of_their_count() {
        let window = |hours: u32| metric_text(&format!("Codex — janela de {hours} hora(s) (%)"));
        assert_eq!(window(1).resolve("en-US"), "Codex — 1-hour window (%)");
        assert_eq!(window(1).resolve("pt-BR"), "Codex — janela de 1 hora (%)");
        assert_eq!(window(5).resolve("pt-BR"), "Codex — janela de 5 horas (%)");
        assert_eq!(window(2).resolve("ru"), "Codex — окно в 2 часа (%)");
        assert_eq!(window(5).resolve("ru"), "Codex — окно в 5 часов (%)");
        assert_eq!(window(21).resolve("ru"), "Codex — окно в 21 час (%)");
        let weeks = metric_text("Codex — janela de 2 semana(s) (%)");
        assert_eq!(weeks.resolve("de"), "Codex — Fenster von 2 Wochen (%)");

        let feed = |unreadable: usize| {
            crate::feed::FeedHealth {
                readable: 0,
                unreadable,
            }
            .note()
            .unwrap()
        };
        assert_eq!(feed(1).resolve("en-US"), "1 unreadable feed file ignored.");
        assert_eq!(feed(3).resolve("en-US"), "3 unreadable feed files ignored.");
        assert_eq!(
            feed(2).resolve("ru"),
            "Пропущено 2 нечитаемых файла потока."
        );
        assert_eq!(
            feed(5).resolve("ru"),
            "Пропущено 5 нечитаемых файлов потока."
        );
        assert_eq!(
            feed(21).resolve("ru"),
            "Пропущен 21 нечитаемый файл потока."
        );
    }

    /// PRD 0023 re-validation (V-04, R-01): Claude's and Kiro's extra
    /// usage labels and unit are translated, never left in Portuguese.
    #[test]
    fn extra_usage_labels_are_translated() {
        let cases = [
            ("Uso extra — utilizado", "Extra usage — used"),
            ("unid. provedor", "provider units"),
            ("Kiro: credit (excedente)", "Kiro: credit (overage)"),
            (
                "Kiro: credit (excedente cobrado)",
                "Kiro: credit (overage charged)",
            ),
            ("Kiro: credit (teste gratuito)", "Kiro: credit (free trial)"),
        ];
        for (raw, english) in cases {
            let text = metric_text(raw);
            assert_ne!(text.id, "metric.verbatim", "{raw}");
            assert_eq!(text.resolve("en-US"), english, "{raw}");
            assert_eq!(text.resolve("pt-BR"), raw, "{raw}");
            for locale in crate::text::catalog_locales() {
                let shown = text.resolve(locale);
                if locale != "pt-BR" && locale != "es" {
                    assert!(!shown.contains("excedente"), "{locale}: {shown}");
                    assert!(!shown.contains("provedor"), "{locale}: {shown}");
                }
            }
        }
    }

    /// PRD 0037 re-validation (V-02, R-03): the texts the source scan found
    /// verbatim are translated, and every slot but `count` is rendered too
    /// (Kimchi's scope, Vertex's quota fallbacks, Ollama's day range); a
    /// slot `metric_text` does not know stays plain text.
    #[test]
    fn slot_values_and_scanned_labels_are_translated() {
        let cases = [
            ("Vozes personalizadas", "de", "Eigene Stimmen"),
            ("Vozes profissionais", "en", "Professional voices"),
            ("Orçamento — usuário", "de", "Budget — Benutzer"),
            ("Orçamento — chave de API", "en", "Budget — API key"),
            (
                "Vertex AI: cota / limite (global)",
                "en",
                "Vertex AI: quota / limit (global)",
            ),
            (
                "Vertex AI: uso recente cota (global)",
                "de",
                "Vertex AI: letzte Nutzung Kontingent (global)",
            ),
            ("Ollama: gasto (7 dias)", "en", "Ollama: spend (7 days)"),
            ("Ollama: gasto (1 dias)", "en", "Ollama: spend (1 day)"),
            ("Ollama: gasto (1 dias)", "pt-BR", "Ollama: gasto (1 dia)"),
            (
                "Ollama: requisições (5 dias)",
                "ru",
                "Ollama: запросы (5 дней)",
            ),
            (
                "Ollama: requisições (2 dias)",
                "ru",
                "Ollama: запросы (2 дня)",
            ),
            ("Mensal", "de", "Monatlich"),
            (
                "Tokens de entrada em cache (lote)",
                "en",
                "Cached input tokens (batch)",
            ),
            ("Assentos", "en", "Seats"),
            ("desconhecida", "en", "unknown"),
        ];
        for (raw, locale, expected) in cases {
            assert_eq!(
                metric_text(raw).resolve(locale),
                expected,
                "{raw} in {locale}"
            );
        }
        let named = metric_text("Orçamento do provedor — anthropic");
        assert!(matches!(named.params["name"], ParamValue::Text(_)));
        let scoped = metric_text("Orçamento — usuário");
        assert!(matches!(scoped.params["scope"], ParamValue::L10n(_)));
    }

    #[test]
    fn every_entry_is_translated_and_pt_br_is_the_adapter_text() {
        let mut ids = std::collections::BTreeSet::new();
        for (template, id, english) in TABLE {
            assert!(ids.insert(*id), "duplicate id {id}");
            assert!(id.starts_with("metric."), "{id}");
            let markers = crate::text::portuguese_markers(english);
            assert!(markers.is_empty(), "{id}: {markers:?} in {english:?}");
            let counted = slots(template).contains(&"count");
            for locale in crate::text::catalog_locales() {
                let catalog = crate::text::core_catalog(locale).unwrap();
                let Some(message) = catalog.get(*id) else {
                    panic!("{locale}: {id} missing");
                };
                // A `{count}` slot makes the entry a plural object; any
                // other entry is a plain text.
                assert_eq!(
                    matches!(message, crate::text::Message::Plural(_)),
                    counted,
                    "{locale}: {id} plural shape"
                );
                for text in message.templates() {
                    assert_eq!(slots(text), slots(template), "{locale}: slots of {id}");
                    if locale == "pt-BR" && !counted {
                        assert_eq!(text, *template, "pt-BR of {id}");
                    }
                    assert!(!text.contains("(s)"), "{locale}: {id} fakes a plural");
                }
            }
            assert_eq!(slots(english), slots(template), "English slots of {id}");
        }
    }

    /// PRD 0038 review F-01: the English labels adapters write are
    /// localized in every catalog, their slots too, and an id shared with
    /// another table keeps its English text.
    #[test]
    fn english_adapter_labels_are_translated() {
        for (raw, id, english) in ENGLISH_LABELS {
            assert!(id.starts_with("metric.label."), "{id}");
            for (_, other_id, other) in TABLE.iter().chain(AGGREGATOR_LABELS) {
                if other_id == id {
                    assert_eq!(english, other, "{id}");
                }
            }
            for locale in crate::text::catalog_locales() {
                let catalog = crate::text::core_catalog(locale).unwrap();
                let Some(message) = catalog.get(*id) else {
                    panic!("{locale}: {id} missing");
                };
                for text in message.templates() {
                    assert_eq!(slots(text), slots(raw), "{locale}: slots of {id}");
                }
            }
            assert_eq!(slots(english), slots(raw), "English slots of {id}");
            if slots(raw).is_empty() {
                assert_eq!(metric_text(raw).id, *id, "{raw}");
                assert_eq!(metric_text(raw).resolve("en-US"), *english, "{raw}");
            }
        }
        let cases = [
            ("Voucher", "de", "Gutschein"),
            (
                "Copilot: Premium requests",
                "de",
                "Copilot: Premium-Anfragen",
            ),
            ("Copilot: Code review", "de", "Copilot: Code review"),
            ("CodeBuddy: Daily", "pt-BR", "CodeBuddy: Diário"),
            ("CodeBuddy: Weekly 2", "de", "CodeBuddy: Wöchentlich 2"),
            ("CodeBuddy: Bonus Pack 1", "ja", "CodeBuddy: Bonus Pack 1"),
            ("Spend limit (%)", "de", "Ausgabenlimit (%)"),
        ];
        for (raw, locale, expected) in cases {
            assert_eq!(
                metric_text(raw).resolve(locale),
                expected,
                "{raw} in {locale}"
            );
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
