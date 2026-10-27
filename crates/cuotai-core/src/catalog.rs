//! PRD 0027 RF1 and RF2: the provider catalog and its search.
//!
//! One [`CatalogEntry`] per provider and connection mode, derived from the
//! access descriptor (`access::classify`) and the native-login support, so
//! the desktop picker and the TUI wizard describe and rank providers the
//! same way. Re-exported as `account_edit::{catalog, search}`.
//!
//! Search is pure and local: it never queries a provider and never reads a
//! credential.

use crate::access::{self, Credential, Level};
use crate::oauth::{requires_client_registration, supports_login};
use crate::text::Localized;
use crate::Provider;
use serde::Serialize;
use std::collections::BTreeMap;
use std::sync::LazyLock;

/// How the person signs in or supplies the credential (the picker's group).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignInMethod {
    BrowserSignIn,
    LocalCredentials,
    ApiKey,
    CloudProfile,
    WebSession,
    /// A gateway the person runs (9router, LiteLLM proxy): Cuotai reads
    /// the instance, never the upstream providers' credentials.
    SelfHostedGateway,
    Extension,
}

impl SignInMethod {
    /// Display order of the groups (RF1.1).
    pub const ALL: [Self; 7] = [
        Self::BrowserSignIn,
        Self::LocalCredentials,
        Self::ApiKey,
        Self::CloudProfile,
        Self::WebSession,
        Self::SelfHostedGateway,
        Self::Extension,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Self::BrowserSignIn => "browser_sign_in",
            Self::LocalCredentials => "local_credentials",
            Self::ApiKey => "api_key",
            Self::CloudProfile => "cloud_profile",
            Self::WebSession => "web_session",
            Self::SelfHostedGateway => "self_hosted_gateway",
            Self::Extension => "extension",
        }
    }

    /// Group label (`catalog.method.<slug>`).
    pub fn label(self) -> Localized {
        let fallback = match self {
            Self::BrowserSignIn => "Browser sign-in",
            Self::LocalCredentials => "Local credentials",
            Self::ApiKey => "API key",
            Self::CloudProfile => "Cloud profile",
            Self::WebSession => "Web session",
            Self::SelfHostedGateway => "Self-hosted gateway",
            Self::Extension => "Extension",
        };
        Localized::fixed(format!("catalog.method.{}", self.slug()), fallback)
    }
}

/// Kind of service; kept as a search term only (the picker groups by method).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Subscription,
    ApiPlatform,
    Cloud,
    CodingTool,
    Router,
    Extension,
}

impl Category {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Subscription => "subscription",
            Self::ApiPlatform => "api_platform",
            Self::Cloud => "cloud",
            Self::CodingTool => "coding_tool",
            Self::Router => "router",
            Self::Extension => "extension",
        }
    }

    /// Category name (`catalog.category.<slug>`).
    pub fn label(self) -> Localized {
        let fallback = match self {
            Self::Subscription => "Subscription",
            Self::ApiPlatform => "API platform",
            Self::Cloud => "Cloud",
            Self::CodingTool => "Coding tool",
            Self::Router => "Router",
            Self::Extension => "Extension",
        };
        Localized::fixed(format!("catalog.category.{}", self.slug()), fallback)
    }
}

/// A local source whose existing import flow serves a mode (PRD 0027 RF6/RF7
/// use it; the catalog only names it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LocalSource {
    /// Cursor's `state.vscdb` (`oauth::cursor_import`).
    CursorDatabase,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogEntry {
    pub provider: Provider,
    /// `auth_mode` id (`"oauth"` or `"credential"`) when the provider has
    /// more than one way to connect; `None` when it has a single one.
    pub mode: Option<&'static str>,
    pub display_name: Localized,
    pub aliases: &'static [&'static str],
    pub method: SignInMethod,
    pub category: Category,
    pub hint: Localized,
    pub level: Level,
    /// Every query is billed (`Provider::is_metered`).
    pub metered: bool,
    pub native_login: bool,
    pub local_import: Option<LocalSource>,
    /// The mode a provider opens with (RF5.1): exactly one per provider.
    pub recommended: bool,
    /// RF1.3: the extension id of an `Extension` entry (PRD 0017
    /// discovery); absent for built-in providers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extension: Option<String>,
}

/// RF1.3: a discovered extension as a catalog entry: "Extension: {name}"
/// under the Extension sign-in method and category, with its own hint.
pub fn extension_entry(entry: &crate::extensions::CatalogEntry) -> CatalogEntry {
    CatalogEntry {
        provider: Provider::Extension,
        mode: None,
        display_name: entry.label.clone(),
        aliases: &[],
        method: SignInMethod::Extension,
        category: Category::Extension,
        hint: entry.hint.clone(),
        level: entry.level,
        metered: false,
        native_login: false,
        local_import: None,
        recommended: true,
        extension: Some(entry.id.clone()),
    }
}

