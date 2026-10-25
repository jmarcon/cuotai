//! PRD 0005 RF1 — how each account's usage is obtained, without network or
//! credentials: reliability level, credential class, plan requirement,
//! scope, per-query cost and evidence. Front ends render the descriptor and
//! never branch on [`Provider`]. Texts are `access.*` ids in the core
//! catalogs (Portuguese fallback). The descriptor serializes with
//! snake_case enums and `{ kind, url }` evidence, the shape `export::v1`
//! (PRD 0012) embeds as `access` per account (RF4.3).

use crate::text::Localized;
use crate::{Account, Provider, Report, Status};
use serde::Serialize;
use std::collections::BTreeMap;

/// Date the evidence below was last checked against its public source.
const VERIFIED_ON: &str = "2026-10-20";

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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    Account,
    Organization,
    /// A bill for every product of the cloud account (Alibaba Cloud).
    WholeCloud,
    Product,
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
    /// Why there is no query; `Some` exactly when `level` is `Unavailable`.
    pub reason: Option<Localized>,
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
            Self::Official => "Oficial",
            Self::Undocumented => "Não documentada",
            Self::Derived => "Derivada",
            Self::Unavailable => "Indisponível",
            Self::Extension => "Extensão",
        };
        Localized::fixed(format!("access.level.{}", self.slug()), pt)
    }

    /// `access.risk.*`: what the level means for the reading's trust.
    pub(crate) fn risk(self) -> Localized {
        let pt = match self {
            Self::Official => "Interface documentada pelo fornecedor para esta credencial; não garante permissão na sua conta.",
            Self::Undocumented => "Interface interna, não documentada pelo fornecedor; pode mudar sem aviso.",
            Self::Derived => "O uso é inferido de outra fonte (métricas ou cabeçalhos de rate limit); não é um saldo e pode atrasar.",
            Self::Unavailable => "Não há consulta de uso para esta credencial; a conta mostra o motivo em vez de números.",
            Self::Extension => "Executa um programa local; revise o código antes de ativar.",
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
        }
    }

    /// `access.credential.*`: credential class label.
    pub fn label(self) -> Localized {
        let pt = match self {
            Self::InferenceKey => "Chave de inferência",
            Self::AdminKey => "Chave de administrador",
            Self::SubscriptionSession => "Sessão da assinatura",
            Self::CloudIdentity => "Identidade de nuvem",
            Self::LocalCli => "CLI local",
            Self::UserSession => "Sessão do usuário",
        };
        Localized::fixed(format!("access.credential.{}", self.slug()), pt)
    }

    fn hint(self) -> Localized {
        let pt = match self {
            Self::InferenceKey => "Usa a chave de API da conta; nenhuma resposta é gerada.",
            Self::AdminKey => "Precisa de uma chave de administrador ou de cobrança, não da chave de inferência.",
            Self::SubscriptionSession => "Usa a sessão da assinatura (login nativo ou o token dela), não uma chave de API da plataforma.",
            Self::CloudIdentity => "Usa uma identidade de nuvem (token OAuth, perfil de CLI ou AccessKey) com permissão de leitura de cobrança ou monitoramento.",
            Self::LocalCli => "Usa um perfil já autenticado do CLI do fornecedor instalado nesta máquina.",
            Self::UserSession => "Precisa de uma sessão web que você cola no assistente da conta (guardada no cofre do sistema) ou exporta numa variável; ela expira e nunca é importada automaticamente.",
        };
        Localized::fixed(format!("access.credential_hint.{}", self.slug()), pt)
    }
}

impl Scope {
    /// `access.scope.*`: what the number covers.
    pub fn label(self) -> Localized {
        let (slug, pt) = match self {
            Self::Account => ("account", "Conta"),
            Self::Organization => ("organization", "Organização"),
            Self::WholeCloud => ("whole_cloud", "Nuvem inteira"),
            Self::Product => ("product", "Produto"),
        };
        Localized::fixed(format!("access.scope.{slug}"), pt)
    }
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
        verified_on: VERIFIED_ON,
        summary: Localized::fixed(format!("access.summary.{slug}"), summary),
        credential_hint: credential.hint(),
        risk: level.risk(),
        reason: None,
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
    fn requires(mut self, plan: &str, pt: &str) -> Self {
        self.plan_requirement = Some(Localized::fixed(format!("access.plan.{plan}"), pt));
        self
    }

