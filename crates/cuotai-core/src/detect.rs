//! PRD 0027 RF6 and RF7: presence-only detection of AI tools already used on
//! this device, offered to the Add-account picker.
//!
//! Detection only asks whether a known file or directory exists
//! (`symlink_metadata`); it never opens, reads or lists anything, and it
//! returns no path, size or timestamp. Frontends call it only after the
//! person agreed to the check (RF6.5) and only when the picker opens
//! (RF6.2). A detected source only *offers* the provider's existing explicit
//! flow; [`Detected::consent`] names what that flow will read (RF7.2).

use crate::text::Localized;
use crate::Provider;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// The operating system whose path conventions apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Windows,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}

/// A local tool whose presence can be checked (RF6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    CursorApp,
    ClaudeCode,
    CodexCli,
    AwsCli,
    GeminiCli,
    CopilotCli,
    OpenCode,
    CursorCli,
    QwenCode,
    /// Kiro's global directory, shared by the Kiro CLI and the Kiro IDE
    /// (the slug keeps its first name).
    KiroCli,
    KimiCli,
    GrokCli,
    Zed,
    /// PRD 0038 Q5: `OPENROUTER_API_KEY` set in the environment (presence
    /// only, [`crate::catalog::detect_env_keys`]).
    OpenRouterKey,
    /// PRD 0036 RF2.5: a 9router instance answering on its default local
    /// port (a network presence probe, see [`probe_nine_router`]).
    NineRouterLocal,
}

impl SourceKind {
    pub fn slug(self) -> &'static str {
        match self {
            Self::CursorApp => "cursor_app",
            Self::ClaudeCode => "claude_code",
            Self::CodexCli => "codex_cli",
            Self::AwsCli => "aws_cli",
            Self::GeminiCli => "gemini_cli",
            Self::CopilotCli => "copilot_cli",
            Self::OpenCode => "open_code",
            Self::CursorCli => "cursor_cli",
            Self::QwenCode => "qwen_code",
            Self::KiroCli => "kiro_cli",
            Self::KimiCli => "kimi_cli",
            Self::GrokCli => "grok_cli",
            Self::Zed => "zed",
            Self::OpenRouterKey => "open_router_key",
            Self::NineRouterLocal => "nine_router_local",
        }
    }

    pub fn from_slug(slug: &str) -> Option<Self> {
        TABLE
            .iter()
            .map(|entry| entry.kind)
            .chain([Self::OpenRouterKey, Self::NineRouterLocal])
            .find(|kind| kind.slug() == slug)
    }

    /// Display name of the tool (product names, not translated).
    fn tool(self) -> &'static str {
        match self {
            Self::CursorApp => "Cursor",
            Self::ClaudeCode => "Claude Code",
            Self::CodexCli => "Codex CLI",
            Self::AwsCli => "AWS CLI",
            Self::GeminiCli => "Gemini CLI",
            Self::CopilotCli => "GitHub Copilot CLI",
            Self::OpenCode => "OpenCode",
            Self::CursorCli => "Cursor CLI",
            Self::QwenCode => "Qwen Code",
            Self::KiroCli => "Kiro",
            Self::KimiCli => "Kimi Code CLI",
            Self::GrokCli => "Grok CLI",
            Self::Zed => "Zed",
            Self::OpenRouterKey => "OpenRouter",
            Self::NineRouterLocal => "9router",
        }
    }
}

/// What the offered flow will read once the person confirms (RF7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    /// `oauth::cursor_import`: reads the token from Cursor's database once.
    CursorImport,
    /// The provider's own browser or device sign-in; nothing local is read.
    NativeLogin,
    /// The AWS Bedrock profile step: reads profile names, never keys.
    AwsProfile,
    /// A key the person pastes; nothing local is read.
    KeyStep,
}

/// One reviewed row of the RF6.4 table.
#[derive(Debug, Clone, Copy)]
pub struct DetectEntry {
    pub kind: SourceKind,
    pub provider: Provider,
    /// Public reference for the checked path.
    pub evidence: &'static str,
    /// `false` for "to verify" rows: they are never checked (RF6.1).
    pub verified: bool,
    flow: Flow,
}

const fn row(
    kind: SourceKind,
    provider: Provider,
    flow: Flow,
    evidence: &'static str,
    verified: bool,
) -> DetectEntry {
    DetectEntry {
        kind,
        provider,
        evidence,
        verified,
        flow,
    }
}

