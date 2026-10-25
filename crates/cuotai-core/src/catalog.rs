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
    Extension,
}

impl SignInMethod {
    /// Display order of the groups (RF1.1).
    pub const ALL: [Self; 6] = [
        Self::BrowserSignIn,
        Self::LocalCredentials,
        Self::ApiKey,
        Self::CloudProfile,
        Self::WebSession,
        Self::Extension,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Self::BrowserSignIn => "browser_sign_in",
            Self::LocalCredentials => "local_credentials",
            Self::ApiKey => "api_key",
            Self::CloudProfile => "cloud_profile",
            Self::WebSession => "web_session",
            Self::Extension => "extension",
        }
    }

    /// Group label (`catalog.method.<slug>`).
    pub fn label(self) -> Localized {
        let fallback = match self {
            Self::BrowserSignIn => "Entrar pelo navegador",
            Self::LocalCredentials => "Credenciais locais",
            Self::ApiKey => "Chave de API",
            Self::CloudProfile => "Perfil de nuvem",
            Self::WebSession => "Sessão web",
            Self::Extension => "Extensão",
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
            Self::Subscription => "Assinatura",
            Self::ApiPlatform => "Plataforma de API",
            Self::Cloud => "Nuvem",
            Self::CodingTool => "Ferramenta de código",
            Self::Router => "Roteador",
            Self::Extension => "Extensão",
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
        Provider::ElevenLabs => &["eleven labs", "11labs"],
        Provider::NineRouter => &["9router", "nine router", "router", "aggregator"],
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
        | Provider::ElevenLabs => Category::ApiPlatform,
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
        Provider::NineRouter => Category::Router,
        Provider::Extension => Category::Extension,
    }
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
    let hint =
        |slug: &str, fallback: &str| Localized::fixed(format!("catalog.hint.{slug}"), fallback);
    if level == Level::Extension {
        return (SignInMethod::Extension, hint("extension", "Extensão local"));
    }
    match credential {
        Credential::InferenceKey => (SignInMethod::ApiKey, hint("api_key", "Chave de API")),
        Credential::AdminKey => (
            SignInMethod::ApiKey,
            hint("admin_key", "Chave de API de administração"),
        ),
        Credential::SubscriptionSession if native_login => (
            SignInMethod::BrowserSignIn,
            hint("browser_sign_in", "Entre pelo navegador"),
        ),
        Credential::SubscriptionSession => (
            SignInMethod::ApiKey,
            hint("access_token", "Token de acesso da conta"),
        ),
        Credential::CloudIdentity if metered => (
            SignInMethod::CloudProfile,
            hint(
                "cloud_metered",
                "Credenciais de nuvem · cobrado por consulta",
            ),
        ),
        Credential::CloudIdentity => (
            SignInMethod::CloudProfile,
            hint("cloud", "Credenciais de nuvem"),
        ),
        Credential::LocalCli => {
            let cli = cli_name(provider);
            (
                SignInMethod::LocalCredentials,
                Localized::with_param(
                    "catalog.hint.local_cli",
                    "cli",
                    cli,
                    format!("Usa a CLI {cli}"),
                ),
            )
        }
        Credential::UserSession => (
            SignInMethod::WebSession,
            hint("web_session", "Cole uma sessão web"),
        ),
        Credential::AggregatorLogin => (
            SignInMethod::ApiKey,
            hint("aggregator_login", "Senha do painel (opcional)"),
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
            slug: fold(entry.provider.slug()),
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
    let entries = &*CATALOG;
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
        let provider = entries[index].provider;
        let names_mode = hits
            .iter()
            .any(|(i, _, specific)| entries[*i].provider == provider && *specific);
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
    use crate::text::core_catalog;

    fn names(query: &str, locale: &str) -> Vec<&'static str> {
        search(query, locale)
            .into_iter()
            .map(|m| m.entry.provider.slug())
            .collect()
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
        // The Portuguese fallback is the embedded text.
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
        for locale in ["en-US", "es", "it"] {
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
