//! PRD 0005 RF1 — how each account's usage is obtained, without network or
//! credentials: reliability level, credential class, plan requirement,
//! scope, per-query cost and evidence. Front ends render the descriptor and
//! never branch on [`Provider`]. Texts are `access.*` ids in the core
//! catalogs (English fallback). The descriptor serializes with
//! snake_case enums and `{ kind, url }` evidence, the shape `export::v1`
//! (PRD 0012) embeds as `access` per account (RF4.3).

use crate::text::Localized;
use crate::{Account, Provider, Report, Status};
use serde::Serialize;
use std::collections::BTreeMap;

/// PRD 0028 RF6.1: the date each interface (provider and mode, named by the
/// slug of its summary) had its evidence last checked against the public
/// source. Raise one entry only after checking that interface; the local
/// canary's evidence patch (RF4.4) is the normal way. A slug missing here
/// reads as the epoch, so it shows up as stale instead of as verified.
const VERIFIED: &[(&str, &str)] = &[
    ("alibaba_cloud", "2026-10-20"),
    ("alibaba_coding_plan", "2026-10-20"),
    ("alibaba_model_studio", "2026-10-20"),
    ("alibaba_token_plan", "2026-10-20"),
    ("anthropic_api", "2026-10-20"),
    ("antigravity", "2026-10-20"),
    ("aws_bedrock", "2026-10-20"),
    ("azure_open_ai", "2026-10-20"),
    ("chat_gpt", "2026-10-20"),
    ("claude", "2026-10-20"),
    ("cline", "2026-10-20"),
    ("cloudflare_ai", "2026-10-20"),
    ("code_buddy", "2026-10-20"),
    ("copilot_billing", "2026-10-20"),
    ("copilot_quota", "2026-10-20"),
    ("cursor_personal", "2026-10-20"),
    ("cursor_teams", "2026-10-20"),
    ("extension", "2026-10-20"),
    ("gemini_ai", "2026-10-20"),
    ("gemini_ai_cli", "2026-10-20"),
    ("gemini_api", "2026-10-20"),
    ("grok", "2026-10-20"),
    ("grok_cli", "2026-10-20"),
    ("groq", "2026-10-20"),
    ("kimchi", "2026-10-20"),
    ("kimi", "2026-10-20"),
    ("kimi_coding", "2026-10-20"),
    ("kiro", "2026-10-20"),
    ("litellm", "2026-10-25"),
    ("mini_max", "2026-10-20"),
    ("mistral", "2026-10-20"),
    ("nine_router", "2026-10-25"),
    ("nvidia_nim", "2026-10-20"),
    ("ollama_cloud", "2026-10-20"),
    ("open_ai_api", "2026-10-20"),
    ("open_code_go", "2026-10-20"),
    ("open_router", "2026-10-23"),
    ("open_router_account", "2026-10-23"),
    ("perplexity", "2026-10-20"),
    ("perplexity_computer", "2026-10-20"),
    ("poolside", "2026-10-20"),
    ("qoder", "2026-10-20"),
    ("qwen", "2026-10-20"),
    ("qwen_oauth", "2026-10-20"),
    ("vertex_ai", "2026-10-20"),
    ("windsurf", "2026-10-20"),
    ("xiaomi_mimo", "2026-10-20"),
    ("zed", "2026-10-20"),
];

fn verified_on(slug: &str) -> &'static str {
    VERIFIED
        .iter()
        .find(|(known, _)| *known == slug)
        .map_or("1970-01-01", |(_, date)| date)
}

macro_rules! router {
    ($file:literal) => {
        Evidence::Reference(concat!(
            "https://github.com/decolua/9router/blob/f01fb909e37189008080632ddaf404f096345cde/open-sse/services/usage/",
            $file
        ))
    };
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// An interface the vendor documents for that credential.
    Official,
    /// An internal or inferred interface; may change without notice.
    Undocumented,
    /// Usage inferred from another source (metrics, rate-limit headers).
    Derived,
    /// No query exists; [`UsageAccess::reason`] says why.
    Unavailable,
    /// PRD 0017: a local program the user installed reports the usage.
    Extension,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Credential {
    InferenceKey,
    AdminKey,
    SubscriptionSession,
    CloudIdentity,
    LocalCli,
    UserSession,
    /// PRD 0036: the dashboard password of a self-hosted aggregator (or
    /// none, when the instance has login disabled).
    AggregatorLogin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Account,
    Organization,
    /// A bill for every product of the cloud account (Alibaba Cloud).
    WholeCloud,
    Product,
    /// PRD 0036: every upstream connection held by one aggregator instance.
    Instance,
}

/// Public source of the classification: never an authenticated URL.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "url", rename_all = "snake_case")]
pub enum Evidence {
    /// Vendor documentation page.
    Documentation(&'static str),
    /// 9router or community-client file that defines the contract.
    Reference(&'static str),
}

impl Evidence {
    pub fn url(self) -> &'static str {
        match self {
            Self::Documentation(url) | Self::Reference(url) => url,
        }
    }
}

/// PRD 0028 RF6.2: evidence older than this reads as "Not verified since".
pub const STALE_AFTER_DAYS: i64 = 90;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UsageAccess {
    pub level: Level,
    pub credential: Credential,
    pub plan_requirement: Option<Localized>,
    pub scope: Scope,
    /// [`Provider::is_metered`]: every query is billed.
    pub metered: bool,
    pub evidence: Evidence,
    /// ISO date of the last check of `evidence`.
    pub verified_on: &'static str,
    pub summary: Localized,
    pub credential_hint: Localized,
    pub risk: Localized,
    /// PRD 0028 RF6.3: `Some` for a subscription-session credential: the
    /// token belongs to the vendor's own client and using it elsewhere may go
    /// against the vendor's terms.
    #[serde(skip)]
    pub token_risk: Option<Localized>,
    /// Why there is no query; `Some` exactly when `level` is `Unavailable`.
    pub reason: Option<Localized>,
    /// PRD 0023 RF2.7: whether this interface reports paid extra usage.
    pub overage: OverageReporting,
    /// PRD 0028 RF7.3: the built-in provider an extension override replaced
    /// for this reading (`level` is then [`Level::Extension`]); absent
    /// otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overrides: Option<Provider>,
}

/// How an interface reports paid extra usage (overage).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverageReporting {
    /// The amount comes in a stated currency.
    Money,
    /// The amount comes in the provider's own units.
    ProviderUnits,
    /// The interface exposes no overage data.
    NotReported,
}

impl OverageReporting {
    /// `access.overage.*`: short label for the docs matrix and details.
    pub fn label(self) -> Localized {
        let (slug, pt) = match self {
            Self::Money => ("money", "Reported in money"),
            Self::ProviderUnits => ("provider_units", "Reported in provider units"),
            Self::NotReported => ("not_reported", "Not reported"),
        };
        Localized::fixed(format!("access.overage.{slug}"), pt)
    }
}

impl Level {
    fn slug(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Undocumented => "undocumented",
            Self::Derived => "derived",
            Self::Unavailable => "unavailable",
            Self::Extension => "extension",
        }
    }

    /// `access.level.*`: short label for pickers and cards.
    pub fn label(self) -> Localized {
        let pt = match self {
            Self::Official => "Official",
            Self::Undocumented => "Undocumented",
            Self::Derived => "Derived",
            Self::Unavailable => "Unavailable",
            Self::Extension => "Extension",
        };
        Localized::fixed(format!("access.level.{}", self.slug()), pt)
    }

    /// `access.risk.*`: what the level means for the reading's trust.
    pub(crate) fn risk(self) -> Localized {
        let pt = match self {
            Self::Official => "Interface documented by the vendor for this credential; it does not guarantee permission on your account.",
            Self::Undocumented => "Internal interface the vendor does not document; it may change without notice.",
            Self::Derived => "Usage is inferred from another source (metrics or rate-limit headers); it is not a balance and may lag.",
            Self::Unavailable => "There is no usage query for this credential; the account shows the reason instead of numbers.",
            Self::Extension => "Runs a local program; review the code before enabling.",
        };
        Localized::fixed(format!("access.risk.{}", self.slug()), pt)
    }
}