/// RF6.4: the one reviewed detection table.
pub const TABLE: &[DetectEntry] = &[
    row(SourceKind::CursorApp, Provider::Cursor, Flow::CursorImport,
        "oauth::cursor_import::candidates (Cursor state.vscdb)", true),
    row(SourceKind::ClaudeCode, Provider::Claude, Flow::NativeLogin,
        "https://docs.anthropic.com/en/docs/claude-code/settings", true),
    row(SourceKind::CodexCli, Provider::ChatGpt, Flow::NativeLogin,
        "https://developers.openai.com/codex/auth", true),
    row(SourceKind::AwsCli, Provider::AwsBedrock, Flow::AwsProfile,
        "https://docs.aws.amazon.com/cli/latest/userguide/cli-configure-files.html", true),
    row(SourceKind::GeminiCli, Provider::GeminiAi, Flow::NativeLogin,
        "https://github.com/google-gemini/gemini-cli/blob/main/docs/get-started/authentication.md", true),
    row(SourceKind::CopilotCli, Provider::Copilot, Flow::NativeLogin,
        "https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-config-dir-reference", true),
    row(SourceKind::OpenCode, Provider::OpenCodeGo, Flow::KeyStep,
        "https://opencode.ai/docs/troubleshooting", true),
    row(SourceKind::CursorCli, Provider::Cursor, Flow::NativeLogin,
        "https://docs.cursor.com/en/cli/reference/configuration", true),
    // Verified 2026-10-25: user settings at `~/.qwen/settings.json`.
    row(SourceKind::QwenCode, Provider::Qwen, Flow::NativeLogin,
        "https://qwenlm.github.io/qwen-code-docs/en/users/configuration/settings/", true),
    // Verified 2026-10-25: global scope `~/.kiro/` (IDE and CLI), moved by
    // an absolute `KIRO_HOME`.
    row(SourceKind::KiroCli, Provider::Kiro, Flow::NativeLogin,
        "https://kiro.dev/docs/configuration/", true),
    // Verified 2026-10-25: "The default configuration file is located at
    // ~/.kimi/config.toml".
    row(SourceKind::KimiCli, Provider::Kimi, Flow::NativeLogin,
        "https://moonshotai.github.io/kimi-cli/en/configuration/config-files.html", true),
    // Verified 2026-10-25: Grok Build's home is `$GROK_HOME`, default
    // `~/.grok` (`config.toml` inside).
    row(SourceKind::GrokCli, Provider::Grok, Flow::NativeLogin,
        "https://docs.x.ai/build/settings/reference", true),
    // Verified 2026-10-25 for macOS and Linux only: the docs show
    // `~/.config/zed/settings.json`; no Windows path is documented.
    row(SourceKind::Zed, Provider::Zed, Flow::NativeLogin,
        "https://zed.dev/docs/configuring-zed", true),
];

/// One detected source. Carries no path, size, time or content (RF6.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detected {
    pub provider: Provider,
    pub kind: SourceKind,
    /// The `auth_mode` the offer opens.
    pub mode: Option<&'static str>,
    /// "Cursor is installed on this device".
    pub hint: Localized,
    /// What the offered flow reads once confirmed, and why (RF7.2).
    pub consent: Localized,
}

/// Environment lookups the path table needs (`CODEX_HOME`, `XDG_*`,
/// `APPDATA`), injected so tests never touch the real environment.
pub trait Env {
    fn var(&self, name: &str) -> Option<String>;
}

/// The process environment.
pub struct ProcessEnv;

impl Env for ProcessEnv {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name)
            .ok()
            .filter(|value| !value.trim().is_empty())
    }
}

impl<F: Fn(&str) -> Option<String>> Env for F {
    fn var(&self, name: &str) -> Option<String> {
        self(name)
    }
}