/// One search result; `score` is the core's ranking, higher first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogMatch {
    pub entry: CatalogEntry,
    pub score: u32,
}

/// Search terms that are not part of the display name, per provider
/// (language-neutral; the method and category names are matched localized).
fn aliases(provider: Provider) -> &'static [&'static str] {
    match provider {
        Provider::OpenAiApi => &["openai", "gpt"],
        Provider::AnthropicApi => &["claude api"],
        Provider::ChatGpt => &["codex", "openai", "chatgpt", "gpt"],
        Provider::Claude => &["anthropic", "claude code", "claude.ai"],
        Provider::GeminiApi => &["google", "gcp", "cloud monitoring"],
        Provider::VertexAi => &["google", "gcp", "vertex"],
        Provider::AwsBedrock => &["aws", "amazon", "bedrock"],
        Provider::AzureOpenAi => &["microsoft", "azure", "openai"],
        Provider::Cursor => &["cursor ai"],
        Provider::Copilot => &["github", "gh", "github copilot"],
        Provider::Windsurf => &["codeium"],
        Provider::MiniMax => &["minimax"],
        Provider::Kimi => &["moonshot"],
        Provider::Qwen => &["qwen code", "dashscope", "alibaba"],
        Provider::Groq => &[],
        Provider::Meta => &["muse", "muse spark", "llama", "meta ai", "facebook"],
        Provider::Perplexity => &["sonar"],
        Provider::Mistral => &["le chat"],
        Provider::CloudflareAi => &["workers ai", "cloudflare"],
        Provider::OllamaCloud => &["ollama"],
        Provider::XiaomiMimo => &["mimo", "xiaomi"],
        Provider::OpenCodeGo => &["opencode", "open code"],
        Provider::Qoder => &[],
        Provider::GeminiAi => &["gemini cli", "google", "ai studio"],
        Provider::Grok => &["xai", "x.ai", "grok cli"],
        Provider::NvidiaNim => &["nvidia", "nim"],
        Provider::Poolside => &[],
        Provider::Cline => &[],
        Provider::Kiro => &["codewhisperer"],
        Provider::CodeBuddy => &["tencent"],
        Provider::Zed => &[],
        Provider::AlibabaModelStudio => &["dashscope", "model studio", "alibaba"],
        Provider::AlibabaCodingPlan => &["bailian", "alibaba", "coding plan"],
        Provider::AlibabaTokenPlan => &["qwen cloud", "alibaba"],
        Provider::AlibabaCloud => &["aliyun", "alibaba"],
        Provider::Antigravity => &["google"],
        Provider::Kimchi => &[],
        Provider::ElevenLabs => &[
            "elevenlabs",
            "eleven labs",
            "11labs",
            "tts",
            "text to speech",
            "voice",
            "voice agents",
        ],
        Provider::NineRouter => &["9router", "nine router", "router", "aggregator"],
        Provider::OpenRouter => &["openrouter", "open router", "router", "or"],
        Provider::LiteLlm => &["litellm", "lite llm", "proxy", "gateway", "aggregator"],
        Provider::Extension => &[],
    }
}

/// Exhaustive on purpose: a new provider does not compile without a category.
fn category(provider: Provider) -> Category {
    match provider {
        Provider::ChatGpt | Provider::Claude => Category::Subscription,
        Provider::OpenAiApi
        | Provider::AnthropicApi
        | Provider::MiniMax
        | Provider::Kimi
        | Provider::Qwen
        | Provider::Groq
        | Provider::Perplexity
        | Provider::Mistral
        | Provider::CloudflareAi
        | Provider::OllamaCloud
        | Provider::XiaomiMimo
        | Provider::GeminiAi
        | Provider::Grok
        | Provider::NvidiaNim
        | Provider::Poolside
        | Provider::AlibabaModelStudio
        | Provider::Kimchi
        | Provider::ElevenLabs
        | Provider::Meta => Category::ApiPlatform,
        Provider::GeminiApi
        | Provider::VertexAi
        | Provider::AwsBedrock
        | Provider::AzureOpenAi
        | Provider::AlibabaCloud => Category::Cloud,
        Provider::Cursor
        | Provider::Copilot
        | Provider::Windsurf
        | Provider::OpenCodeGo
        | Provider::Qoder
        | Provider::Cline
        | Provider::Kiro
        | Provider::CodeBuddy
        | Provider::Zed
        | Provider::AlibabaCodingPlan
        | Provider::AlibabaTokenPlan
        | Provider::Antigravity => Category::CodingTool,
        Provider::NineRouter | Provider::OpenRouter | Provider::LiteLlm => Category::Router,
        Provider::Extension => Category::Extension,
    }
}