impl Credential {
    fn slug(self) -> &'static str {
        match self {
            Self::InferenceKey => "inference_key",
            Self::AdminKey => "admin_key",
            Self::SubscriptionSession => "subscription_session",
            Self::CloudIdentity => "cloud_identity",
            Self::LocalCli => "local_cli",
            Self::UserSession => "user_session",
            Self::AggregatorLogin => "aggregator_login",
        }
    }

    /// `access.credential.*`: credential class label.
    pub fn label(self) -> Localized {
        let pt = match self {
            Self::InferenceKey => "Inference key",
            Self::AdminKey => "Admin key",
            Self::SubscriptionSession => "Subscription session",
            Self::CloudIdentity => "Cloud identity",
            Self::LocalCli => "Local CLI",
            Self::UserSession => "User session",
            Self::AggregatorLogin => "Aggregator login",
        };
        Localized::fixed(format!("access.credential.{}", self.slug()), pt)
    }

    fn hint(self) -> Localized {
        let pt = match self {
            Self::InferenceKey => "Uses the account's API key; no response is generated.",
            Self::AdminKey => "Needs an admin or billing key, not the inference key.",
            Self::SubscriptionSession => "Uses the subscription session (native login or its token), not a platform API key.",
            Self::CloudIdentity => "Uses a cloud identity (OAuth token, CLI profile or AccessKey) allowed to read billing or monitoring.",
            Self::LocalCli => "Uses an already signed-in profile of the vendor's CLI installed on this machine.",
            Self::UserSession => "Needs a web session you paste in the account wizard (kept in the system vault) or export in a variable; it expires and is never imported automatically.",
            Self::AggregatorLogin => "Uses the dashboard password of your own instance (or none, when login is off on it); the providers' credentials never leave the instance.",
        };
        Localized::fixed(format!("access.credential_hint.{}", self.slug()), pt)
    }
}

impl Scope {
    /// `access.scope.*`: what the number covers.
    pub fn label(self) -> Localized {
        let (slug, pt) = match self {
            Self::Account => ("account", "Account"),
            Self::Organization => ("organization", "Organization"),
            Self::WholeCloud => ("whole_cloud", "Whole cloud"),
            Self::Product => ("product", "Product"),
            Self::Instance => ("instance", "Instance"),
        };
        Localized::fixed(format!("access.scope.{slug}"), pt)
    }
}

/// `access.risk.subscription_token`.
fn subscription_token_risk() -> Localized {
    Localized::fixed(
        "access.risk.subscription_token",
        "The token belongs to the vendor's own client; using it from another tool may go against the vendor's terms.",
    )
}

fn entry(
    level: Level,
    credential: Credential,
    scope: Scope,
    evidence: Evidence,
    slug: &str,
    summary: &str,
) -> UsageAccess {
    UsageAccess {
        level,
        credential,
        plan_requirement: None,
        scope,
        metered: false,
        evidence,
        verified_on: verified_on(slug),
        token_risk: (credential == Credential::SubscriptionSession).then(subscription_token_risk),
        summary: Localized::fixed(format!("access.summary.{slug}"), summary),
        credential_hint: credential.hint(),
        risk: level.risk(),
        reason: None,
        overage: OverageReporting::NotReported,
        overrides: None,
    }
}

fn unavailable(
    credential: Credential,
    evidence: Evidence,
    slug: &str,
    summary: &str,
    reason: &str,
) -> UsageAccess {
    let mut access = entry(
        Level::Unavailable,
        credential,
        Scope::Account,
        evidence,
        slug,
        summary,
    );
    access.reason = Some(Localized::fixed(format!("access.reason.{slug}"), reason));
    access
}

impl UsageAccess {
    /// The level's label, or "Extension (override)" when an extension
    /// override answered (PRD 0028 RF7.3).
    pub fn label(&self) -> Localized {
        if self.overrides.is_some() {
            Localized::fixed("access.level.extension_override", "Extension (override)")
        } else {
            self.level.label()
        }
    }

    fn requires(mut self, plan: &str, label: &str) -> Self {
        self.plan_requirement = Some(Localized::fixed(format!("access.plan.{plan}"), label));
        self
    }

    /// `access.cost_advisory`: the per-query cost warning (PRD 0005 RF3.3),
    /// `Some` exactly when `metered`.
    pub fn cost_advisory(&self) -> Option<Localized> {
        self.metered.then(|| {
            Localized::fixed(
                "access.cost_advisory",
                "Provider is billed per query: about one Cost Explorer request per query. Without an interval Cuotai queries once an hour; the effective minimum is 15 minutes (intervals of 5 to 14 minutes are raised to 15, lower values are rejected). Several windows or commands share one request per interval. Manual refreshes remain explicit.",
            )
        })
    }

    /// What the account editors show where the credential help appears
    /// (PRD 0005 RF3.2), in order: credential hint, plan requirement and,
    /// for `Unavailable`, the reason. The interface `risk` stays out: how an
    /// integration works is documented in `docs/providers.md`, not in the UI.
    pub fn notes(&self) -> Vec<Localized> {
        let mut notes = vec![self.credential_hint.clone()];
        notes.extend(self.plan_requirement.clone().map(|plan| {
            Localized::with_param(
                "access.requires_plan",
                "plan",
                plan,
                "Requires the {plan} plan.",
            )
        }));
        notes.extend(self.reason.clone());
        notes
    }

    /// PRD 0028 RF6.2 (DS-CMP-VER-1): how old the evidence is on `today`.
    /// "Verified N months ago", and past [`STALE_AFTER_DAYS`] "Not verified
    /// since {date}". A malformed date reads as never verified.
    pub fn verification(&self, today: chrono::NaiveDate) -> Localized {
        let Ok(date) = chrono::NaiveDate::parse_from_str(self.verified_on, "%Y-%m-%d") else {
            return Localized::with_param(
                "access.verification_stale",
                "date",
                self.verified_on,
                "Not verified since {date}.",
            );
        };
        let days = (today - date).num_days().max(0);
        if days > STALE_AFTER_DAYS {
            return Localized::with_param(
                "access.verification_stale",
                "date",
                self.verified_on,
                "Not verified since {date}.",
            );
        }
        let months = days / 30;
        match months {
            0 => Localized::fixed("access.verified_recent", "Verified this month."),
            1 => Localized::fixed("access.verified_month", "Verified 1 month ago."),
            months => Localized::with_param(
                "access.verified_months",
                "months",
                months as u64,
                "Verified {months} months ago.",
            ),
        }
    }

    /// PRD 0005 RF3.4: the warning an editor shows before saving an
    /// `Unavailable` account, confirmed with "Save anyway"; `None` for every
    /// other level.
    pub fn save_confirmation(&self) -> Option<Localized> {
        self.reason.clone().map(|reason| {
            Localized::with_param(
                "access.confirm_unavailable",
                "reason",
                reason,
                "{reason} The account will be saved, but it will show this reason instead of numbers.",
            )
        })
    }

    /// PRD 0005 RF4.2: what the card and the account details show in place
    /// of `report`'s generic failure. An `Unavailable` account gets its
    /// reason whatever the failure; an `Undocumented` one whose response no
    /// longer matched the known schema gets "the interface may have changed"
    /// with the public reference it follows. `None` for a successful report
    /// and for every other failure, whose own note already explains it.
    pub fn explain(&self, report: &Report) -> Option<Localized> {
        if report.status == Status::Ready {
            return None;
        }
        match self.level {
            Level::Unavailable => self.reason.clone(),
            Level::Undocumented
                if report.status == Status::Error
                    && report
                        .notes_l10n
                        .iter()
                        .any(|note| note.id == crate::providers::SCHEMA_NOTE) =>
            {
                let evidence = self.evidence.url();
                Some(Localized::with_param(
                    "access.interface_changed",
                    "evidence",
                    evidence,
                    "The response does not follow the known internal interface: it may have changed without notice. Reference contract: {evidence}",
                ))
            }
            Level::Official | Level::Derived | Level::Undocumented | Level::Extension => None,
        }
    }
}