/// The candidate paths of one source on `os` (never touched here).
pub fn paths(kind: SourceKind, home: &Path, os: Os, env: &dyn Env) -> Vec<PathBuf> {
    let config_dir = || match os {
        Os::MacOs => home.join("Library").join("Application Support"),
        Os::Windows => env
            .var("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData").join("Roaming")),
        Os::Linux => env
            .var("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config")),
    };
    match kind {
        // An environment variable, not a file: nothing to look for on disk.
        SourceKind::OpenRouterKey => Vec::new(),
        SourceKind::CursorApp => ["Cursor", "Cursor - Insiders"]
            .iter()
            .map(|profile| {
                config_dir()
                    .join(profile)
                    .join("User")
                    .join("globalStorage")
                    .join("state.vscdb")
            })
            .collect(),
        SourceKind::ClaudeCode => vec![home.join(".claude")],
        SourceKind::CodexCli => vec![env
            .var("CODEX_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".codex"))],
        SourceKind::AwsCli => vec![home.join(".aws").join("config")],
        SourceKind::GeminiCli => vec![home.join(".gemini")],
        SourceKind::CopilotCli => vec![home.join(".copilot")],
        SourceKind::OpenCode => vec![env
            .var("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local").join("share"))
            .join("opencode")],
        SourceKind::CursorCli => vec![home.join(".cursor").join("cli-config.json")],
        SourceKind::QwenCode => vec![home.join(".qwen").join("settings.json")],
        SourceKind::KimiCli => vec![home.join(".kimi").join("config.toml")],
        SourceKind::GrokCli => vec![env
            .var("GROK_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".grok"))
            .join("config.toml")],
        SourceKind::Zed => match os {
            Os::Windows => Vec::new(),
            Os::MacOs | Os::Linux => vec![home.join(".config").join("zed").join("settings.json")],
        },
        // A network probe, never a path (`probe_nine_router`).
        SourceKind::NineRouterLocal => Vec::new(),
        // Kiro ignores a relative `KIRO_HOME` (and a few others); only an
        // absolute one is followed here.
        SourceKind::KiroCli => vec![env
            .var("KIRO_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".kiro"))],
    }
}

/// Presence only: `symlink_metadata` never opens the file.
fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// PRD 0027 RF6.5: the one "Check this device" consent. It names every
/// check the scan makes: tool folders, environment variables (presence
/// only, never values) and the local 9router health probe on 127.0.0.1.
pub fn consent_prompt() -> Localized {
    Localized::fixed(
        "detect.ask",
        "Check this device for AI tools you already use? Cuotai checks whether their folders exist, whether certain environment variables are set (presence only, never their values) and whether 9router answers locally on 127.0.0.1; it reads nothing.",
    )
}

fn texts(entry: &DetectEntry) -> (Localized, Localized) {
    let tool = entry.kind.tool();
    let provider = entry.provider.label();
    let hint = Localized::with_param(
        "detect.hint",
        "tool",
        tool,
        "{tool} is installed on this device",
    );
    let consent = match entry.flow {
        Flow::CursorImport => Localized::fixed(
            "detect.consent.cursor_import",
            "Cuotai will read the sign-in token from Cursor's local database once, to query your usage. It is stored in your system's credential vault.",
        ),
        Flow::NativeLogin => Localized::with_params(
            "detect.consent.native_login",
            crate::text::params([("provider", provider.into()), ("tool", tool.into())]),
            "Cuotai will open the {provider} sign-in; nothing is read from {tool}'s folder.",
        ),
        Flow::AwsProfile => Localized::fixed(
            "detect.consent.aws_profile",
            "Cuotai will read only the profile names in ~/.aws/config when you choose the profile; no key is read.",
        ),
        Flow::KeyStep => Localized::with_param(
            "detect.consent.key_step",
            "tool",
            tool,
            "Paste your key; Cuotai reads nothing from {tool}'s folder.",
        ),
    };
    (hint, consent)
}

/// RF6.1–RF6.3: the verified sources present under `home`, skipping
/// providers in `configured` (they already have an account) and providers
/// the picker does not offer. Order follows [`TABLE`]; one result per
/// provider (the first present source).
pub fn local_sources(home: &Path, os: Os, env: &dyn Env, configured: &[Provider]) -> Vec<Detected> {
    let mut out: Vec<Detected> = Vec::new();
    for entry in TABLE.iter().filter(|entry| entry.verified) {
        if configured.contains(&entry.provider)
            || out.iter().any(|d| d.provider == entry.provider)
            || !crate::access::has_usage_query(entry.provider)
        {
            continue;
        }
        if paths(entry.kind, home, os, env).iter().any(|p| exists(p)) {
            let (hint, consent) = texts(entry);
            let login = matches!(entry.flow, Flow::NativeLogin | Flow::CursorImport);
            let mode = crate::catalog::catalog()
                .into_iter()
                .find(|e| e.provider == entry.provider && e.native_login == login)
                .and_then(|e| e.mode);
            out.push(Detected {
                provider: entry.provider,
                kind: entry.kind,
                mode,
                hint,
                consent,
            });
        }
    }
    // PRD 0038 Q5 with PRD 0027 RF6: the same consent covers known API-key
    // variables, checked for presence only (the value is never read or
    // kept); the account then names the variable.
    let is_set = |name: &str| env.var(name).is_some_and(|value| !value.is_empty());
    for found in crate::catalog::detect_env_keys(true, is_set) {
        if configured.contains(&found.provider) || out.iter().any(|d| d.provider == found.provider)
        {
            continue;
        }
        let variable = found.variable;
        out.push(Detected {
            provider: found.provider,
            kind: SourceKind::OpenRouterKey,
            mode: Some("credential"),
            hint: Localized::with_param(
                "detect.hint.env",
                "variable",
                variable,
                "{variable} is set in this environment",
            ),
            consent: Localized::with_param(
                "detect.consent.env",
                "variable",
                variable,
                "The account will keep only the name {variable}; the value is never read or saved.",
            ),
        });
    }
    out
}

/// PRD 0036 RF2.5: the one address the presence probe may ask.
pub const NINE_ROUTER_LOCAL: &str = "http://127.0.0.1:20128";
/// RF2.5: the probe's whole budget.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);

/// PRD 0036 RF2.5: with the device-check consent (PRD 0027 RF6.5), whether
/// a 9router instance answers `GET http://127.0.0.1:20128/api/health` with
/// `{"ok": true}` within 200 ms. One public request, no credential, no
/// redirect, no proxy, no other port and no file of 9router is read.
/// `None` when 9router is already configured or nothing answers.
pub async fn probe_nine_router(configured: &[Provider]) -> Option<Detected> {
    probe_nine_router_at(NINE_ROUTER_LOCAL, configured).await
}

async fn probe_nine_router_at(base: &str, configured: &[Provider]) -> Option<Detected> {
    if configured.contains(&Provider::NineRouter) {
        return None;
    }
    let http = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PROBE_TIMEOUT)
        .build()
        .ok()?;
    let response = http.get(format!("{base}/api/health")).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: serde_json::Value = response.json().await.ok()?;
    if body.get("ok") != Some(&serde_json::Value::Bool(true)) {
        return None;
    }
    Some(Detected {
        provider: Provider::NineRouter,
        kind: SourceKind::NineRouterLocal,
        mode: None,
        hint: Localized::fixed(
            "detect.hint.nine_router_local",
            "Found on this device: 9router answers on 127.0.0.1:20128",
        ),
        consent: Localized::fixed(
            "detect.consent.nine_router_local",
            "The address http://127.0.0.1:20128 is filled in. Only 9router's public health route was asked; nothing was read from it.",
        ),
    })
}

/// [`local_sources`] for the current user and OS.
pub fn local_sources_here(configured: &[Provider]) -> Vec<Detected> {
    match dirs::home_dir() {
        Some(home) => local_sources(&home, Os::current(), &ProcessEnv, configured),
        None => Vec::new(),
    }
}

#[cfg(test)]
mod probe_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn health(body: serde_json::Value) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/health"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn a_local_instance_is_found_only_when_it_says_ok() {
        let server = health(serde_json::json!({"ok": true})).await;
        let found = probe_nine_router_at(&server.uri(), &[]).await.unwrap();
        assert_eq!(found.provider, Provider::NineRouter);
        assert_eq!(found.kind.slug(), "nine_router_local");
        assert_eq!(SourceKind::from_slug("nine_router_local"), Some(found.kind));
        // Already configured: no request at all.
        assert!(probe_nine_router_at(&server.uri(), &[Provider::NineRouter])
            .await
            .is_none());
        assert_eq!(server.received_requests().await.unwrap().len(), 1);
        let other = health(serde_json::json!({"status": "up"})).await;
        assert!(probe_nine_router_at(&other.uri(), &[]).await.is_none());
        // Nothing listening.
        assert!(probe_nine_router_at("http://127.0.0.1:9", &[])
            .await
            .is_none());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// PRD 0027 RNF: the presence check finishes in under 50 ms (the
    /// network probe of 9router is separate and has its own timeout).
    #[test]
    fn detection_finishes_within_the_budget() {
        let home = tempfile::tempdir().unwrap();
        for dir in [".claude", ".codex", ".aws", ".gemini", ".kiro"] {
            std::fs::create_dir_all(home.path().join(dir)).unwrap();
        }
        for os in [Os::Linux, Os::MacOs, Os::Windows] {
            crate::assert_within_budget(
                "detect::local_sources",
                std::time::Duration::from_millis(50),
                3,
                || {
                    let _ = local_sources(home.path(), os, &no_env, &[]);
                },
            );
        }
    }

    /// RF6.5 (owner decision 2026-10-25): the consent names folders,
    /// environment variables (presence only) and the 127.0.0.1 9router check
    /// in every catalog.
    #[test]
    fn the_consent_names_every_check_in_every_catalog() {
        let english = consent_prompt();
        assert_eq!(english.id, "detect.ask");
        let catalogs = [
            ("en", english.text().to_string()),
            ("pt-BR", include_str!("../locales/pt-BR.json").to_string()),
            ("es", include_str!("../locales/es.json").to_string()),
            ("it", include_str!("../locales/it.json").to_string()),
        ];
        for (locale, text) in catalogs {
            let text = if locale == "en" {
                text
            } else {
                let map: serde_json::Value = serde_json::from_str(&text).unwrap();
                map["detect.ask"].as_str().unwrap().to_string()
            };
            assert!(text.contains("127.0.0.1"), "{locale}");
            assert!(text.contains("9router"), "{locale}");
            assert!(
                text.contains("env") || text.contains("ambient") || text.contains("entorno"),
                "{locale}"
            );
        }
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, b"secret").unwrap();
    }

    #[test]
    fn every_row_has_evidence_and_unverified_rows_are_never_checked() {
        for entry in TABLE {
            assert!(!entry.evidence.trim().is_empty(), "{:?}", entry.kind);
        }
        assert!(TABLE.iter().all(|entry| !entry.verified
            || entry.evidence.starts_with("https://")
            || entry.kind == SourceKind::CursorApp));
        // Locations only third-party sources named (2026-10-23) stay
        // unchecked: neither is a documented path.
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".qwen")).unwrap();
        std::fs::create_dir_all(home.path().join(".local/share/kiro-cli")).unwrap();
        touch(&home.path().join(".aws/sso/cache/kiro-auth-token.json"));
        let found = local_sources(home.path(), Os::Linux, &no_env, &[]);
        assert!(
            found.iter().all(|d| d.provider == Provider::AwsBedrock),
            "{found:?}"
        );
    }

    /// PRD 0027 RF6.4: the rows verified on 2026-10-25 against each tool's
    /// own documentation.
    #[test]
    fn documented_paths_of_qwen_kiro_kimi_grok_and_zed() {
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join(".qwen/settings.json"));
        std::fs::create_dir_all(home.path().join(".kiro")).unwrap();
        touch(&home.path().join(".kimi/config.toml"));
        touch(&home.path().join(".grok/config.toml"));
        touch(&home.path().join(".config/zed/settings.json"));
        let kinds: Vec<_> = local_sources(home.path(), Os::Linux, &no_env, &[])
            .into_iter()
            .map(|d| d.kind)
            .collect();
        for kind in [
            SourceKind::QwenCode,
            SourceKind::KiroCli,
            SourceKind::KimiCli,
            SourceKind::GrokCli,
            SourceKind::Zed,
        ] {
            let offered = crate::access::has_usage_query(
                TABLE.iter().find(|e| e.kind == kind).unwrap().provider,
            );
            assert_eq!(kinds.contains(&kind), offered, "{kind:?}");
        }
        let h = Path::new("/h");
        assert!(paths(SourceKind::Zed, h, Os::Windows, &no_env).is_empty());
        let grok = |name: &str| (name == "GROK_HOME").then(|| "/g".to_string());
        assert_eq!(
            paths(SourceKind::GrokCli, h, Os::Linux, &grok),
            vec![PathBuf::from("/g/config.toml")]
        );
        // A relative KIRO_HOME is ignored, as Kiro itself does.
        let relative = |name: &str| (name == "KIRO_HOME").then(|| "kiro".to_string());
        assert_eq!(
            paths(SourceKind::KiroCli, h, Os::Linux, &relative),
            vec![PathBuf::from("/h/.kiro")]
        );
        for kind in [SourceKind::KimiCli, SourceKind::GrokCli, SourceKind::Zed] {
            assert_eq!(SourceKind::from_slug(kind.slug()), Some(kind));
        }
    }

    #[test]
    fn empty_home_detects_nothing() {
        let home = tempfile::tempdir().unwrap();
        for os in [Os::MacOs, Os::Linux, Os::Windows] {
            assert!(local_sources(home.path(), os, &no_env, &[]).is_empty());
        }
    }

    #[test]
    fn detects_directories_and_skips_configured_providers() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".claude")).unwrap();
        std::fs::create_dir_all(home.path().join(".codex")).unwrap();
        touch(&home.path().join(".aws/config"));
        let found = local_sources(home.path(), Os::Linux, &no_env, &[Provider::ChatGpt]);
        let providers: Vec<_> = found.iter().map(|d| d.provider).collect();
        assert_eq!(providers, vec![Provider::Claude, Provider::AwsBedrock]);
        assert_eq!(found[0].mode, Some("oauth"));
        // RF6.3: no path or user name in what is returned.
        let json = serde_json::to_string(&found).unwrap();
        assert!(!json.contains(&home.path().display().to_string()));
    }

    #[test]
    fn cursor_paths_per_os() {
        let home = Path::new("/h");
        let mac = paths(SourceKind::CursorApp, home, Os::MacOs, &no_env);
        assert_eq!(
            mac[0],
            Path::new("/h/Library/Application Support/Cursor/User/globalStorage/state.vscdb")
        );
        let linux = paths(SourceKind::CursorApp, home, Os::Linux, &no_env);
        assert_eq!(
            linux[0],
            Path::new("/h/.config/Cursor/User/globalStorage/state.vscdb")
        );
        let appdata = |name: &str| (name == "APPDATA").then(|| "/appdata".to_string());
        let win = paths(SourceKind::CursorApp, home, Os::Windows, &appdata);
        assert_eq!(
            win[0],
            Path::new("/appdata/Cursor/User/globalStorage/state.vscdb")
        );
        let codex = |name: &str| (name == "CODEX_HOME").then(|| "/codex".to_string());
        assert_eq!(
            paths(SourceKind::CodexCli, home, Os::Linux, &codex),
            vec![PathBuf::from("/codex")]
        );
    }

    #[test]
    fn one_offer_per_provider() {
        let home = tempfile::tempdir().unwrap();
        touch(
            &home
                .path()
                .join(".config/Cursor/User/globalStorage/state.vscdb"),
        );
        touch(&home.path().join(".cursor/cli-config.json"));
        let found = local_sources(home.path(), Os::Linux, &no_env, &[]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, SourceKind::CursorApp);
    }

    /// Presence is detected without opening: an unreadable file still counts.
    #[cfg(unix)]
    #[test]
    fn unreadable_files_are_detected_without_opening() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let config = home.path().join(".aws/config");
        touch(&config);
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o000)).unwrap();
        let found = local_sources(home.path(), Os::MacOs, &no_env, &[]);
        std::fs::set_permissions(&config, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, SourceKind::AwsCli);
    }

    #[test]
    fn slugs_round_trip() {
        for entry in TABLE {
            assert_eq!(SourceKind::from_slug(entry.kind.slug()), Some(entry.kind));
        }
    }

    /// PRD 0038 Q5: after consent, a set `OPENROUTER_API_KEY` is offered
    /// as an OpenRouter key account (presence only), unless one exists.
    #[test]
    fn a_set_openrouter_variable_is_offered_once() {
        struct Set;
        impl Env for Set {
            fn var(&self, name: &str) -> Option<String> {
                (name == "OPENROUTER_API_KEY").then(|| "sk-or-v1-secret".to_string())
            }
        }
        let home = tempfile::tempdir().unwrap();
        let found = local_sources(home.path(), Os::Linux, &Set, &[]);
        let offer = found
            .iter()
            .find(|d| d.provider == Provider::OpenRouter)
            .expect("offered");
        assert_eq!(offer.kind, SourceKind::OpenRouterKey);
        assert_eq!(offer.mode, Some("credential"));
        assert!(!format!("{offer:?}").contains("sk-or-v1-secret"));
        assert_eq!(
            SourceKind::from_slug("open_router_key"),
            Some(SourceKind::OpenRouterKey)
        );
        assert!(
            local_sources(home.path(), Os::Linux, &Set, &[Provider::OpenRouter])
                .iter()
                .all(|d| d.provider != Provider::OpenRouter)
        );
    }
}