    /// `access.cost_advisory`: the per-query cost warning (PRD 0005 RF3.3),
    /// `Some` exactly when `metered`.
    pub fn cost_advisory(&self) -> Option<Localized> {
        self.metered.then(|| {
            Localized::fixed(
                "access.cost_advisory",
                "Provedor cobrado por consulta: cada ciclo consulta e gera cobrança. \
                 O intervalo mínimo configurado é 300s; valores menores são rejeitados \
                 pela validação do core. Mantenha um intervalo alto (ex.: 3600s) \
                 para reduzir custo. Atualizações manuais continuam explícitas.",
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
            let pt = format!("Requer o plano {}.", plan.fallback);
            Localized::with_param("access.requires_plan", "plan", plan, pt)
        }));
        notes.extend(self.reason.clone());
        notes
    }

    /// PRD 0005 RF3.4: the warning an editor shows before saving an
    /// `Unavailable` account, confirmed with "Save anyway"; `None` for every
    /// other level.
    pub fn save_confirmation(&self) -> Option<Localized> {
        self.reason.clone().map(|reason| {
            let pt = format!(
                "{} A conta será gravada, mas mostrará este motivo em vez de números.",
                reason.fallback
            );
            Localized::with_param("access.confirm_unavailable", "reason", reason, pt)
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
                    format!(
                        "A resposta não segue a interface interna conhecida: ela pode ter \
                         mudado sem aviso. Contrato de referência: {evidence}"
                    ),
                ))
            }
            Level::Official | Level::Derived | Level::Undocumented | Level::Extension => None,
        }
    }
}