/// How `account`'s usage is obtained. Pure: reads only the provider, the
/// authentication type and the `params` that switch the queried interface;
/// a session slot held in the vault (PRD 0016) counts as a set parameter.
/// PRD 0028 RF7.3: [`describe`] of `account` as `report` was read: when an
/// extension override answered (the report's `source` is `extension:<id>`
/// on a built-in account), the extension's descriptor with `overrides` set
/// to the replaced provider; otherwise [`describe`].
pub fn describe_reading(account: &Account, report: Option<&crate::Report>) -> UsageAccess {
    let overridden = account.provider != Provider::Extension
        && report.is_some_and(|report| report.source.starts_with("extension:"));
    if !overridden {
        return describe(account);
    }
    let mut access = classify(Provider::Extension, false, &BTreeMap::new());
    access.metered = false;
    access.overrides = Some(account.provider);
    access
}

pub fn describe(account: &Account) -> UsageAccess {
    let slots = account.session.iter().flat_map(|s| s.slots.iter());
    classify(
        account.provider,
        account.oauth.is_some(),
        &with_vault_slots(&account.params, slots),
    )
}

/// `params` plus a placeholder for every slot stored in the vault, standing
/// for the session the query reads from there.
pub(crate) fn with_vault_slots<'a>(
    params: &BTreeMap<String, String>,
    slots: impl Iterator<Item = &'a String>,
) -> BTreeMap<String, String> {
    let mut params = params.clone();
    for slot in slots {
        params.insert(slot.clone(), "vault".into());
    }
    params
}

