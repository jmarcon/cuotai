//! PRD 0035 RF11 (re-validation V35-02 to V35-09, V35-11): the register
//! table, the glossary and the punctuation rules of
//! `apps/desktop/ui/locales/README.md`, enforced on the built-in catalogs of
//! the three layers (desktop UI, core, TUI).
//!
//! The tests are grouped by delivery commit: `// G10` register, `// G11`
//! glossary ratchets, `// G12` punctuation. A G11 ratchet freezes the ids
//! whose text still uses the competing term on purpose: a new use fails
//! until it is reviewed and added, and a fixed one fails until it is
//! removed, so each list only shrinks.

use super::Catalog;
use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;

/// The parsed catalog of `code` in `layer` (`ui`, `core` or `tui`), when
/// that layer has one.
fn layer_catalog(layer: &str, code: &str) -> Option<Catalog> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let dir = match layer {
        "ui" => root.join("../../apps/desktop/ui/locales"),
        "core" => root.join("locales"),
        "tui" => root.join("../cuotai-tui/locales"),
        other => panic!("unknown layer {other}"),
    };
    let raw = std::fs::read_to_string(dir.join(format!("{code}.json"))).ok()?;
    Some(Catalog::parse(&raw).unwrap_or_else(|error| panic!("{layer}/{code}.json: {error}")))
}

/// Every built-in text of `code`, as (`layer:id`, template); a plural gives
/// one entry per form.
fn texts(code: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for layer in ["ui", "core", "tui"] {
        let Some(catalog) = layer_catalog(layer, code) else {
            continue;
        };
        for (id, message) in catalog.messages() {
            for template in message.templates() {
                out.push((format!("{layer}:{id}"), template.to_owned()));
            }
        }
    }
    assert!(!out.is_empty(), "no catalog found for {code}");
    out
}

/// The `layer:id`s of `code` whose text matches `hit`, each once.
fn offending_ids(code: &str, hit: impl Fn(&str, &str) -> bool) -> BTreeSet<String> {
    texts(code)
        .into_iter()
        .filter(|(id, text)| hit(id, text))
        .map(|(id, _)| id)
        .collect()
}

/// `text` with every `{placeholder}` replaced by `with`.
fn without_placeholders(text: &str, with: &str) -> String {
    Regex::new(r"\{[A-Za-z0-9_]*\}")
        .unwrap()
        .replace_all(text, with)
        .into_owned()
}

// G10 ---------------------------------------------------------------------

// G10
/// V35-02: German uses "Sie" (README register table), never du/dich/dir/dein….
/// The possessive is spelled out (`dein`, `deine`, `deinen`, …) so that
/// words such as "deinstallieren" are not taken for it.
#[test]
fn de_uses_the_sie_form() {
    let du = Regex::new(r"(?i)\b(du|dich|dir|dein(e[nmrs]?)?)\b").unwrap();
    let bad = offending_ids("de", |_, text| {
        du.is_match(&without_placeholders(text, " "))
    });
    assert!(bad.is_empty(), "de texts use du instead of Sie: {bad:#?}");
}

/// German du-imperatives that `de_uses_the_sie_form` cannot see, because
/// they carry no pronoun ("Gib den Schlüssel ein"). The Sie form names the
/// verb in its infinitive ("Geben Sie"), so these forms never start a Sie
/// imperative.
const DE_DU_IMPERATIVES: &[&str] = &[
    "Gib",
    "Wähle",
    "Öffne",
    "Starte",
    "Führe",
    "Prüfe",
    "Korrigiere",
    "Entferne",
    "Setze",
    "Schreibe",
    "Tippe",
    "Füge",
    "Schalte",
    "Lösche",
    "Installiere",
    "Melde",
    "Beende",
    "Aktualisiere",
    "Erlaube",
    "Entsperre",
    "Verschiebe",
    "Erstelle",
    "Ignoriere",
    "Speichere",
    "Repariere",
    "Sende",
    "Lass",
    "Bestätige",
    "Versuche",
    "Verwende",
];