/// How `account`'s usage is obtained. Pure: reads only the provider, the
/// authentication type and the `params` that switch the queried interface;
/// a session slot held in the vault (PRD 0016) counts as a set parameter.
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
            "open_ai_api", "Custos e uso da organização pela Admin API da OpenAI."),
        Provider::AnthropicApi => entry(Official, AdminKey, Scope::Organization,
            doc("https://platform.claude.com/docs/en/manage-claude/usage-cost-api"),
            "anthropic_api", "Custos e uso da organização pela Usage and Cost Admin API da Anthropic."),
        Provider::ChatGpt => entry(Undocumented, SubscriptionSession, Scope::Account,
            Evidence::Reference("https://github.com/openai/codex/blob/main/codex-rs/backend-client/src/client/rate_limit_resets.rs"),
            "chat_gpt", "Janelas de limite do Codex pela interface interna do Codex CLI."),
        Provider::Claude => entry(Undocumented, SubscriptionSession, Scope::Account, router!("claude.js"),
            "claude", "Janelas de uso do plano Claude pela interface interna usada pelos clientes oficiais."),
        Provider::GeminiApi => entry(Derived, CloudIdentity, Scope::Product,
            doc("https://cloud.google.com/monitoring/api/metrics_gcp_p_z"),
            "gemini_api", "Limites e consumo recente da Gemini API lidos nas métricas do Cloud Monitoring."),
        Provider::VertexAi => entry(Derived, CloudIdentity, Scope::Product,
            doc("https://cloud.google.com/monitoring/api/metrics_gcp_p_z"),
            "vertex_ai", "Limites e consumo recente da Vertex AI lidos nas métricas do Cloud Monitoring."),
        Provider::AwsBedrock => entry(Official, CloudIdentity, Scope::Product,
            doc("https://docs.aws.amazon.com/cli/latest/reference/ce/get-cost-and-usage.html"),
            "aws_bedrock", "Custo do mês do Amazon Bedrock pelo Cost Explorer; cada consulta é cobrada."),
        Provider::AzureOpenAi => entry(Official, CloudIdentity, Scope::Product,
            doc("https://learn.microsoft.com/en-us/rest/api/cost-management/query/usage?view=rest-cost-management-2025-03-01"),
            "azure_open_ai", "Custo do mês do recurso escolhido pelo Azure Cost Management."),
        Provider::Cursor if oauth || param("query_mode", "personal") => entry(Undocumented, SubscriptionSession, Scope::Account,
            Evidence::Reference("https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Cursor/CursorStatusProbe.swift"),
            "cursor_personal", "Resumo da assinatura pessoal do Cursor pela interface interna do site."),
        Provider::Cursor => entry(Official, AdminKey, Scope::Organization,
            doc("https://cursor.com/docs/account/teams/admin-api"),
            "cursor_teams", "Gasto da equipe no ciclo pela Admin API do Cursor.")
            .requires("teams_enterprise", "Teams ou Enterprise"),
        Provider::Copilot if oauth || param("query_mode", "quota") => entry(Undocumented, SubscriptionSession, Scope::Account,
            router!("github.js"), "copilot_quota", "Quota pessoal de interações do Copilot pela interface interna do GitHub."),
        Provider::Copilot => entry(Official, AdminKey,
            if param("mode", "org") { Scope::Organization } else { Scope::Account },
            doc("https://docs.github.com/en/rest/billing/usage"),
            "copilot_billing", "Uso e custo cobrados do Copilot pela API de billing do GitHub."),
        Provider::Windsurf => enterprise(entry(Official, AdminKey, Scope::Organization,
            doc("https://docs.windsurf.com/windsurf/accounts/api-reference/get-team-credit-balance"),
            "windsurf", "Créditos da equipe no ciclo pela API de contas do Windsurf.")),
        Provider::MiniMax => entry(Undocumented, InferenceKey, Scope::Account, router!("minimax.js"),
            "mini_max", "Janelas do Token/Coding Plan do MiniMax pelos endpoints usados pelo 9router."),
        Provider::Kimi if oauth || param("product", "coding") => entry(Undocumented,
            if oauth { SubscriptionSession } else { InferenceKey }, Scope::Account, router!("kimi.js"),
            "kimi_coding", "Quota semanal e rate limit do Kimi Coding pela interface interna da assinatura."),
        Provider::Kimi => entry(Official, InferenceKey, Scope::Account,
            doc("https://platform.kimi.ai/docs/api/balance.md"),
            "kimi", "Saldo da plataforma internacional do Kimi pelo endpoint de saldo documentado."),
        Provider::Qwen if oauth => unavailable(SubscriptionSession,
            doc("https://qwenlm.github.io/qwen-code-docs/en/users/configuration/auth/"),
            "qwen_oauth",
            "Sessão Qwen Code OAuth.",
            "Não há endpoint de quota confirmado para a sessão Qwen Code; o plano OAuth gratuito foi descontinuado."),
        Provider::Qwen => unavailable(InferenceKey,
            doc("https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope"), "qwen",
            "Chave DashScope do Qwen.", "A chave DashScope só autentica inferência; não há consulta pública de quota para ela."),
        Provider::Groq => entry(Derived, InferenceKey, Scope::Organization,
            doc("https://console.groq.com/docs/rate-limits"),
            "groq", "Rate limits da organização lidos nos cabeçalhos x-ratelimit-* da listagem de modelos."),
        Provider::Perplexity if params.get("product").is_some_and(|v| v.eq_ignore_ascii_case("computer")) => enterprise(entry(Official, AdminKey, Scope::Organization,
            doc("https://docs.perplexity.ai/docs/admin/computer-analytics-api.md"),
            "perplexity_computer", "Créditos da organização pela Analytics API do Perplexity Computer.")),
        Provider::Perplexity => unavailable(InferenceKey,
            doc("https://docs.perplexity.ai/docs/admin/computer-analytics-api.md"), "perplexity",
            "Chave da API Sonar do Perplexity.",
            "A API Sonar não tem consulta pública de saldo ou uso por chave; o Perplexity Computer usa a Analytics API (product = computer)."),
        Provider::Mistral => enterprise(entry(Official, AdminKey, Scope::Organization,
            doc("https://docs.mistral.ai/admin/admin-api/usage-metrics.md"),
            "mistral", "Limite de gasto da organização pela Admin API da Mistral.")),
        Provider::CloudflareAi => unavailable(InferenceKey,
            doc("https://developers.cloudflare.com/workers-ai/platform/pricing/"), "cloudflare_ai",
            "Chave do Cloudflare Workers AI.",
            "O consumo de Neurons do Workers AI só aparece no painel; não há consulta pública por chave."),
        Provider::OllamaCloud => entry(Undocumented, InferenceKey, Scope::Account, router!("misc.js"),
            "ollama_cloud", "Relatório de uso do Ollama Cloud pela interface interna usada pelo 9router."),
        Provider::XiaomiMimo => entry(Undocumented, UserSession, Scope::Account, router!("xiaomi-mimo.js"),
            "xiaomi_mimo", "Quota semanal ou saldo do Xiaomi MiMo com o cookie de sessão da conta."),
        Provider::OpenCodeGo => entry(Undocumented, InferenceKey, Scope::Account, router!("opencode-go.js"),
            "open_code_go", "Percentuais contínuo, semanal e mensal do OpenCode Go pela interface interna de uso."),
        Provider::Qoder => entry(Undocumented, SubscriptionSession, Scope::Account, router!("misc.js"),
            "qoder", "Quota do Qoder pela troca do PAT por token de job e a interface interna de uso."),
        Provider::GeminiAi if oauth || param("product", "cli") => entry(Undocumented, SubscriptionSession, Scope::Account,
            router!("google.js"), "gemini_ai_cli", "Quota do Gemini CLI pela interface interna do Code Assist."),
        Provider::GeminiAi => unavailable(InferenceKey,
            doc("https://ai.google.dev/gemini-api/docs/rate-limits"), "gemini_ai",
            "Chave de API do AI Studio.",
            "A chave do AI Studio não tem consulta pública de uso ou cota restante; use Gemini (Cloud Monitoring) ou o Gemini CLI."),
        Provider::Grok if oauth || param("product", "cli") => entry(Undocumented, SubscriptionSession, Scope::Account,
            router!("grok-cli.js"), "grok_cli", "Créditos e pool SuperGrok pela interface interna do Grok CLI."),
        Provider::Grok => entry(Official, AdminKey, Scope::Organization,
            doc("https://docs.x.ai/developers/rest-api-reference/management/billing.md"),
            "grok", "Saldo pré-pago e fatura prevista da equipe pela Management API da xAI."),
        Provider::NvidiaNim => unavailable(InferenceKey,
            doc("https://docs.api.nvidia.com/nim/reference/llm-apis"), "nvidia_nim",
            "Chave de inferência do NVIDIA NIM.",
            "A cobrança fica na conta NVIDIA Cloud; não há consulta pública de saldo por chave de inferência."),
        Provider::Poolside => unavailable(InferenceKey,
            doc("https://docs.poolside.ai/api/overview.md"), "poolside",
            "Chave de inferência do Poolside.",
            "O endpoint de uso conhecido exige um token de administrador da plataforma, não a chave de inferência."),
        Provider::Kiro => entry(Undocumented, SubscriptionSession, Scope::Account, router!("kiro.js"),
            "kiro", "Créditos do Kiro pela interface interna getUsageLimits."),
        Provider::Cline => entry(Official, if oauth { SubscriptionSession } else { InferenceKey },
            if set("organization_id") { Scope::Organization } else { Scope::Account },
            doc("https://docs.cline.bot/enterprise-solutions/api-reference.md"),
            "cline", "Saldo de créditos pessoal ou da organização pela API de contas do Cline."),
        Provider::CodeBuddy => entry(Undocumented, SubscriptionSession, Scope::Account, router!("codebuddy-cn.js"),
            "code_buddy", "Recursos e pacotes do CodeBuddy pela interface interna de billing."),
        Provider::Zed => entry(Undocumented, SubscriptionSession, Scope::Account, router!("zed.js"),
            "zed", "Uso do plano Zed pela interface interna do cliente."),
        Provider::AlibabaModelStudio => unavailable(InferenceKey,
            doc("https://www.alibabacloud.com/help/en/model-studio/qwen-api-via-dashscope"), "alibaba_model_studio",
            "Chave de inferência do Model Studio.",
            "A chave de inferência do Model Studio não dá acesso a uma consulta pública de saldo ou quota."),
        Provider::AlibabaCodingPlan => entry(Undocumented,
            if set("session_env") { UserSession } else { InferenceKey }, Scope::Account,
            Evidence::Reference("https://github.com/steipete/CodexBar/blob/main/Sources/CodexBarCore/Providers/Alibaba/AlibabaCodingPlanUsageFetcher.swift"),
            "alibaba_coding_plan", "Uso de 5 h, semana e mês do Coding Plan pelo RPC interno do console."),
        Provider::AlibabaTokenPlan => entry(Undocumented,
            if set("session_env") { UserSession } else { LocalCli }, Scope::Account,
            Evidence::Reference("https://github.com/modelstudioai/cli/blob/main/packages/commands/src/commands/usage/token-plan.ts"),
            "alibaba_token_plan", "Quota de 5 h e semana do Token Plan pelo CLI oficial ou pelo console do Qwen Cloud."),
        Provider::AlibabaCloud => entry(Official, CloudIdentity, Scope::WholeCloud,
            doc("https://www.alibabacloud.com/help/en/user-center/developer-reference/api-bssopenapi-2017-12-14-queryaccountbalance"),
            "alibaba_cloud", "Saldo e fatura de todos os produtos da conta Alibaba Cloud pela BSS OpenAPI."),
        Provider::Antigravity => entry(Undocumented, SubscriptionSession, Scope::Account, router!("antigravity-weekly.js"),
            "antigravity", "Quotas por modelo, sessão e semana do Antigravity pela interface interna do Code Assist."),
        Provider::Kimchi => entry(Undocumented, InferenceKey, Scope::Account,
            Evidence::Reference("https://github.com/getkimchi/kimchi/blob/36cc0406339a50f5ced984266d0306fb8a150408/src/extensions/billing/status.ts"),
            "kimchi", "Saldo em USD e orçamentos do Kimchi pelo contrato do cliente oficial."),
        Provider::Extension => entry(Extension, LocalCli, Scope::Account,
            doc("https://github.com/jmarcon/cuotai/blob/main/docs/extensions.md"),
            "extension", "Uso informado por uma extensão local aprovada pelo usuário."),
    };
    if provider == Provider::Extension {
        access.credential_hint = Localized::fixed(
            "access.credential_hint.extension",
            "Recebe somente as variáveis de ambiente pedidas no manifesto e aprovadas pelo usuário.",
        );
    }
    access.metered = provider.is_metered();
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
            account: account.name.clone(),
            provider: account.provider,
            checked_at: chrono::Utc::now(),
            status: error.status,
            metrics: Vec::new(),
            notes,
            notes_l10n,
            source: String::new(),
            session_expires_at: None,
        }
    }

    fn account(provider: Provider, params: &[(&str, &str)], oauth: bool) -> Account {
        Account {
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
    const MODES: [&[(&str, &str)]; 9] = [
        &[],
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
        let base: [(Provider, Level, Credential); 36] = [
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
        let modes: [Mode; 18] = [
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
        for locale in ["en-US", "es", "it"] {
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
            "{} | {} | {plan} | {} | {evidence}",
            en(access.level.label()),
            en(access.credential.label()),
            en(access.scope.label()),
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
            "| Identifier | Mode | Level | Credential | Plan | Scope | Evidence |\n\
             |---|---|---|---|---|---|---|\n",
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