/// [`describe`] from its inputs, so an account still being edited (see
/// `account_edit::draft_access`) gets the same answer.
pub(crate) fn classify(
    provider: Provider,
    oauth: bool,
    params: &BTreeMap<String, String>,
) -> UsageAccess {
    use Credential::*;
    use Level::*;
    let param = |key: &str, value: &str| params.get(key).is_some_and(|v| v == value);
    let set = |key: &str| params.get(key).is_some_and(|v| !v.trim().is_empty());
    let doc = Evidence::Documentation;
    let enterprise = |access: UsageAccess| access.requires("enterprise", "Enterprise");
    // Exhaustive on purpose (PRD 0005 RF1.3): a new provider does not compile
    // without declaring how its usage is read.
    let mut access = match provider {
        Provider::OpenAiApi => entry(Official, AdminKey, Scope::Organization,
            doc("https://developers.openai.com/api/reference/resources/admin/subresources/organization"),
            "open_ai_api", "Organization costs and usage through the OpenAI Admin API."),
        Provider::AnthropicApi => entry(Official, AdminKey, Scope::Organization,
            doc("https://platform.claude.com/docs/en/manage-claude/usage-cost-api"),
            "anthropic_api", "Organization cost and usage through Anthropic's Usage and Cost Admin API."),
        Provider::ChatGpt => entry(Undocumented, SubscriptionSession, Scope::Account,
            Evidence::Reference("https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs"),
            "chat_gpt", "Codex limit windows through the Codex CLI's internal interface."),
        Provider::Claude => entry(Undocumented, SubscriptionSession, Scope::Account, router!("claude.js"),
            "claude", "Claude plan usage windows through the internal interface the official clients use."),
        Provider::GeminiApi => entry(Derived, CloudIdentity, Scope::Product,
            doc("https://cloud.google.com/monitoring/api/metrics_gcp_p_z"),
            "gemini_api", "Gemini API limits and recent consumption read from Cloud Monitoring metrics."),
        Provider::VertexAi => entry(Derived, CloudIdentity, Scope::Product,
            doc("https://cloud.google.com/monitoring/api/metrics_gcp_p_z"),
            "vertex_ai", "Vertex AI limits and recent consumption read from Cloud Monitoring metrics."),
        Provider::AwsBedrock => entry(Official, CloudIdentity, Scope::Product,
            doc("https://docs.aws.amazon.com/cli/latest/reference/ce/get-cost-and-usage.html"),
            "aws_bedrock", "Amazon Bedrock month cost through Cost Explorer; every query is billed."),
        Provider::AzureOpenAi => entry(Official, CloudIdentity, Scope::Product,
            doc("https://learn.microsoft.com/en-us/rest/api/cost-management/query/usage?view=rest-cost-management-2025-03-01"),
            "azure_open_ai", "Month cost of the chosen resource through Azure Cost Management."),
        Provider::Cursor if oauth || param("query_mode", "personal") => entry(Undocumented, SubscriptionSession, Scope::Account,
            Evidence::Reference("https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift"),
            "cursor_personal", "Personal Cursor subscription summary through the site's internal interface."),
        Provider::Cursor => entry(Official, AdminKey, Scope::Organization,
            doc("https://cursor.com/docs/account/teams/admin-api"),
            "cursor_teams", "Team spend in the cycle through Cursor's Admin API.")
            .requires("teams_enterprise", "Teams or Enterprise"),
        Provider::Copilot if oauth || param("query_mode", "quota") => entry(Undocumented, SubscriptionSession, Scope::Account,
            router!("github.js"), "copilot_quota", "Personal Copilot interaction quota through GitHub's internal interface."),
        Provider::Copilot => entry(Official, AdminKey,
            if param("mode", "org") { Scope::Organization } else { Scope::Account },
            doc("https://docs.github.com/en/rest/billing/usage"),
            "copilot_billing", "Billed Copilot usage and cost through GitHub's billing API."),
        Provider::Windsurf => enterprise(entry(Official, AdminKey, Scope::Organization,
            doc("https://docs.windsurf.com/windsurf/accounts/api-reference/get-team-credit-balance"),
            "windsurf", "Team credits in the cycle through Windsurf's accounts API.")),
        Provider::MiniMax => entry(Undocumented, InferenceKey, Scope::Account, router!("minimax.js"),
            "mini_max", "MiniMax Token/Coding Plan windows through the endpoints 9router uses."),
        Provider::Kimi if oauth || param("product", "coding") => entry(Undocumented,
            if oauth { SubscriptionSession } else { InferenceKey }, Scope::Account, router!("kimi.js"),
            "kimi_coding", "Kimi Coding weekly quota and rate limit through the subscription's internal interface."),
        Provider::Kimi => entry(Official, InferenceKey, Scope::Account,
            doc("https://platform.kimi.ai/docs/api/balance.md"),
            "kimi", "Kimi international platform balance through the documented balance endpoint."),
        Provider::Qwen if oauth => unavailable(SubscriptionSession,
            doc("https://qwenlm.github.io/qwen-code-docs/en/users/configuration/auth/"),
            "qwen_oauth",
            "Qwen Code OAuth session.",
            "No quota endpoint is confirmed for the Qwen Code session; the free OAuth plan was discontinued."),
        Provider::Qwen => unavailable(InferenceKey,
            doc("https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope"), "qwen",
            "Qwen DashScope key.", "The DashScope key only authenticates inference; there is no public quota query for it."),
        Provider::Groq => entry(Derived, InferenceKey, Scope::Organization,
            doc("https://console.groq.com/docs/rate-limits"),
            "groq", "Organization rate limits read from the x-ratelimit-* headers of the model listing."),
        Provider::Perplexity if params.get("product").is_some_and(|v| v.eq_ignore_ascii_case("computer")) => enterprise(entry(Official, AdminKey, Scope::Organization,
            doc("https://docs.perplexity.ai/docs/admin/computer-analytics-api.md"),
            "perplexity_computer", "Organization credits through the Perplexity Computer Analytics API.")),
        Provider::Perplexity => unavailable(InferenceKey,
            doc("https://docs.perplexity.ai/docs/admin/computer-analytics-api.md"), "perplexity",
            "Perplexity Sonar API key.",
            "The Sonar API has no public balance or usage query per key; Perplexity Computer uses the Analytics API (product = computer)."),
        Provider::Mistral => enterprise(entry(Official, AdminKey, Scope::Organization,
            doc("https://docs.mistral.ai/admin/admin-api/usage-metrics.md"),
            "mistral", "Organization spend limit through Mistral's Admin API.")),
        Provider::CloudflareAi => unavailable(InferenceKey,
            doc("https://developers.cloudflare.com/workers-ai/platform/pricing/"), "cloudflare_ai",
            "Cloudflare Workers AI key.",
            "Workers AI Neuron usage is only shown in the dashboard; there is no public query per key."),
        Provider::OllamaCloud => entry(Undocumented, InferenceKey, Scope::Account, router!("misc.js"),
            "ollama_cloud", "Ollama Cloud usage report through the internal interface 9router uses."),
        Provider::XiaomiMimo => entry(Undocumented, UserSession, Scope::Account, router!("xiaomi-mimo.js"),
            "xiaomi_mimo", "Xiaomi MiMo weekly quota or balance with the account's session cookie."),
        Provider::OpenCodeGo => entry(Undocumented, InferenceKey, Scope::Account, router!("opencode-go.js"),
            "open_code_go", "OpenCode Go rolling, weekly and monthly percentages through the internal usage interface."),
        Provider::Qoder => entry(Undocumented, SubscriptionSession, Scope::Account, router!("misc.js"),
            "qoder", "Qoder quota by exchanging the PAT for a job token and the internal usage interface."),
        Provider::GeminiAi if oauth || param("product", "cli") => entry(Undocumented, SubscriptionSession, Scope::Account,
            router!("google.js"), "gemini_ai_cli", "Gemini CLI quota through the internal Code Assist interface."),
        Provider::GeminiAi => unavailable(InferenceKey,
            doc("https://ai.google.dev/gemini-api/docs/rate-limits"), "gemini_ai",
            "AI Studio API key.",
            "The AI Studio key has no public usage or remaining-quota query; use Gemini (Cloud Monitoring) or the Gemini CLI."),
        Provider::Grok if oauth || param("product", "cli") => entry(Undocumented, SubscriptionSession, Scope::Account,
            router!("grok-cli.js"), "grok_cli", "Credits and SuperGrok pool through the Grok CLI's internal interface."),
        Provider::Grok => entry(Official, AdminKey, Scope::Organization,
            doc("https://docs.x.ai/developers/rest-api-reference/management/billing.md"),
            "grok", "Team prepaid balance and invoice preview through the xAI Management API."),
        Provider::NvidiaNim => unavailable(InferenceKey,
            doc("https://docs.api.nvidia.com/nim/reference/llm-apis"), "nvidia_nim",
            "NVIDIA NIM inference key.",
            "Billing lives in the NVIDIA Cloud account; there is no public balance query per inference key."),
        Provider::Poolside => unavailable(InferenceKey,
            doc("https://docs.poolside.ai/api/overview.md"), "poolside",
            "Poolside inference key.",
            "The known usage endpoint needs a platform admin token, not the inference key."),
        Provider::Kiro => entry(Undocumented, SubscriptionSession, Scope::Account, router!("kiro.js"),
            "kiro", "Kiro credits through the internal getUsageLimits interface."),
        Provider::Cline => entry(Official, if oauth { SubscriptionSession } else { InferenceKey },
            if set("organization_id") { Scope::Organization } else { Scope::Account },
            doc("https://docs.cline.bot/enterprise-solutions/api-reference.md"),
            "cline", "Personal or organization credit balance through Cline's account API."),
        Provider::CodeBuddy => entry(Undocumented, SubscriptionSession, Scope::Account, router!("codebuddy-cn.js"),
            "code_buddy", "CodeBuddy resources and packages through the internal billing interface."),
        Provider::Zed => entry(Undocumented, SubscriptionSession, Scope::Account, router!("zed.js"),
            "zed", "Zed plan usage through the client's internal interface."),
        Provider::AlibabaModelStudio => unavailable(InferenceKey,
            doc("https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope"), "alibaba_model_studio",
            "Model Studio inference key.",
            "The Model Studio inference key gives no access to a public balance or quota query."),
        Provider::AlibabaCodingPlan => entry(Undocumented,
            if set("session_env") { UserSession } else { InferenceKey }, Scope::Account,
            Evidence::Reference("https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Alibaba/AlibabaCodingPlanUsageFetcher.swift"),
            "alibaba_coding_plan", "5 h, weekly and monthly Coding Plan usage through the console's internal RPC."),
        Provider::AlibabaTokenPlan => entry(Undocumented,
            if set("session_env") { UserSession } else { LocalCli }, Scope::Account,
            Evidence::Reference("https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/token-plan.ts"),
            "alibaba_token_plan", "5 h and weekly Token Plan quota through the official CLI or the Qwen Cloud console."),
        Provider::AlibabaCloud => entry(Official, CloudIdentity, Scope::WholeCloud,
            doc("https://www.alibabacloud.com/help/en/user-center/developer-reference/api-bssopenapi-2017-12-14-queryaccountbalance"),
            "alibaba_cloud", "Balance and bill of every product of the Alibaba Cloud account through the BSS OpenAPI."),
        Provider::Antigravity => entry(Undocumented, SubscriptionSession, Scope::Account, router!("antigravity-weekly.js"),
            "antigravity", "Antigravity per-model, session and weekly quotas through the internal Code Assist interface."),
        Provider::Kimchi => entry(Undocumented, InferenceKey, Scope::Account,
            Evidence::Reference("https://github.com/getkimchi/kimchi/blob/36cc0406339a50f5ced984266d0306fb8a150408/src/extensions/billing/status.ts"),
            "kimchi", "Kimchi USD balance and budgets through the official client's contract."),
        Provider::ElevenLabs => UsageAccess {
            verified_on: "2026-10-23",
            ..entry(Official, InferenceKey, Scope::Account,
                doc("https://elevenlabs.io/docs/api-reference/user/subscription/get"),
                "eleven_labs", "Reads credits used, plan limit and next reset from the documented ElevenLabs subscription endpoint, plus extra usage when enabled.")
        },
        Provider::NineRouter => entry(Undocumented, AggregatorLogin, Scope::Instance,
            Evidence::Reference("https://github.com/decolua/9router/blob/f39d8f07481ae8518e93175f0ecaaafa61573c6c/src/dashboardGuard.js"),
            "nine_router", "Quotas of the connections you already added in your own 9router instance, read through its internal management routes."),
        Provider::OpenRouter => {
            let account = param("product", "account");
            let mut access = if account {
                entry(Official, AdminKey, Scope::Account,
                    doc("https://openrouter.ai/docs/api/api-reference/credits/get-credits"),
                    "open_router_account", "Credit balance and optional spend by upstream provider through the documented OpenRouter management endpoints.")
            } else {
                entry(Official, InferenceKey, Scope::Account,
                    doc("https://openrouter.ai/docs/api/api-reference/api-keys/get-current-key"),
                    "open_router", "Key limit, spend by period and the free-model request counter through the documented OpenRouter key endpoint.")
            };
            access.credential_hint = if account {
                Localized::fixed(
                    "access.credential_hint.open_router_account",
                    "Needs an OpenRouter management key, which cannot make inference calls; the default view uses the inference key.",
                )
            } else {
                Localized::fixed(
                    "access.credential_hint.open_router",
                    "Uses an OpenRouter inference key and shows only that key's numbers, not the account balance.",
                )
            };
            if account {
                access.risk = Localized::fixed(
                    "access.risk.open_router_management_key",
                    "A management key can also create and delete keys, so it is more powerful than an inference key: keep it in the system vault, not in a shared shell profile.",
                );
            }
            access
        }
        Provider::LiteLlm => entry(Official, AdminKey, Scope::Instance,
            doc("https://docs.litellm.ai/docs/proxy/provider_budget_routing"),
            "litellm", "USD budget and spend per provider of your LiteLLM proxy, through its documented provider budget route."),
        Provider::Extension => entry(Extension, LocalCli, Scope::Account,
            doc("https://github.com/jmarcon/cuotai/blob/main/docs/extensions.md"),
            "extension", "Usage reported by a user-approved local extension."),
    };
    if provider == Provider::Extension {
        access.credential_hint = Localized::fixed(
            "access.credential_hint.extension",
            "Receives only the environment variables requested in the manifest and approved by the user.",
        );
    }
    if provider == Provider::NineRouter {
        access.risk = Localized::fixed(
            "access.risk.nine_router",
            "Internal, undocumented 9router interface; reading usage can make 9router renew the sign-ins of the providers it holds.",
        );
    }
    access.metered = provider.is_metered();
    if access.level != Unavailable {
        access.overage = match provider {
            Provider::Cursor if oauth || param("query_mode", "personal") => OverageReporting::Money,
            Provider::Kiro => OverageReporting::Money,
            Provider::Claude => OverageReporting::ProviderUnits,
            Provider::Grok if oauth || param("product", "cli") => OverageReporting::ProviderUnits,
            _ => OverageReporting::NotReported,
        };
    }
    access
}