/// The de ids that still use a du-imperative (PRD 0046 RF4.2, PRD 0035
/// RF11.5). They lie outside the backup, restore and setup-check surfaces
/// that PRD 0046 moved to "Sie"; their PRDs' owners move them later. The
/// list only shrinks: a moved id fails until it is removed here.
const DE_DU_IMPERATIVE_RATCHET: &[&str] = &[
    "core:budget.error.amount_ambiguous",
    "core:budget.error.characters",
    "core:budget.error.selector",
    "core:budget.error.unit_mismatch",
    "core:config.account.requires_newer",
    "core:cost.view.unreadable",
    "core:edit_error.config_inline_accounts",
    "core:edit_error.config_newer_accounts",
    "core:feed.banner.claude_code",
    "core:feed.banner.codex_cli",
    "core:history.error.damaged",
    "core:history.error.erase_incomplete",
    "core:history.error.export_target",
    "core:note.claude_code.no_feed",
    "core:note.codex_cli.no_feed",
    "core:secret_input.duplicate_slot",
    "core:secret_input.help",
    "core:secret_input.unknown_slot",
    "core:state.error.newer_major",
    "core:state.error.newer_major_history",
    "tui:canary.foreign_folder",
    "tui:history_actions.path_empty",
    "tui:report.healthy",
    "tui:report.open_link",
    "ui:coreText.doctorNoKey",
    "ui:coreText.toolMissing",
    "ui:dialog.monogram.invalid",
    "ui:settings.dataDir.pending",
    "ui:settings.erase.partial",
    "ui:settings.erase.word",
    "ui:settings.error.busy",
    "ui:settings.error.newer",
    "ui:settings.insights.digestTestOff",
];

/// Whether `text` holds a du-imperative from [`DE_DU_IMPERATIVES`] at the
/// start of a sentence or clause (after `.`, `!`, `?`, `:` or `;`) or right
/// after `und`/`oder`, not followed by " Sie".
fn has_du_imperative(text: &str) -> bool {
    let pattern = format!(
        r"(?i)(?:^|[.!?:;]\s+|\b(?:und|oder)\s+)(?:{})\b",
        DE_DU_IMPERATIVES.join("|")
    );
    let verb = Regex::new(&pattern).unwrap();
    let text = without_placeholders(text, " ");
    let found = verb
        .find_iter(&text)
        .any(|found| !text[found.end()..].starts_with(" Sie"));
    found
}

// G10
/// V35-02 (PRD 0046 V-3): German instructions use the Sie imperative
/// ("Geben Sie …"), never the du imperative ("Gib …"), except the ids of
/// [`DE_DU_IMPERATIVE_RATCHET`].
#[test]
fn de_uses_the_sie_imperative() {
    let found = offending_ids("de", |_, text| has_du_imperative(text));
    let allowed: BTreeSet<String> = DE_DU_IMPERATIVE_RATCHET
        .iter()
        .map(|id| id.to_string())
        .collect();
    let new: Vec<_> = found.difference(&allowed).collect();
    let gone: Vec<_> = allowed.difference(&found).collect();
    assert!(
        new.is_empty(),
        "de texts use a du-imperative; write the Sie form (\"Geben Sie …\"): {new:#?}"
    );
    assert!(
        gone.is_empty(),
        "these de ids no longer use a du-imperative; remove them from \
         DE_DU_IMPERATIVE_RATCHET: {gone:#?}"
    );
}

// G10
/// The du-imperative rule finds the forms it names and leaves the Sie form,
/// conjugated verbs and longer words alone.
#[test]
fn du_imperative_detection() {
    assert!(has_du_imperative("Gib den Schlüssel erneut ein."));
    assert!(has_du_imperative("Datei geändert; starte sie erneut."));
    assert!(has_du_imperative("Prüfe die Adresse und sende einen Test."));
    assert!(has_du_imperative("{field}: Setze kind."));
    assert!(!has_du_imperative("Geben Sie den Schlüssel erneut ein."));
    assert!(!has_du_imperative("Starten Sie Cuotai neu."));
    assert!(!has_du_imperative("Cuotai gibt nichts weiter."));
    assert!(!has_du_imperative("Die Startseite öffnet sich."));
    assert!(!has_du_imperative("Drei Versuche. Gibt es mehr?"));
}

// G10
/// V35-03: Simplified Chinese drops the pronoun; 您 only in error, consent,
/// privacy and warning texts, never 你.
#[test]
fn zh_cn_never_uses_ni() {
    let bad = offending_ids("zh-CN", |_, text| text.contains('你'));
    assert!(bad.is_empty(), "zh-CN texts use 你: {bad:#?}");
}

// G10
/// V35-04: Japanese never writes あなた (README: use 現在のユーザー for
/// "your user").
#[test]
fn ja_never_uses_anata() {
    let bad = offending_ids("ja", |_, text| text.contains("あなた"));
    assert!(bad.is_empty(), "ja texts use あなた: {bad:#?}");
}