/// PRD 0038 Q5: an environment variable that, when set, means the person
/// already has a key for `provider`. Presence only: the value is never read.
const ENV_HINTS: &[(Provider, &str)] = &[(Provider::OpenRouter, "OPENROUTER_API_KEY")];

/// One environment variable found set, offered as "use this key" (the
/// account gets the variable's name, never its value).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct EnvDetection {
    pub provider: Provider,
    pub variable: &'static str,
}

/// PRD 0038 Q5: consent-gated, presence-only detection of known API-key
/// variables. Without `consented` nothing is checked; `is_set` is asked only
/// whether a name is set and non-empty (for the process environment, pass
/// [`process_env_is_set`]). Meant for the CLI/TUI wizard: a GUI-launched
/// desktop app does not inherit the shell environment.
pub fn detect_env_keys(consented: bool, is_set: impl Fn(&str) -> bool) -> Vec<EnvDetection> {
    if !consented {
        return Vec::new();
    }
    ENV_HINTS
        .iter()
        .filter(|(_, variable)| is_set(variable))
        .map(|&(provider, variable)| EnvDetection { provider, variable })
        .collect()
}

/// Whether the process environment has `name` set to a non-empty value,
/// without keeping the value.
pub fn process_env_is_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|value| !value.is_empty())
}

/// The CLI a `LocalCli` credential uses, for the "Uses the {cli} CLI" hint.
fn cli_name(provider: Provider) -> &'static str {
    match provider {
        Provider::AlibabaTokenPlan => "bl",
        _ => "local",
    }
}

/// RF1.2 and RF1.5: the (method, hint) of a mode, from its credential class
/// and whether it is the native-login mode. The only table of overrides is
/// [`cli_name`]; nothing is written per provider.
fn method_and_hint(
    provider: Provider,
    credential: Credential,
    level: Level,
    metered: bool,
    native_login: bool,
) -> (SignInMethod, Localized) {
    let (method, hint) = credential_method(provider, credential, level, metered, native_login);
    // PRD 0036 RF10: aggregators (9router, LiteLLM proxy) form their own
    // "Self-hosted gateway" group whatever their credential class.
    if provider.is_aggregator() {
        return (SignInMethod::SelfHostedGateway, hint);
    }
    (method, hint)
}

fn credential_method(
    provider: Provider,
    credential: Credential,
    level: Level,
    metered: bool,
    native_login: bool,
) -> (SignInMethod, Localized) {
    let hint =
        |slug: &str, fallback: &str| Localized::fixed(format!("catalog.hint.{slug}"), fallback);
    if level == Level::Extension {
        return (
            SignInMethod::Extension,
            hint("extension", "Local extension"),
        );
    }
    match credential {
        Credential::InferenceKey if native_login => (
            SignInMethod::BrowserSignIn,
            hint("browser_sign_in", "Sign in with your browser"),
        ),
        Credential::InferenceKey => (SignInMethod::ApiKey, hint("api_key", "API key")),
        Credential::AdminKey => (SignInMethod::ApiKey, hint("admin_key", "Admin API key")),
        Credential::SubscriptionSession if native_login => (
            SignInMethod::BrowserSignIn,
            hint("browser_sign_in", "Sign in with your browser"),
        ),
        Credential::SubscriptionSession => (
            SignInMethod::ApiKey,
            hint("access_token", "Account access token"),
        ),
        Credential::CloudIdentity if metered => (
            SignInMethod::CloudProfile,
            hint("cloud_metered", "Cloud credentials · billed per query"),
        ),
        Credential::CloudIdentity => (
            SignInMethod::CloudProfile,
            hint("cloud", "Cloud credentials"),
        ),
        Credential::LocalCli => {
            let cli = cli_name(provider);
            (
                SignInMethod::LocalCredentials,
                Localized::with_param("catalog.hint.local_cli", "cli", cli, "Uses the {cli} CLI"),
            )
        }
        Credential::UserSession => (
            SignInMethod::WebSession,
            hint("web_session", "Paste a web session"),
        ),
        Credential::AggregatorLogin => (
            SignInMethod::ApiKey,
            hint("aggregator_login", "Dashboard password (optional)"),
        ),
    }
}