/// Every param value that may switch a provider to another queried
/// interface (`product`, `query_mode`, `mode`, a session or an organization),
/// walked with and without the native login by [`has_usage_query`].
const SWITCHING_PARAMS: [&[(&str, &str)]; 9] = [
    &[],
    &[("product", "cli")],
    &[("product", "coding")],
    &[("product", "computer")],
    &[("query_mode", "personal")],
    &[("query_mode", "quota")],
    &[("mode", "org")],
    &[("session_env", "X")],
    &[("organization_id", "X")],
];

/// Whether some mode of `provider` has a usage query, that is, whether it is
/// not [`Level::Unavailable`] in every mode. A provider without any query
/// would only produce an account that shows a reason instead of numbers, so
/// the account editors leave it out of the provider picker; the
/// documentation (`docs/providers.md`) keeps listing it.
pub fn has_usage_query(provider: Provider) -> bool {
    SWITCHING_PARAMS.iter().any(|params| {
        let params: BTreeMap<String, String> = params
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        [false, true]
            .into_iter()
            .any(|oauth| classify(provider, oauth, &params).level != Level::Unavailable)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oauth::OAuthConfig;
    use crate::providers::QueryError;
    use Credential as C;
    use Level::*;

    struct NoCredentials;
    impl crate::Credentials for NoCredentials {
        fn get(&self, _: &str) -> Option<String> {
            None
        }
    }

    /// The report `Monitor::refresh_account` builds from a failed query.
    fn failed(account: &Account, error: QueryError) -> Report {
        let (notes, notes_l10n) = crate::text::Notes::one(error.note).finish();
        Report {
            account_id: None,
            diagnostic: None,
            account: account.name.clone(),
            provider: account.provider,
            checked_at: chrono::Utc::now(),
            status: error.status,
            metrics: Vec::new(),
            notes,
            notes_l10n,
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        }
    }

    fn account(provider: Provider, params: &[(&str, &str)], oauth: bool) -> Account {
        Account {
            id: None,
            provider,
            oauth: oauth.then(|| OAuthConfig::new(String::new())),
            params: params
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Default::default()
        }
    }

    /// Value standing for "any non-empty value" in [`MODES`].
    const SET: &str = "X";

    /// The default account plus every param that may switch the queried
    /// interface; the native login is walked separately.
    const MODES: [&[(&str, &str)]; 10] = [
        &[],
        &[("product", "account")],
        &[("product", "cli")],
        &[("product", "coding")],
        &[("product", "computer")],
        &[("query_mode", "personal")],
        &[("query_mode", "quota")],
        &[("mode", "org")],
        &[("session_env", SET)],
        &[("organization_id", SET)],
    ];

    /// Every interface-switching param and the OAuth login, per provider.
    fn variants() -> Vec<Account> {
        const CASE: &[(&str, &str)] = &[("product", "Computer")];
        Provider::ALL
            .into_iter()
            .flat_map(|p| {
                MODES
                    .into_iter()
                    .chain([CASE])
                    .flat_map(move |m| [false, true].map(|oauth| account(p, m, oauth)))
            })
            .collect()
    }

    /// Only providers that are `Unavailable` in every mode lack a query; a
    /// provider with one working mode (Gemini AI, Perplexity) keeps it.
    #[test]
    fn has_usage_query_is_false_only_without_any_queried_mode() {
        use Provider::*;
        let without: Vec<_> = Provider::ALL
            .into_iter()
            .filter(|p| !has_usage_query(*p))
            .collect();
        assert_eq!(
            without,
            [Qwen, CloudflareAi, NvidiaNim, Poolside, AlibabaModelStudio]
        );
    }

    #[test]
    fn classification_table_covers_every_provider_and_mode() {
        use Provider::*;
        let base: [(Provider, Level, Credential); 40] = [
            (OpenAiApi, Official, C::AdminKey),
            (AnthropicApi, Official, C::AdminKey),
            (ChatGpt, Undocumented, C::SubscriptionSession),
            (Claude, Undocumented, C::SubscriptionSession),
            (GeminiApi, Derived, C::CloudIdentity),
            (VertexAi, Derived, C::CloudIdentity),
            (AwsBedrock, Official, C::CloudIdentity),
            (AzureOpenAi, Official, C::CloudIdentity),
            (Cursor, Official, C::AdminKey),
            (Copilot, Official, C::AdminKey),
            (Windsurf, Official, C::AdminKey),
            (MiniMax, Undocumented, C::InferenceKey),
            (Kimi, Official, C::InferenceKey),
            (Qwen, Unavailable, C::InferenceKey),
            (Groq, Derived, C::InferenceKey),
            (Perplexity, Unavailable, C::InferenceKey),
            (Mistral, Official, C::AdminKey),
            (CloudflareAi, Unavailable, C::InferenceKey),
            (OllamaCloud, Undocumented, C::InferenceKey),
            (XiaomiMimo, Undocumented, C::UserSession),
            (OpenCodeGo, Undocumented, C::InferenceKey),
            (Qoder, Undocumented, C::SubscriptionSession),
            (GeminiAi, Unavailable, C::InferenceKey),
            (Grok, Official, C::AdminKey),
            (NvidiaNim, Unavailable, C::InferenceKey),
            (Poolside, Unavailable, C::InferenceKey),
            (Kiro, Undocumented, C::SubscriptionSession),
            (Cline, Official, C::InferenceKey),
            (CodeBuddy, Undocumented, C::SubscriptionSession),
            (Zed, Undocumented, C::SubscriptionSession),
            (AlibabaModelStudio, Unavailable, C::InferenceKey),
            (AlibabaCodingPlan, Undocumented, C::InferenceKey),
            (AlibabaTokenPlan, Undocumented, C::LocalCli),
            (AlibabaCloud, Official, C::CloudIdentity),
            (Antigravity, Undocumented, C::SubscriptionSession),
            (Kimchi, Undocumented, C::InferenceKey),
            (ElevenLabs, Official, C::InferenceKey),
            (NineRouter, Undocumented, C::AggregatorLogin),
            (OpenRouter, Official, C::InferenceKey),
            (LiteLlm, Official, C::AdminKey),
        ];
        let mut seen: Vec<_> = base.iter().map(|row| row.0).collect();
        seen.dedup();
        assert_eq!(seen, Provider::ALL, "one row per provider, in ALL order");
        for (provider, level, credential) in base {
            let got = describe(&account(provider, &[], false));
            assert_eq!(
                (got.level, got.credential),
                (level, credential),
                "{provider:?}"
            );
        }
        // Params and logins that switch the queried interface:
        // (provider, params, oauth, level, credential, summary slug).
        type Mode = (
            Provider,
            &'static [(&'static str, &'static str)],
            bool,
            Level,
            Credential,
            &'static str,
        );
        let modes: [Mode; 19] = [
            (
                Cursor,
                &[("query_mode", "personal")],
                false,
                Undocumented,
                C::SubscriptionSession,
                "cursor_personal",
            ),
            (
                Cursor,
                &[],
                true,
                Undocumented,
                C::SubscriptionSession,
                "cursor_personal",
            ),
            (
                Copilot,
                &[("query_mode", "quota")],
                false,
                Undocumented,
                C::SubscriptionSession,
                "copilot_quota",
            ),
            (
                Copilot,
                &[],
                true,
                Undocumented,
                C::SubscriptionSession,
                "copilot_quota",
            ),
            (
                Copilot,
                &[("query_mode", "billing")],
                false,
                Official,
                C::AdminKey,
                "copilot_billing",
            ),
            (
                Kimi,
                &[("product", "coding")],
                false,
                Undocumented,
                C::InferenceKey,
                "kimi_coding",
            ),
            (
                Kimi,
                &[],
                true,
                Undocumented,
                C::SubscriptionSession,
                "kimi_coding",
            ),
            (
                Grok,
                &[("product", "cli")],
                false,
                Undocumented,
                C::SubscriptionSession,
                "grok_cli",
            ),
            (
                Grok,
                &[("product", "management")],
                false,
                Official,
                C::AdminKey,
                "grok",
            ),
            (
                GeminiAi,
                &[("product", "cli")],
                false,
                Undocumented,
                C::SubscriptionSession,
                "gemini_ai_cli",
            ),
            (
                GeminiAi,
                &[("product", "default")],
                false,
                Unavailable,
                C::InferenceKey,
                "gemini_ai",
            ),
            (
                GeminiAi,
                &[],
                true,
                Undocumented,
                C::SubscriptionSession,
                "gemini_ai_cli",
            ),
            (
                Qwen,
                &[],
                true,
                Unavailable,
                C::SubscriptionSession,
                "qwen_oauth",
            ),
            (
                Perplexity,
                &[("product", "computer")],
                false,
                Official,
                C::AdminKey,
                "perplexity_computer",
            ),
            (
                Perplexity,
                &[("product", "Computer")],
                false,
                Official,
                C::AdminKey,
                "perplexity_computer",
            ),
            (Cline, &[], true, Official, C::SubscriptionSession, "cline"),
            (
                OpenRouter,
                &[("product", "account")],
                false,
                Official,
                C::AdminKey,
                "open_router_account",
            ),
            (
                AlibabaCodingPlan,
                &[("session_env", "X")],
                false,
                Undocumented,
                C::UserSession,
                "alibaba_coding_plan",
            ),
            (
                AlibabaTokenPlan,
                &[("session_env", "X")],
                false,
                Undocumented,
                C::UserSession,
                "alibaba_token_plan",
            ),
        ];
        for (provider, params, oauth, level, credential, slug) in modes {
            let got = describe(&account(provider, params, oauth));
            assert_eq!(
                (got.level, got.credential),
                (level, credential),
                "{provider:?} {params:?} {oauth}"
            );
            assert_eq!(got.summary.id, format!("access.summary.{slug}"));
        }
    }

    #[test]
    fn scope_plan_and_cost_follow_the_interface() {
        use Provider::*;
        let scope = |p, params: &[(&str, &str)]| describe(&account(p, params, false)).scope;
        assert_eq!(scope(AlibabaCloud, &[]), Scope::WholeCloud);
        assert_eq!(scope(Copilot, &[]), Scope::Account);
        assert_eq!(scope(Copilot, &[("mode", "org")]), Scope::Organization);
        assert_eq!(scope(Cline, &[]), Scope::Account);
        assert_eq!(
            scope(Cline, &[("organization_id", "org_1")]),
            Scope::Organization
        );
        let plan = |p, params: &[(&str, &str)]| {
            describe(&account(p, params, false))
                .plan_requirement
                .map(|l| l.id)
        };
        assert_eq!(
            plan(Windsurf, &[]).as_deref(),
            Some("access.plan.enterprise")
        );
        assert_eq!(
            plan(Mistral, &[]).as_deref(),
            Some("access.plan.enterprise")
        );
        assert_eq!(
            plan(Cursor, &[]).as_deref(),
            Some("access.plan.teams_enterprise")
        );
        assert_eq!(plan(Cursor, &[("query_mode", "personal")]), None);
        assert_eq!(
            plan(Perplexity, &[("product", "computer")]).as_deref(),
            Some("access.plan.enterprise")
        );
        for account in variants() {
            assert_eq!(describe(&account).metered, account.provider.is_metered());
        }
        assert!(describe(&account(AwsBedrock, &[], false)).metered);
    }

    #[test]
    fn every_descriptor_keeps_the_level_invariants() {
        for case in variants() {
            let access = describe(&case);
            let at = format!(
                "{:?} {:?} {}",
                case.provider,
                case.params,
                case.oauth.is_some()
            );
            // Unavailable always explains itself; the others never carry a reason.
            assert_eq!(
                access.reason.is_some(),
                access.level == Level::Unavailable,
                "{at}"
            );
            // Official cites vendor documentation, never a community reference.
            if access.level == Level::Official {
                assert!(
                    matches!(access.evidence, Evidence::Documentation(_)),
                    "{at}"
                );
            }
            // Undocumented has no vendor documentation: its evidence is the
            // public contract `explain` cites when the interface changes.
            if access.level == Level::Undocumented {
                assert!(matches!(access.evidence, Evidence::Reference(_)), "{at}");
            }
            // RF2.2: public HTTPS sources only, no credentials in the URL, no URL in the texts.
            let url = access.evidence.url().to_ascii_lowercase();
            assert!(url.starts_with("https://") && !url.contains('@'), "{at}");
            assert!(
                !["token=", "key=", "sig=", "secret"]
                    .iter()
                    .any(|s| url.contains(s)),
                "{at}"
            );
            assert_eq!(access.verified_on.len(), 10, "{at}");
            assert_ne!(
                access.verified_on, "1970-01-01",
                "{at}: missing from VERIFIED"
            );
            // RF6.3: every subscription-session interface carries the token risk.
            assert_eq!(
                access.token_risk.is_some(),
                access.credential == C::SubscriptionSession,
                "{at}"
            );
            let texts = [&access.summary, &access.credential_hint, &access.risk]
                .into_iter()
                .chain(&access.reason)
                .chain(&access.plan_requirement);
            for text in texts {
                assert!(!text.fallback.contains("://"), "{at}: {}", text.id);
            }
        }
    }

    /// RF2: every access text resolves in pt (fallback), en-US and es, and
    /// the desktop catalogs do not duplicate the core-owned strings.
    #[test]
    fn access_texts_are_complete_in_the_three_languages() {
        use std::collections::BTreeSet;
        let mut texts: Vec<Localized> = Vec::new();
        for account in variants() {
            let access = describe(&account);
            texts.extend(access.notes());
            let today = chrono::NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
            texts.extend(access.token_risk.clone());
            for days in [0, 40, 70, 200] {
                texts.push(access.verification(today + chrono::Duration::days(days)));
            }
            texts.extend(access.cost_advisory());
            texts.extend(access.save_confirmation());
            texts.extend(access.explain(&failed(&account, QueryError::schema())));
            texts.extend([
                access.level.label(),
                access.credential.label(),
                access.scope.label(),
                access.summary,
            ]);
            texts.extend(access.plan_requirement);
        }
        for locale in crate::text::catalog_locales() {
            let catalog = crate::text::core_catalog(locale).expect("shipped catalog");
            let missing: BTreeSet<_> = texts
                .iter()
                .filter(|t| !catalog.contains_key(&*t.id))
                .map(|t| &t.id)
                .collect();
            assert!(missing.is_empty(), "{locale} lacks {missing:?}");
        }
        for locale in ["pt-BR", "en-US", "es", "it"] {
            for text in &texts {
                assert!(
                    !text.resolve(locale).trim().is_empty(),
                    "{locale}: {}",
                    text.id
                );
            }
        }
        let ui =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop/ui/locales");
        for entry in std::fs::read_dir(ui).expect("desktop locales") {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "json") {
                let raw = std::fs::read_to_string(&path).unwrap();
                let keys: serde_json::Map<String, serde_json::Value> =
                    serde_json::from_str(&raw).unwrap();
                assert!(
                    !keys.keys().any(|k| k.starts_with("access.")),
                    "{path:?} owns core access ids"
                );
            }
        }
    }

    /// RF3: the editors' view of a draft matches the saved account, and the
    /// cost warning, plan note and save confirmation follow the level.
    #[test]
    fn draft_access_matches_the_saved_account() {
        use crate::account_edit::{draft_access, DraftField, DraftValue};
        use Provider::*;
        let choice = |key, index| DraftField {
            key,
            value: DraftValue::Choice(index),
        };
        let text = |key, value: &str| DraftField {
            key,
            value: DraftValue::Text(value.into()),
        };
        let cases = [
            (
                Copilot,
                "credential",
                vec![choice("query_mode", 1)],
                &[("query_mode", "quota")][..],
                false,
            ),
            (
                Copilot,
                "credential",
                vec![choice("query_mode", 0)],
                &[],
                false,
            ),
            (
                GeminiAi,
                "credential",
                vec![choice("product", 1)],
                &[("product", "cli")],
                false,
            ),
            (
                GeminiAi,
                "credential",
                vec![choice("product", 0)],
                &[],
                false,
            ),
            (
                Perplexity,
                "credential",
                vec![text("product", " Computer ")],
                &[("product", "computer")],
                false,
            ),
            (
                Cline,
                "credential",
                vec![text("organization_id", "  ")],
                &[],
                false,
            ),
            (Cursor, "oauth", vec![], &[], true),
            // Undeclared keys and mismatched kinds never switch the interface.
            (
                Grok,
                "credential",
                vec![text("product", "cli"), choice("bogus", 1)],
                &[],
                false,
            ),
        ];
        for (provider, mode, fields, params, oauth) in cases {
            assert_eq!(
                draft_access(provider, mode, &fields, &[]),
                describe(&account(provider, params, oauth)),
                "{provider:?} {mode}"
            );
        }
        // PRD 0016 RF1: a session moved to the vault keeps the account a
        // user-session one although `session_env` is gone from the form.
        let mut migrated = account(AlibabaCodingPlan, &[], false);
        migrated.session = Some(crate::user_session::SessionConfig {
            credential_ref: "6f1d3c1e-8a52-4a3c-9f7e-2a8b5b1c0d11".into(),
            slots: ["session_env".to_string()].into(),
        });
        let stored = ["session_env".to_string()];
        let draft = draft_access(
            AlibabaCodingPlan,
            "credential",
            &[text("session_env", "")],
            &stored,
        );
        assert_eq!(draft.credential, C::UserSession);
        assert_eq!(draft, describe(&migrated));
        assert_eq!(
            describe(&account(AlibabaCodingPlan, &[], false)).credential,
            C::InferenceKey
        );
        let empty = |p| draft_access(p, "credential", &[], &[]);
        assert!(empty(AwsBedrock).cost_advisory().is_some());
        assert!(empty(OpenAiApi).cost_advisory().is_none());
        let windsurf: Vec<_> = empty(Windsurf).notes().into_iter().map(|n| n.id).collect();
        assert_eq!(
            windsurf,
            ["access.credential_hint.admin_key", "access.requires_plan"]
        );
        let poolside = empty(Poolside);
        assert_eq!(poolside.notes().last(), poolside.reason.as_ref());
        let confirm = poolside.save_confirmation().expect("unavailable asks");
        assert!(confirm
            .resolve("en-US")
            .starts_with("The known usage endpoint"));
        for account in variants() {
            let access = describe(&account);
            assert_eq!(
                access.save_confirmation().is_some(),
                access.level == Level::Unavailable
            );
        }
    }

    /// PRD 0005 RF4.2: an `Unavailable` account explains its reason instead
    /// of the generic failure; an `Undocumented` one with a schema mismatch
    /// says the interface may have changed and points to its reference.
    #[tokio::test]
    async fn explain_replaces_the_generic_failure() {
        use Provider::*;
        // The report a real refresh of an Unavailable provider produces.
        let poolside = Account {
            name: "p".into(),
            ..account(Poolside, &[], false)
        };
        let config = crate::Config {
            accounts: vec![poolside.clone()],
            ..Default::default()
        };
        let monitor = crate::Monitor::with_credentials(config, std::sync::Arc::new(NoCredentials))
            .expect("valid config");
        let unsupported = monitor.refresh_account(0).await;
        assert_eq!(unsupported.status, Status::Unsupported);
        let access = describe(&poolside);
        assert_eq!(access.explain(&unsupported), access.reason);
        assert!(access.reason.is_some());
        // Whatever the failure, an Unavailable account has no query to blame.
        let transport = failed(&poolside, QueryError::transport());
        assert_eq!(access.explain(&transport), access.reason);

        let claude = account(Claude, &[], true);
        let schema = failed(&claude, QueryError::schema());
        let changed = describe(&claude).explain(&schema).expect("schema mismatch");
        assert_eq!(changed.id, "access.interface_changed");
        let en = changed.resolve("en-US");
        assert!(en.contains("may have changed without notice"), "{en}");
        assert!(en.ends_with(describe(&claude).evidence.url()), "{en}");
        // Other failures of an Undocumented account keep their own note.
        for error in [QueryError::transport(), QueryError::config("query.x", "x")] {
            assert_eq!(describe(&claude).explain(&failed(&claude, error)), None);
        }
        // A documented or derived interface does not blame a schema change.
        for provider in [OpenAiApi, GeminiApi] {
            let acc = account(provider, &[], false);
            assert_eq!(
                describe(&acc).explain(&failed(&acc, QueryError::schema())),
                None
            );
        }
        // A successful report never gets an explanation.
        for acc in variants() {
            let ready = Report {
                status: Status::Ready,
                ..failed(&acc, QueryError::schema())
            };
            assert_eq!(describe(&acc).explain(&ready), None);
        }
    }

    /// PRD 0028 RF6.1/RF6.2: dates are per interface; the age text follows the
    /// months, and past 90 days says since when.
    #[test]
    fn verification_reads_the_age_of_the_evidence() {
        use chrono::NaiveDate;
        let day = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        let mut access = describe(&account(Provider::Claude, &[], true));
        access.verified_on = "2026-07-01";
        let en = |today| access.verification(today).resolve("en-US");
        assert_eq!(en(day(2026, 7, 10)), "Verified this month.");
        assert_eq!(en(day(2026, 8, 5)), "Verified 1 month ago.");
        assert_eq!(en(day(2026, 9, 5)), "Verified 2 months ago.");
        assert_eq!(en(day(2026, 9, 27)), "Verified 2 months ago.");
        assert_eq!(en(day(2026, 10, 2)), "Not verified since 2026-07-01.");
        access.verified_on = "not-a-date";
        assert_eq!(
            access.verification(day(2026, 10, 2)).id,
            "access.verification_stale"
        );
    }

    /// RF4.3: the serialized descriptor `export::v1` embeds per account.
    #[test]
    fn descriptor_serializes_with_stable_slugs_and_public_evidence() {
        let json = |acc: &Account| serde_json::to_value(describe(acc)).unwrap();
        let poolside = json(&account(Provider::Poolside, &[], false));
        assert_eq!(poolside["level"], "unavailable");
        assert_eq!(poolside["credential"], "inference_key");
        assert_eq!(poolside["scope"], "account");
        assert_eq!(poolside["metered"], false);
        assert_eq!(poolside["evidence"]["kind"], "documentation");
        assert_eq!(poolside["reason"]["id"], "access.reason.poolside");
        assert_eq!(poolside["plan_requirement"], serde_json::Value::Null);
        let cursor = json(&account(Provider::Cursor, &[], false));
        assert_eq!(cursor["level"], "official");
        assert_eq!(cursor["scope"], "organization");
        assert_eq!(
            cursor["plan_requirement"]["id"],
            "access.plan.teams_enterprise"
        );
        assert_eq!(cursor["reason"], serde_json::Value::Null);
        let claude = json(&account(Provider::Claude, &[], true));
        assert_eq!(claude["level"], "undocumented");
        assert_eq!(claude["credential"], "subscription_session");
        assert_eq!(claude["evidence"]["kind"], "reference");
        assert!(claude["evidence"]["url"]
            .as_str()
            .is_some_and(|url| url.starts_with("https://github.com/decolua/9router/")));
        assert_eq!(
            json(&account(Provider::AwsBedrock, &[], false))["metered"],
            true
        );
    }

    const PROVIDERS_DOC: &str = "docs/providers.md";
    const MATRIX_START: &str =
        "<!-- access-matrix:start (generated from access::describe; do not edit by hand) -->";
    const MATRIX_END: &str = "<!-- access-matrix:end -->";
    const REGENERATE: &str = "CUOTAI_REGENERATE_DOCS";

    /// Level, credential, plan, scope and evidence cells of one descriptor.
    fn matrix_cells(access: &UsageAccess) -> String {
        let en = |text: Localized| text.resolve("en-US");
        let plan = access.plan_requirement.clone().map_or("—".into(), en);
        let evidence = match access.evidence {
            Evidence::Documentation(url) => format!("[documentation]({url})"),
            Evidence::Reference(url) => {
                let file = url.rsplit('/').next().unwrap_or(url);
                format!("reference [`{file}`]({url})")
            }
        };
        format!(
            "{} | {} | {plan} | {} | {evidence} | {} | {}",
            en(access.level.label()),
            en(access.credential.label()),
            en(access.scope.label()),
            access.verified_on,
            en(access.overage.label()).to_lowercase(),
        )
    }

    fn mode_label(params: &[(&str, &str)]) -> String {
        let label = params
            .iter()
            .map(|(key, value)| match *value {
                SET => format!("`{key}` set"),
                value => format!("`{key} = \"{value}\"`"),
            })
            .collect::<Vec<_>>()
            .join(", ");
        if label.is_empty() {
            "default".into()
        } else {
            label
        }
    }

    /// PRD 0005 RF5.1: the `docs/providers.md` access matrix, one row per
    /// provider plus one per mode (param or native login) whose descriptor
    /// differs from the default; modes with the same cells share a row.
    fn access_matrix() -> String {
        use std::fmt::Write;
        let mut out = String::from(
            "| Identifier | Mode | Level | Credential | Plan | Scope | Evidence | Verified on | Overage |\n\
             |---|---|---|---|---|---|---|---|---|\n",
        );
        for provider in Provider::ALL {
            let default = matrix_cells(&describe(&account(provider, &[], false)));
            let mut rows = vec![(vec![mode_label(&[])], default.clone())];
            let login = crate::oauth::supports_login(provider)
                .then(|| ("native login".to_string(), account(provider, &[], true)));
            // PRD 0016 RF5.1: a session pasted into the vault, beside the
            // `session_env` variable it can replace.
            let vault = (!crate::user_session::descriptors(provider).is_empty())
                .then(|| ("session in the vault".to_string(), vault_account(provider)));
            let modes = MODES[1..]
                .iter()
                .map(|params| (mode_label(params), account(provider, params, false)))
                .chain(login)
                .chain(vault);
            for (label, mode) in modes {
                let cells = matrix_cells(&describe(&mode));
                if cells == default {
                    continue;
                }
                match rows.iter_mut().find(|(_, row)| *row == cells) {
                    Some((labels, _)) => labels.push(label),
                    None => rows.push((vec![label], cells)),
                }
            }
            for (labels, cells) in rows {
                let slug = provider.slug();
                writeln!(out, "| `{slug}` | {} | {cells} |", labels.join(" or ")).unwrap();
            }
        }
        let metered: Vec<_> = Provider::ALL
            .into_iter()
            .filter(|p| describe(&account(*p, &[], false)).metered)
            .map(|p| format!("`{}`", p.slug()))
            .collect();
        write!(
            out,
            "\nBilled per query (`metered`): {}.\n",
            metered.join(", ")
        )
        .unwrap();
        out
    }

    /// An account of `provider` keeping every session slot it declares in
    /// the vault and naming no variable.
    fn vault_account(provider: Provider) -> Account {
        let mut vault = account(provider, &[], false);
        vault.session = Some(crate::user_session::SessionConfig {
            credential_ref: "6f1d3c1e-8a52-4a3c-9f7e-2a8b5b1c0d11".into(),
            slots: crate::user_session::descriptors(provider)
                .iter()
                .map(|descriptor| descriptor.slot.to_string())
                .collect(),
        });
        vault
    }

    /// PRD 0016 RF5.2: a provider accepts a web session only through PRD
    /// 0005: every provider with session descriptors classifies a vault
    /// session as `UserSession` on a queried (not `Unavailable`) interface
    /// with its evidence, sends it only to declared hosts, and every
    /// interface classified `UserSession` has a descriptor to paste into.
    #[test]
    fn session_providers_go_through_the_access_descriptor() {
        for provider in Provider::ALL {
            // PRD 0036 RF2.1/RF2.6: an aggregator's vault password, key or
            // SSO session goes only to the account's own instance
            // (`account_host`) and keeps the aggregator's access arm.
            if crate::user_session::descriptors(provider)
                .iter()
                .all(|descriptor| descriptor.account_host)
                && provider.is_aggregator()
            {
                continue;
            }
            let descriptors = crate::user_session::descriptors(provider);
            let session_modes = MODES
                .iter()
                .flat_map(|params| [false, true].map(|oauth| account(provider, params, oauth)))
                .any(|mode| describe(&mode).credential == C::UserSession);
            assert_eq!(
                !descriptors.is_empty(),
                session_modes,
                "{provider:?}: a UserSession interface needs a session descriptor and vice versa"
            );
            if descriptors.is_empty() {
                continue;
            }
            let access = describe(&vault_account(provider));
            assert_eq!(access.credential, C::UserSession, "{provider:?}");
            assert_ne!(access.level, Unavailable, "{provider:?}");
            assert!(
                access.evidence.url().starts_with("https://"),
                "{provider:?}"
            );
            for descriptor in descriptors {
                assert!(
                    !descriptor.hosts.is_empty(),
                    "{provider:?} {}",
                    descriptor.slot
                );
                assert!(
                    descriptor
                        .hosts
                        .iter()
                        .all(|host| !host.contains('/') && !host.contains(':')),
                    "{provider:?} {}: hosts are bare host names",
                    descriptor.slot
                );
            }
        }
    }

    /// RF5.1: `docs/providers.md` carries the matrix `access::describe`
    /// produces today; a classification change without the document fails.
    #[test]
    fn providers_doc_matrix_matches_the_descriptors() {
        // Both doc tests rewrite the same file when regenerating.
        let _guard = crate::catalog::DOC_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(PROVIDERS_DOC);
        // Windows checkouts may convert the document to CRLF.
        let doc = std::fs::read_to_string(&path)
            .expect("providers doc")
            .replace("\r\n", "\n");
        let (head, rest) = doc
            .split_once(MATRIX_START)
            .expect("matrix start marker in docs/providers.md");
        let (current, tail) = rest
            .split_once(MATRIX_END)
            .expect("matrix end marker in docs/providers.md");
        let expected = format!("\n{}", access_matrix());
        if std::env::var_os(REGENERATE).is_some() {
            let updated = format!("{head}{MATRIX_START}{expected}{MATRIX_END}{tail}");
            std::fs::write(&path, updated).expect("write providers doc");
            return;
        }
        assert_eq!(
            current, expected,
            "{PROVIDERS_DOC} is out of date with access::describe. Regenerate it with \
             `{REGENERATE}=1 cargo test -p cuotai-core providers_doc_matrix` and commit the result."
        );
    }
}