// G10
/// V35-05: Korean is written in 합쇼체 (confirmation titles end in
/// …하시겠습니까?), never a 해요체 sentence end. The polite imperative -세요
/// is allowed by the README.
#[test]
fn ko_has_no_haeyo_sentence_ends() {
    let haeyo = Regex::new(r#"(까요|해요|어요|아요|네요|죠)[.?!]*(\s|$|["'”’)\]])"#).unwrap();
    let bad = offending_ids("ko", |_, text| haeyo.is_match(text));
    assert!(
        bad.is_empty(),
        "ko texts end a sentence in 해요체: {bad:#?}"
    );
}

// G11 ---------------------------------------------------------------------

/// The ratchet of one glossary term: the ids of `code` whose text matches
/// `term` (among those `scope` keeps) are exactly `allowed`.
fn glossary_ratchet(code: &str, term: &str, scope: impl Fn(&str) -> bool, allowed: &[&str]) {
    let term = Regex::new(term).unwrap();
    let found = offending_ids(code, |id, text| scope(id) && term.is_match(text));
    let allowed: BTreeSet<String> = allowed.iter().map(|id| id.to_string()).collect();
    let new: Vec<_> = found.difference(&allowed).collect();
    let gone: Vec<_> = allowed.difference(&found).collect();
    assert!(
        new.is_empty(),
        "{code}: these texts use a term the README glossary replaces ({term}); \
         use the glossary term, or add the id here once reviewed: {new:#?}"
    );
    assert!(
        gone.is_empty(),
        "{code}: these ids no longer use {term}; remove them from the list: {gone:#?}"
    );
}

// G11
/// ru ids that keep аккаунт on purpose (none).
const RU_AKKAUNT: &[&str] = &[];

// G11
/// ko ids that keep 공급자/제공자 on purpose: the D-Bus "Secret Service
/// provider" is not an AI provider.
const KO_PROVIDER: &[&str] = &["core:oauth_error.store.linux_secret_service_unavailable"];

// G11
/// zh-CN ids that keep 服务商 on purpose (none).
const ZH_CN_FUWUSHANG: &[&str] = &[];

// G11
/// de ids that keep "Reset" on purpose (none).
const DE_RESET: &[&str] = &[];

// G11
/// hi-IN ids that keep लॉगिन/लॉग इन on purpose: their English says "login"
/// (Native login, `bl auth login`, the dashboard login of 9router, the
/// autostart keywords), not "sign in".
const HI_IN_LOGIN: &[&str] = &[
    "core:access.credential.aggregator_login",
    "core:access.credential_hint.aggregator_login",
    "core:access.credential_hint.subscription_session",
    "core:access.summary.alibaba_model_studio_cli",
    "core:auth_mode.oauth.label",
    "core:credential_help.alibaba_model_studio",
    "core:credential_help.antigravity",
    "core:credential_help.kiro",
    "core:credential_help.nine_router",
    "core:credential_help.zed",
    "core:edit_error.login_partial_failure",
    "core:edit_error.oauth_required",
    "core:field.alibaba_coding_plan.session_env.help",
    "core:field.kiro.auth_method.help",
    "core:field.kiro.auth_method.option.api_key.unavailable",
    "core:field.kiro.auth_method.option.builder-id.unavailable",
    "core:field.kiro.auth_method.option.external_idp.unavailable",
    "core:field.kiro.auth_method.option.github.unavailable",
    "core:field.kiro.auth_method.option.google.unavailable",
    "core:field.kiro.auth_method.option.idc.unavailable",
    "core:field.kiro.auth_method.option.import.unavailable",
    "core:login.alibaba_model_studio.bailian_console.completion_notice",
    "core:login.alibaba_model_studio.bailian_console.help",
    "core:login.alibaba_model_studio.bailian_console.option.profile.help",
    "core:login.alibaba_token_plan.bailian_console.completion_notice",
    "core:login.alibaba_token_plan.bailian_console.help",
    "core:login.alibaba_token_plan.bailian_console.option.profile.help",
    "core:login.antigravity.oauth.completion_notice",
    "core:login.antigravity.oauth.label",
    "core:login.aws_bedrock.aws_sso.completion_notice",
    "core:login.azure_open_ai.oauth.completion_notice",
    "core:login.azure_open_ai.oauth.label",
    "core:login.chat_gpt.oauth.completion_notice",
    "core:login.chat_gpt.oauth.label",
    "core:login.claude.oauth.completion_notice",
    "core:login.claude.oauth.label",
    "core:login.cline.oauth.completion_notice",
    "core:login.cline.oauth.label",
    "core:login.code_buddy.oauth.completion_notice",
    "core:login.code_buddy.oauth.label",
    "core:login.copilot.oauth.completion_notice",
    "core:login.copilot.oauth.label",
    "core:login.cursor.oauth.help",
    "core:login.gemini_ai.oauth.completion_notice",
    "core:login.gemini_ai.oauth.label",
    "core:login.gemini_api.oauth.completion_notice",
    "core:login.gemini_api.oauth.help",
    "core:login.gemini_api.oauth.label",
    "core:login.grok.oauth.completion_notice",
    "core:login.grok.oauth.label",
    "core:login.kimi.oauth.completion_notice",
    "core:login.kimi.oauth.label",
    "core:login.kiro.oauth.completion_notice",
    "core:login.kiro.oauth.label",
    "core:login.progress.bailian_done",
    "core:login.qoder.oauth.completion_notice",
    "core:login.qoder.oauth.label",
    "core:login.qwen.oauth.completion_notice",
    "core:login.qwen.oauth.help",
    "core:login.qwen.oauth.label",
    "core:login.vertex_ai.oauth.completion_notice",
    "core:login.vertex_ai.oauth.help",
    "core:login.vertex_ai.oauth.label",
    "core:login.xiaomi_mimo.oauth.label",
    "core:login.zed.oauth.completion_notice",
    "core:login.zed.oauth.label",
    "core:native.kiro.auth_method.help",
    "core:oauth.registration_help.gemini_api",
    "core:oauth.registration_help.vertex_ai",
    "core:oauth_error.aws.sso_login_incomplete",
    "core:oauth_error.bailian.profile_blank",
    "core:oauth_error.browser.local_port_in_use",
    "core:oauth_error.browser.provider_no_oauth",
    "core:oauth_error.config.credentials_not_in_options",
    "core:oauth_error.config.provider_no_integrated_oauth",
    "core:oauth_error.flows.provider_no_oauth",
    "core:oauth_error.kiro.invalid_auth_method",
    "core:oauth_error.kiro.invalid_aws_region",
    "core:oauth_error.login.dedicated_profile_missing",
    "core:oauth_error.login.mode_not_offered",
    "core:oauth_error.login.unknown_mode",
    "core:oauth_error.login.unknown_option",
    "core:oauth_error.native.invalid_login_endpoint",
    "core:oauth_error.native.local_key_generation_failed",
    "core:oauth_error.native.no_integrated_flow",
    "core:oauth_error.session.login_not_found",
    "core:oauth_error.store.macos_keychain_unavailable",
    "core:query.alibaba_plans.model_studio_cli_auth",
    "core:query.nine_router.locked",
    "core:query.nine_router.login_refused",
    "core:query.nine_router.redirected",
    "core:query.oauth.denied",
    "core:query.oauth.expired",
    "core:query.router_grok.missing_oauth_token",
    "core:query.router_keys.xiaomi_native_key_has_no_quota",
    "core:user_session.warning",
    "core:user_session.warning_env",
    "tui:login.cleanup_warning",
    "tui:login.code.closed",
    "tui:login.partial.base",
    "tui:login.partial.failed",
    "tui:login.partial.previous_restored",
    "tui:login.title",
    "tui:ui.login_cancelled",
    "tui:wizard.credential.action.authorized",
    "tui:wizard.credential.action.existing",
    "tui:wizard.credential.action.external",
    "tui:wizard.credential.action.finished",
    "tui:wizard.credential.action.start",
    "tui:wizard.credential.login_unavailable",
    "tui:wizard.hint.credential_login",
    "tui:wizard.hint.credential_modes",
    "tui:wizard.login.authorized",
    "tui:wizard.login.finished",
    "tui:wizard.summary.login.authorized",
    "tui:wizard.summary.login.existing",
    "tui:wizard.summary.login.pending",
    "ui:dialog.nativeNote",
    "ui:settings.autostart.label",
    "ui:settings.autostart.stale",
    "ui:settings.error.autostart_failed",
    "ui:settings.kw.autostart",
];

// G11
/// vi reset/renew ids that keep làm mới on purpose: "the next refresh".
const VI_LAM_MOI: &[&str] = &["core:resets.body", "core:resets.body.private"];

// G11
/// V35-06: Russian "account" is учётная запись, not аккаунт.
#[test]
fn ru_account_is_uchyotnaya_zapis() {
    glossary_ratchet("ru", "(?i)аккаунт", |_| true, RU_AKKAUNT);
}

// G11
/// V35-07: Korean "provider" is 제공업체, not 공급자 or 제공자.
#[test]
fn ko_provider_is_jegongeopche() {
    glossary_ratchet("ko", "공급자|제공자", |_| true, KO_PROVIDER);
}

// G11
/// V35-07: Simplified Chinese "provider" is 提供商, not 服务商.
#[test]
fn zh_cn_provider_is_tigongshang() {
    glossary_ratchet("zh-CN", "服务商", |_| true, ZH_CN_FUWUSHANG);
}

// G11
/// V35-07: German "reset" is Zurücksetzung (or the verb), not "Reset";
/// search keywords (`settings.kw.*`) may list it.
#[test]
fn de_reset_is_zuruecksetzung() {
    glossary_ratchet(
        "de",
        r"\bReset\b",
        |id| !id.starts_with("ui:settings.kw."),
        DE_RESET,
    );
}

// G11
/// V35-07: Hindi "sign in" is साइन इन; लॉगिन stays only where the English
/// says "login" (native login, autostart, `bl auth login`).
#[test]
fn hi_in_sign_in_is_sain_in() {
    glossary_ratchet("hi-IN", "लॉग इन|लॉगिन", |_| true, HI_IN_LOGIN);
}

// G11
/// V35-07: Vietnamese "reset"/"renew" is đặt lại; làm mới is "refresh".
/// Only ids about a reset or a renewal are checked.
#[test]
fn vi_reset_is_dat_lai() {
    glossary_ratchet(
        "vi",
        "(?i)làm mới",
        |id| id.contains("reset") || id.contains("renew"),
        VI_LAM_MOI,
    );
}

// G11
/// V35-11: the settings section "Appearance" and the theme picker "Theme"
/// are two words in every language.
#[test]
fn appearance_and_theme_are_distinct() {
    let mut same = Vec::new();
    for row in super::LOCALES.iter().filter(|row| row.is_translated()) {
        let catalog = layer_catalog("ui", row.code).expect("desktop catalog");
        let text = |id: &str| catalog.messages().get(id).and_then(|m| m.as_text());
        if text("settings.appearance").is_some()
            && text("settings.appearance") == text("theme.label")
        {
            same.push(row.code);
        }
    }
    assert!(
        same.is_empty(),
        "settings.appearance equals theme.label in {same:?}"
    );
}

// G12 ---------------------------------------------------------------------

// G12
/// V35-09: French typography (README register table): U+00A0 before `:`,
/// U+202F before `; ? !`, U+00A0 inside « ». A plain space before those
/// marks or inside the guillemets is refused, and so is the wrong no-break
/// space. Text inside `backticks` and `{placeholders}` is not checked, nor
/// a mark followed by anything but whitespace, the end or a closing quote
/// or bracket (code such as `::1`, `:free`, `?to=`).
#[test]
fn fr_spacing_uses_no_break_spaces() {
    let code_spans = Regex::new(r"`[^`]*`|\{[A-Za-z0-9_]*\}").unwrap();
    let ends_word =
        |next: Option<char>| next.is_none_or(|c| c.is_whitespace() || "\"')]»”".contains(c));
    let bad = offending_ids("fr", |_, text| {
        let text = code_spans.replace_all(text, "`");
        let chars: Vec<char> = text.chars().collect();
        chars.iter().enumerate().any(|(i, &c)| {
            let before = i.checked_sub(1).map(|j| chars[j]);
            let next = chars.get(i + 1).copied();
            match c {
                ':' | ';' | '?' | '!' if ends_word(next) => match before {
                    Some(' ') => true,
                    Some('\u{a0}') => c != ':',
                    Some('\u{202f}') => c == ':',
                    _ => false,
                },
                '»' => before == Some(' '),
                '«' => next == Some(' '),
                _ => false,
            }
        })
    });
    assert!(
        bad.is_empty(),
        "fr texts need U+00A0 before ':', U+202F before '; ? !' and U+00A0 inside « »: {bad:#?}"
    );
}

// G12
/// V35-08: Japanese writes the full-width colon `：` after Japanese text,
/// never an ASCII `:`.
#[test]
fn ja_colon_is_full_width_after_japanese_text() {
    let colon = Regex::new(r"[\u{3040}-\u{30FF}\u{3400}-\u{9FFF}\u{FF00}-\u{FFEF}]:").unwrap();
    let bad = offending_ids("ja", |_, text| colon.is_match(text));
    assert!(
        bad.is_empty(),
        "ja texts use an ASCII ':' after Japanese text: {bad:#?}"
    );
}