fn entry_for(provider: Provider, native_login: bool, multi: bool) -> CatalogEntry {
    let access = access::classify(provider, native_login, &BTreeMap::new());
    let (method, hint) = method_and_hint(
        provider,
        access.credential,
        access.level,
        access.metered,
        native_login,
    );
    CatalogEntry {
        provider,
        mode: multi.then_some(if native_login {
            "oauth"
        } else {
            crate::account_edit::AUTH_MODE_CREDENTIAL
        }),
        display_name: crate::text::provider_l10n(provider),
        aliases: aliases(provider),
        method,
        category: category(provider),
        hint,
        level: access.level,
        metered: access.metered,
        native_login,
        local_import: (provider == Provider::Cursor && native_login)
            .then_some(LocalSource::CursorDatabase),
        // RF5.1: native login first; otherwise the only (credential) mode.
        recommended: native_login || !native_login_available(provider),
        extension: None,
    }
}

/// Native login applies when the provider supports it without registering
/// an OAuth client (the same rule `build_account` uses).
fn native_login_available(provider: Provider) -> bool {
    supports_login(provider) && !requires_client_registration(provider)
}

static CATALOG: LazyLock<Vec<CatalogEntry>> = LazyLock::new(|| {
    let mut entries = Vec::new();
    for provider in Provider::ALL {
        if native_login_available(provider) {
            entries.push(entry_for(provider, true, true));
            entries.push(entry_for(provider, false, true));
        } else {
            entries.push(entry_for(provider, false, false));
        }
    }
    entries
});

/// RF1.1: every provider and connection mode, in provider order with the
/// recommended mode first.
pub fn catalog() -> Vec<CatalogEntry> {
    CATALOG.clone()
}

// ---------------------------------------------------------------------------
// Search (RF2)
// ---------------------------------------------------------------------------

/// Lower-cases and strips accents. Equivalent to NFKD folding for the Latin
/// letters the supported languages use (no normalization crate is
/// available); combining marks are dropped and anything that is not a letter
/// or digit becomes a single space.
fn fold(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let push = |c: char, out: &mut String| {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with(' ') && !out.is_empty() {
            out.push(' ');
        }
    };
    for c in text.chars() {
        if ('\u{300}'..='\u{36f}').contains(&c) {
            continue;
        }
        for lower in c.to_lowercase() {
            match base_letter(lower) {
                Some(base) => base.chars().for_each(|b| push(b, &mut out)),
                None => push(lower, &mut out),
            }
        }
    }
    out.trim_end().to_string()
}

/// ASCII base of a precomposed Latin letter, `None` when it has none.
fn base_letter(c: char) -> Option<&'static str> {
    Some(match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'ç' | 'ć' | 'č' | 'ĉ' | 'ċ' => "c",
        'ď' | 'đ' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'ì' | 'í' | 'î' | 'ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'ñ' | 'ń' | 'ň' | 'ņ' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ŏ' | 'ő' => "o",
        'ŕ' | 'ř' | 'ŗ' => "r",
        'ś' | 'š' | 'ş' | 'ŝ' => "s",
        'ť' | 'ţ' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'ý' | 'ÿ' => "y",
        'ź' | 'ž' | 'ż' => "z",
        'ß' => "ss",
        'æ' => "ae",
        'œ' => "oe",
        _ => return None,
    })
}

/// Match tier of `token` in the folded `field`, 0 when absent:
/// exact 4, prefix 3, word prefix 2, substring 1.
fn tier(field: &str, token: &str) -> u32 {
    if field == token {
        4
    } else if field.starts_with(token) {
        3
    } else if field.split(' ').any(|word| word.starts_with(token)) {
        2
    } else if field.contains(token) {
        1
    } else {
        0
    }
}

/// Folded forms of a localized text in the query locale and in en-US.
fn texts(text: &Localized, locale: &str) -> Vec<String> {
    let mut forms = vec![fold(&text.resolve(locale))];
    let en = fold(&text.resolve("en-US"));
    if !forms.contains(&en) {
        forms.push(en);
    }
    forms
}

struct Fields {
    name: Vec<String>,
    slug: String,
    aliases: Vec<String>,
    category: Vec<String>,
    method: Vec<String>,
    hint: Vec<String>,
}

impl Fields {
    fn of(entry: &CatalogEntry, locale: &str) -> Self {
        Self {
            name: texts(&entry.display_name, locale),
            slug: fold(entry.extension.as_deref().unwrap_or(entry.provider.slug())),
            aliases: entry.aliases.iter().map(|alias| fold(alias)).collect(),
            category: texts(&entry.category.label(), locale),
            method: texts(&entry.method.label(), locale),
            hint: texts(&entry.hint, locale),
        }
    }

    /// Best provider-level score of `token` (name, slug, aliases, category).
    fn provider_score(&self, token: &str) -> u32 {
        let best = |fields: &[String], weight: u32, offset: u32| {
            fields
                .iter()
                .map(|f| tier(f, token))
                .max()
                .filter(|t| *t > 0)
                .map_or(0, |t| t * weight - offset)
        };
        let name = best(&self.name, 100, 0);
        let slug = best(std::slice::from_ref(&self.slug), 100, 10);
        let alias = best(&self.aliases, 15, 0);
        let category = best(&self.category, 5, 0);
        name.max(slug).max(alias).max(category)
    }

    /// Best mode-level score of `token` (sign-in method label and hint).
    fn mode_score(&self, token: &str) -> u32 {
        let best = |fields: &[String]| fields.iter().map(|f| tier(f, token)).max().unwrap_or(0) * 5;
        best(&self.method).max(best(&self.hint))
    }
}

/// RF2: case- and accent-insensitive search over the catalog.
///
/// Scoring: exact > prefix > word prefix > substring of the display name or
/// slug, then aliases, then the category, sign-in method and hint. Every
/// word of the query must match. One result per provider (its recommended
/// mode) unless the query is only satisfied through a mode's method or hint
/// ("cursor admin"), in which case those modes are listed. Ties keep catalog
/// order. An empty query returns the whole catalog grouped by sign-in method
/// (RF2.2); a query that matches nothing returns an empty list.
pub fn search(query: &str, locale: &str) -> Vec<CatalogMatch> {
    search_in(&CATALOG, query, locale)
}

/// RF1.3/RF2: [`search`] over the catalog plus the discovered extensions'
/// entries ([`extension_entry`]), which rank and group like any other
/// entry (under the Extension method, after the built-in methods).
pub fn search_with_extensions(
    query: &str,
    locale: &str,
    extensions: &[crate::extensions::CatalogEntry],
) -> Vec<CatalogMatch> {
    if extensions.is_empty() {
        return search(query, locale);
    }
    let mut entries = CATALOG.clone();
    entries.extend(extensions.iter().map(extension_entry));
    search_in(&entries, query, locale)
}

fn search_in(entries: &[CatalogEntry], query: &str, locale: &str) -> Vec<CatalogMatch> {
    let folded = fold(query);
    if folded.is_empty() {
        let mut grouped: Vec<CatalogMatch> = entries
            .iter()
            .map(|entry| CatalogMatch {
                entry: entry.clone(),
                score: 0,
            })
            .collect();
        grouped.sort_by_key(|m| {
            SignInMethod::ALL
                .iter()
                .position(|method| *method == m.entry.method)
        });
        return grouped;
    }
    let tokens: Vec<&str> = folded.split(' ').collect();
    // (catalog index, score, needed a mode-level field)
    let mut hits: Vec<(usize, u32, bool)> = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let fields = Fields::of(entry, locale);
        let mut total = 0;
        let mut mode_specific = false;
        let mut matched = true;
        for token in &tokens {
            let provider = fields.provider_score(token);
            if provider > 0 {
                total += provider;
                continue;
            }
            let mode = fields.mode_score(token);
            if mode == 0 {
                matched = false;
                break;
            }
            total += mode;
            mode_specific = true;
        }
        if matched {
            hits.push((index, total, mode_specific));
        }
    }
    let mut kept: Vec<(usize, u32)> = Vec::new();
    for &(index, score, mode_specific) in &hits {
        // One result per provider; each extension is its own "provider".
        let identity = |i: usize| (entries[i].provider, entries[i].extension.as_deref());
        let names_mode = hits
            .iter()
            .any(|(i, _, specific)| identity(*i) == identity(index) && *specific);
        let keep = if names_mode {
            mode_specific
        } else {
            entries[index].recommended
        };
        if keep {
            kept.push((index, score));
        }
    }
    kept.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    kept.into_iter()
        .map(|(index, score)| CatalogMatch {
            entry: entries[index].clone(),
            score,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Generated documentation table (RF1.4)
// ---------------------------------------------------------------------------

#[cfg(test)]
pub(crate) static DOC_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_detection_is_consent_gated_and_presence_only() {
        use std::cell::RefCell;
        let asked = RefCell::new(Vec::new());
        let set = |name: &str| {
            asked.borrow_mut().push(name.to_string());
            name == "OPENROUTER_API_KEY"
        };
        assert!(detect_env_keys(false, set).is_empty());
        assert!(
            asked.borrow().is_empty(),
            "nothing is checked without consent"
        );
        let found = detect_env_keys(true, set);
        assert_eq!(
            found,
            [EnvDetection {
                provider: Provider::OpenRouter,
                variable: "OPENROUTER_API_KEY"
            }]
        );
        assert!(detect_env_keys(true, |_| false).is_empty());
        assert!(!process_env_is_set("CUOTAI_SURELY_UNSET_VARIABLE_0038"));
    }
    use crate::text::core_catalog;

    fn names(query: &str, locale: &str) -> Vec<&'static str> {
        search(query, locale)
            .into_iter()
            .map(|m| m.entry.provider.slug())
            .collect()
    }

    fn discovered(id: &str, name: &str) -> crate::extensions::CatalogEntry {
        crate::extensions::CatalogEntry {
            id: id.into(),
            label: Localized::with_param(
                "extensions.catalog.label",
                "name",
                name,
                "Extension: {name}",
            ),
            level: Level::Extension,
            warning: Level::Extension.risk(),
            hint: Localized::with_param(
                "extensions.catalog.hint",
                "description",
                "Team quota",
                "Team quota",
            ),
        }
    }

    /// PRD 0027 RF1.3: discovered extensions are `Extension` entries named
    /// "Extension: {name}" with their own hint, one result each, grouped
    /// last with an empty query and found by name or id.
    #[test]
    fn extension_entries_join_the_catalog_and_its_search() {
        let found = [
            discovered("team-quota", "Team"),
            discovered("proxy", "Proxy"),
        ];
        let all = search_with_extensions("", "en-US", &found);
        assert_eq!(all.len(), CATALOG.len() + 2);
        let tail: Vec<_> = all[all.len() - 2..]
            .iter()
            .map(|m| m.entry.extension.clone())
            .collect();
        assert_eq!(tail, [Some("team-quota".into()), Some("proxy".into())]);
        let entry = &all.last().unwrap().entry;
        assert_eq!(entry.method, SignInMethod::Extension);
        assert_eq!(entry.display_name.resolve("en-US"), "Extension: Proxy");
        assert_eq!(entry.hint.resolve("en-US"), "Team quota");
        assert!(entry.recommended);

        let hits = search_with_extensions("proxy", "en-US", &found);
        assert_eq!(hits[0].entry.extension.as_deref(), Some("proxy"));
        let both = search_with_extensions("extension", "en-US", &found);
        assert_eq!(
            both.iter().filter(|m| m.entry.extension.is_some()).count(),
            2,
            "one result per extension, never collapsed into one"
        );
        assert_eq!(
            search_with_extensions("team quota", "it", &found)[0]
                .entry
                .extension
                .as_deref(),
            Some("team-quota")
        );
        assert_eq!(
            search_with_extensions("codex", "en-US", &found),
            search("codex", "en-US")
        );
    }

    /// PRD 0027 RNF: a catalog search answers in under 5 ms.
    #[test]
    fn a_search_answers_within_the_budget() {
        let _warm = search("claude", "en-US");
        for query in ["claude code", "api key", "x", "voice agents", "zzzz"] {
            crate::assert_within_budget(query, std::time::Duration::from_millis(5), 5, || {
                let _ = search(query, "pt-BR");
            });
        }
    }

    /// PRD 0037 RF2.3: the ElevenLabs aliases find it.
    #[test]
    fn eleven_labs_is_found_by_its_aliases() {
        for query in [
            "elevenlabs",
            "11labs",
            "tts",
            "text to speech",
            "voice",
            "voice agents",
        ] {
            assert!(
                search(query, "en-US")
                    .iter()
                    .any(|m| m.entry.provider == Provider::ElevenLabs),
                "{query}"
            );
        }
    }

    #[test]
    fn every_provider_and_mode_has_one_entry_and_one_recommendation() {
        let entries = catalog();
        for provider in Provider::ALL {
            let own: Vec<_> = entries.iter().filter(|e| e.provider == provider).collect();
            let expected = if native_login_available(provider) {
                2
            } else {
                1
            };
            assert_eq!(own.len(), expected, "{provider:?}");
            assert_eq!(
                own.iter().filter(|e| e.recommended).count(),
                1,
                "{provider:?}: exactly one recommended mode"
            );
            let modes: std::collections::BTreeSet<_> = own.iter().map(|e| e.mode).collect();
            assert_eq!(modes.len(), own.len(), "{provider:?}: distinct modes");
            if own.len() == 2 {
                assert!(own[0].recommended && own[0].native_login, "{provider:?}");
            }
        }
        assert!(entries.iter().all(|e| e.provider != Provider::Extension));
    }

    #[test]
    fn aggregators_form_the_self_hosted_gateway_group() {
        for provider in [Provider::NineRouter, Provider::LiteLlm] {
            let access = access::classify(provider, false, &BTreeMap::new());
            let (method, _) = method_and_hint(
                provider,
                access.credential,
                access.level,
                access.metered,
                false,
            );
            assert_eq!(method, SignInMethod::SelfHostedGateway, "{provider:?}");
        }
    }

    #[test]
    fn method_and_hint_follow_the_credential_class() {
        let find = |provider, native| {
            catalog()
                .into_iter()
                .find(|e| e.provider == provider && e.native_login == native)
                .unwrap()
        };
        let hint = |e: &CatalogEntry| e.hint.resolve("en-US");
        let cursor_login = find(Provider::Cursor, true);
        assert_eq!(cursor_login.method, SignInMethod::BrowserSignIn);
        assert_eq!(hint(&cursor_login), "Sign in with your browser");
        assert_eq!(cursor_login.local_import, Some(LocalSource::CursorDatabase));
        let cursor_admin = find(Provider::Cursor, false);
        assert_eq!(cursor_admin.method, SignInMethod::ApiKey);
        assert_eq!(hint(&cursor_admin), "Admin API key");
        assert_eq!(cursor_admin.local_import, None);
        let openai = find(Provider::OpenAiApi, false);
        assert_eq!(hint(&openai), "Admin API key");
        let groq = find(Provider::Groq, false);
        assert_eq!(hint(&groq), "API key");
        let bedrock = find(Provider::AwsBedrock, false);
        assert_eq!(bedrock.method, SignInMethod::CloudProfile);
        assert_eq!(hint(&bedrock), "Cloud credentials · billed per query");
        assert!(bedrock.metered);
        let vertex = find(Provider::VertexAi, false);
        assert_eq!(hint(&vertex), "Cloud credentials");
        let token_plan = find(Provider::AlibabaTokenPlan, false);
        assert_eq!(token_plan.method, SignInMethod::LocalCredentials);
        assert_eq!(hint(&token_plan), "Uses the bl CLI");
        let mimo = find(Provider::XiaomiMimo, false);
        assert_eq!(mimo.method, SignInMethod::WebSession);
        assert_eq!(hint(&mimo), "Paste a web session");
        // The token mode of a subscription is a key-like credential.
        let claude_token = find(Provider::Claude, false);
        assert_eq!(claude_token.method, SignInMethod::ApiKey);
        assert_eq!(hint(&claude_token), "Account access token");
        // The English fallback is the embedded text.
        assert_eq!(groq.hint.resolve("pt-BR"), "Chave de API");
    }

    #[test]
    fn catalog_texts_exist_in_every_translated_catalog() {
        let mut ids = Vec::new();
        for entry in catalog() {
            ids.push(entry.hint.id.to_string());
            ids.push(entry.method.label().id.to_string());
            ids.push(entry.category.label().id.to_string());
        }
        for method in SignInMethod::ALL {
            ids.push(method.label().id.to_string());
        }
        for category in [
            Category::Subscription,
            Category::ApiPlatform,
            Category::Cloud,
            Category::CodingTool,
            Category::Router,
            Category::Extension,
        ] {
            ids.push(category.label().id.to_string());
        }
        for locale in crate::text::catalog_locales() {
            let map = core_catalog(locale).unwrap();
            for id in &ids {
                assert!(map.contains_key(id), "{locale}: missing {id}");
            }
        }
    }

    #[test]
    fn search_table_in_the_four_languages() {
        for locale in ["pt-BR", "en-US", "es", "it"] {
            assert!(names("codex", locale).contains(&"chat_gpt"), "{locale}");
            let anthropic = names("anthropic", locale);
            assert!(anthropic.contains(&"claude"), "{locale}");
            assert!(anthropic.contains(&"anthropic_api"), "{locale}");
            assert_eq!(names("copilot", locale), ["copilot"], "{locale}");
            assert_eq!(names("aws", locale)[0], "aws_bedrock", "{locale}");
            assert_eq!(names("CURSOR", locale), ["cursor"], "{locale}");
            // The accent of a name or query is irrelevant.
            assert!(
                names("alibabá", locale).contains(&"alibaba_cloud"),
                "{locale}"
            );
            assert!(names("sessão", "pt-BR").contains(&"xiaomi_mimo"));
            assert!(
                names("zzzz-no-such-provider", locale).is_empty(),
                "{locale}"
            );
        }
        let cloud_pt = names("nuvem", "pt-BR");
        assert!(cloud_pt.contains(&"vertex_ai") && cloud_pt.contains(&"aws_bedrock"));
        let cloud = names("cloud", "en-US");
        assert!(cloud.contains(&"vertex_ai") && cloud.contains(&"azure_open_ai"));
        assert!(names("nube", "es").contains(&"vertex_ai"));
        assert!(names("cloud", "it").contains(&"vertex_ai"));
    }

    #[test]
    fn method_queries_and_mode_queries_list_the_matching_modes() {
        let api_key = search("api key", "en-US");
        assert!(!api_key.is_empty());
        // Claude is listed through its key mode only, never its sign-in.
        let claude: Vec<_> = api_key
            .iter()
            .filter(|m| m.entry.provider == Provider::Claude)
            .collect();
        assert_eq!(claude.len(), 1);
        assert!(!claude[0].entry.native_login);
        // Cursor appears once, as its API-key mode.
        let cursor: Vec<_> = api_key
            .iter()
            .filter(|m| m.entry.provider == Provider::Cursor)
            .collect();
        assert_eq!(cursor.len(), 1);
        assert!(!cursor[0].entry.native_login);
        let admin = search("cursor admin", "en-US");
        assert_eq!(admin.len(), 1);
        assert_eq!(admin[0].entry.mode, Some("credential"));
        // A plain provider query gives its recommended mode only.
        let plain = search("cursor", "en-US");
        assert_eq!(plain.len(), 1);
        assert!(plain[0].entry.recommended);
    }

    #[test]
    fn ranking_prefers_exact_then_prefix_then_alias_and_is_stable() {
        let anthropic = names("anthropic", "en-US");
        assert_eq!(
            anthropic[0], "anthropic_api",
            "display-name prefix beats alias"
        );
        assert_eq!(anthropic, names("anthropic", "en-US"), "stable order");
        let zed = search("zed", "en-US");
        assert_eq!(zed[0].entry.provider, Provider::Zed);
        let scores: Vec<u32> = search("goo", "en-US").iter().map(|m| m.score).collect();
        assert!(scores.windows(2).all(|w| w[0] >= w[1]));
    }

    #[test]
    fn empty_query_groups_the_catalog_by_sign_in_method() {
        let all = search("  ", "en-US");
        assert_eq!(all.len(), catalog().len());
        let positions: Vec<usize> = all
            .iter()
            .map(|m| {
                SignInMethod::ALL
                    .iter()
                    .position(|x| *x == m.entry.method)
                    .unwrap()
            })
            .collect();
        assert!(positions.windows(2).all(|w| w[0] <= w[1]));
        // Inside a group providers keep catalog order.
        let order: Vec<_> = catalog()
            .iter()
            .filter(|e| e.method == SignInMethod::ApiKey)
            .map(|e| (e.provider, e.mode))
            .collect();
        let grouped: Vec<_> = all
            .iter()
            .filter(|m| m.entry.method == SignInMethod::ApiKey)
            .map(|m| (m.entry.provider, m.entry.mode))
            .collect();
        assert_eq!(order, grouped);
    }

    #[test]
    fn fold_removes_accents_case_and_punctuation() {
        assert_eq!(fold("Alibabá — Faturamento"), "alibaba faturamento");
        assert_eq!(fold("a\u{301}"), "a");
        assert_eq!(fold("ÇÃO"), "cao");
        assert_eq!(fold("x.ai"), "x ai");
    }

    const CATALOG_START: &str =
        "<!-- catalog-table:start (generated from account_edit::catalog; do not edit by hand) -->";
    const CATALOG_END: &str = "<!-- catalog-table:end -->";

    fn catalog_table() -> String {
        let mut out = String::from(
            "| Display name | Identifier | Mode | Sign-in method | Category | Aliases | Hint |\n\
             |---|---|---|---|---|---|---|\n",
        );
        for e in catalog() {
            let aliases = if e.aliases.is_empty() {
                "—".to_string()
            } else {
                e.aliases.join(", ")
            };
            out.push_str(&format!(
                "| {} | `{}` | {} | {} | {} | {} | {} |\n",
                e.display_name.resolve("en-US"),
                e.provider.slug(),
                e.mode.map_or("—".to_string(), |m| format!("`{m}`")),
                e.method.label().resolve("en-US"),
                e.category.label().resolve("en-US"),
                aliases,
                e.hint.resolve("en-US"),
            ));
        }
        out
    }

    /// RF1.4: `docs/providers.md` carries the generated catalog table.
    #[test]
    fn providers_doc_matrix_catalog_table_matches_the_catalog() {
        let _guard = DOC_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/providers.md");
        let doc = std::fs::read_to_string(&path)
            .expect("providers doc")
            .replace("\r\n", "\n");
        let (head, rest) = doc
            .split_once(CATALOG_START)
            .expect("catalog start marker in docs/providers.md");
        let (current, tail) = rest
            .split_once(CATALOG_END)
            .expect("catalog end marker in docs/providers.md");
        let expected = format!("\n{}", catalog_table());
        if std::env::var_os("CUOTAI_REGENERATE_DOCS").is_some() {
            let updated = format!("{head}{CATALOG_START}{expected}{CATALOG_END}{tail}");
            std::fs::write(&path, updated).expect("write providers doc");
            return;
        }
        assert_eq!(
            current, expected,
            "docs/providers.md is out of date with account_edit::catalog. Regenerate it with \
             `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_matrix` and commit the result."
        );
    }
}
