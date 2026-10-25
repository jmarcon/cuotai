//! The page the browser shows after a loopback sign-in callback.
//!
//! One self-contained HTML document (inline CSS and SVG, no script, no
//! external request, a `default-src 'none'` CSP) for the three outcomes the
//! user can see: authorization received, denied, and not completed (a
//! malformed callback or a state mismatch). A timeout never reaches the
//! browser: nothing connects, so there is nothing to answer. The page never
//! carries a code, state, token or URL: the text is fixed per outcome.
//!
//! The core stays UI independent. A frontend may call [`set_callback_style`]
//! with the interface language and plain colour tokens taken from its theme;
//! without them the page uses a neutral palette and `prefers-color-scheme`.

use crate::text::Localized;
use std::sync::RwLock;

/// What the callback reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CallbackPage {
    /// Authorization received.
    Success,
    /// The user denied the authorization.
    Denied,
    /// The callback could not be completed (malformed or state mismatch).
    Failed,
}

/// The colour scheme of a frontend palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallbackScheme {
    Light,
    Dark,
}

/// Plain `#rgb` / `#rrggbb` colours for the page, named after the desktop
/// colour roles. One invalid value rejects the whole palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallbackPalette {
    pub bg: String,
    pub surface: String,
    pub border: String,
    pub fg: String,
    pub fg_muted: String,
    pub bad: String,
}

/// Language and optional theme of the page.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CallbackStyle {
    /// BCP 47-like interface language; empty means Portuguese.
    pub locale: String,
    /// The active theme's scheme and colours. `None` follows
    /// `prefers-color-scheme` with the built-in neutral palette.
    pub theme: Option<(CallbackScheme, CallbackPalette)>,
}

static STYLE: RwLock<Option<CallbackStyle>> = RwLock::new(None);

/// Set the language and theme used by every later callback page. Returns
/// `false` (keeping the previous style) when a palette colour is not a hex
/// colour or the locale is not a plain language code.
pub fn set_callback_style(style: CallbackStyle) -> bool {
    let locale_ok = style.locale.len() <= 35
        && style
            .locale
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let theme_ok = style.theme.as_ref().is_none_or(|(_, p)| {
        [&p.bg, &p.surface, &p.border, &p.fg, &p.fg_muted, &p.bad]
            .iter()
            .all(|c| is_hex(c))
    });
    if !locale_ok || !theme_ok {
        return false;
    }
    if let Ok(mut slot) = STYLE.write() {
        *slot = Some(style);
    }
    true
}

fn current_style() -> CallbackStyle {
    STYLE
        .read()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_default()
}

fn is_hex(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('#') else {
        return false;
    };
    matches!(rest.len(), 3 | 6) && rest.chars().all(|c| c.is_ascii_hexdigit())
}

fn text(page: CallbackPage, title: bool, locale: &str) -> String {
    let (id, pt) = match (page, title) {
        (CallbackPage::Success, true) => ("oauth.page.success.title", "Authorization received"),
        (CallbackPage::Success, false) => (
            "oauth.page.success.body",
            "You can close this window and go back to Cuotai.",
        ),
        (CallbackPage::Denied, true) => ("oauth.page.denied.title", "Authorization denied"),
        (CallbackPage::Denied, false) => (
            "oauth.page.denied.body",
            "Nothing was connected. You can close this window and try again in Cuotai.",
        ),
        (CallbackPage::Failed, true) => ("oauth.page.failed.title", "Could not complete"),
        (CallbackPage::Failed, false) => (
            "oauth.page.failed.body",
            "Close this window and start the sign-in again in Cuotai.",
        ),
    };
    Localized::fixed(id, pt).resolve(locale)
}

fn esc(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

const LOGO: &str = r##"<svg class="logo" xmlns="http://www.w3.org/2000/svg" viewBox="60 60 1560 392" role="img" aria-label="Cuotai"><g fill="none" stroke-width="72"><path d="M371.1 367.1A160 160 0 0 1 211.9 409.8" class="a0"/><path d="M196.1 404.3A160 160 0 0 1 96.9 272.7" class="a1"/><path d="M96.0 256.0A160 160 0 0 1 180.9 114.7" class="a2"/><path d="M196.1 107.7A160 160 0 0 1 371.1 144.9" class="a3"/></g><g fill="none" stroke="currentColor" stroke-width="58"><path d="M466.0 176V339A84 84 0 0 0 634.0 339M634.0 176V446"/><circle cx="848" cy="311" r="112"/><path d="M1101 100V446M1023 205.0H1179"/></g><g class="ai"><path d="M1209.0 446H1271.6L1381.1 66H1316.9Z"/><path d="M1513.0 446H1450.4L1340.9 66H1405.1Z"/><path d="M1279.2 283.7H1442.8V334.7H1279.2Z"/><path d="M1551.9 66H1609.9V446H1551.9Z"/></g></svg>"##;

/// The logotype keeps its own colours per scheme (never theme roles).
const LOGO_LIGHT: &str = "--a0:#00687d;--a1:#6b2fe0;--a2:#b5106c;--a3:#0b6e2a;--ai:#990058";
const LOGO_DARK: &str = "--a0:#8be9fd;--a1:#bd93f9;--a2:#ff79c6;--a3:#50fa7b;--ai:#ff79c6";
const NEUTRAL_LIGHT: &str =
    "--bg:#f6f7fa;--surface:#fff;--border:#d9dce5;--fg:#1d1f27;--muted:#555a68;--bad:#b3261e";
const NEUTRAL_DARK: &str =
    "--bg:#16181f;--surface:#1f222b;--border:#343846;--fg:#f2f3f7;--muted:#a9aebd;--bad:#ff6b6b";

/// Render the page for `page` with the process-wide [`CallbackStyle`].
pub(crate) fn render(page: CallbackPage) -> String {
    render_with(page, &current_style())
}

fn render_with(page: CallbackPage, style: &CallbackStyle) -> String {
    let locale = if style.locale.is_empty() {
        "pt-BR"
    } else {
        style.locale.as_str()
    };
    let title = esc(&text(page, true, locale));
    let body = esc(&text(page, false, locale));
    let (scheme_meta, vars) = match &style.theme {
        Some((scheme, p)) => {
            let (name, logo) = match scheme {
                CallbackScheme::Light => ("light", LOGO_LIGHT),
                CallbackScheme::Dark => ("dark", LOGO_DARK),
            };
            (
                name,
                format!(
                    ":root{{color-scheme:{name};--bg:{};--surface:{};--border:{};--fg:{};--muted:{};--bad:{};{logo}}}",
                    p.bg, p.surface, p.border, p.fg, p.fg_muted, p.bad
                ),
            )
        }
        None => (
            "light dark",
            format!(
                ":root{{color-scheme:light dark;{NEUTRAL_LIGHT};{LOGO_LIGHT}}}\
                 @media (prefers-color-scheme:dark){{:root{{{NEUTRAL_DARK};{LOGO_DARK}}}}}"
            ),
        ),
    };
    let (tone, mark) = if page == CallbackPage::Success {
        ("", "")
    } else {
        (" bad", "! ")
    };
    let lang = locale;
    format!(
        "<!doctype html><html lang=\"{lang}\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
<meta name=\"referrer\" content=\"no-referrer\">\
<meta name=\"color-scheme\" content=\"{scheme_meta}\">\
<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">\
<title>Cuotai</title><style>{vars}\
.a0{{stroke:var(--a0)}}.a1{{stroke:var(--a1)}}.a2{{stroke:var(--a2)}}.a3{{stroke:var(--a3)}}.ai{{fill:var(--ai)}}\
*{{box-sizing:border-box}}\
body{{margin:0;min-height:100vh;display:grid;place-items:center;padding:16px;background:var(--bg);color:var(--fg);\
font:16px/1.5 system-ui,-apple-system,\"Segoe UI\",Roboto,\"Helvetica Neue\",Arial,sans-serif}}\
main{{width:100%;max-width:420px;text-align:center;padding:40px 32px;background:var(--surface);\
border:1px solid var(--border);border-radius:16px}}\
.logo{{display:block;width:180px;max-width:70%;height:auto;margin:0 auto 28px;color:var(--fg)}}\
h1{{margin:0 0 8px;font-size:1.25rem;font-weight:650}}\
h1.bad{{color:var(--bad)}}\
p{{margin:0;color:var(--muted)}}\
</style></head><body><main role=\"status\">{LOGO}\
<h1 class=\"title{tone}\">{mark}{title}</h1><p>{body}</p></main></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [CallbackPage; 3] = [
        CallbackPage::Success,
        CallbackPage::Denied,
        CallbackPage::Failed,
    ];

    fn palette() -> CallbackPalette {
        CallbackPalette {
            bg: "#101010".into(),
            surface: "#202020".into(),
            border: "#303030".into(),
            fg: "#f0f0f0".into(),
            fg_muted: "#a0a0a0".into(),
            bad: "#ff5555".into(),
        }
    }

    #[test]
    fn every_variant_is_self_contained_and_branded() {
        for page in ALL {
            let html = render_with(page, &CallbackStyle::default());
            assert!(html.starts_with("<!doctype html>"));
            assert!(html.contains("aria-label=\"Cuotai\""), "logo");
            assert!(html.contains("prefers-color-scheme:dark"), "dark scheme");
            assert!(html.contains("--bg:#f6f7fa"), "light scheme");
            assert!(html.contains("default-src 'none'"));
            let html = html.replace("http://www.w3.org/2000/svg", "");
            for forbidden in [
                "<script", "http://", "https://", "src=", "href=", "@import", "url(",
            ] {
                assert!(!html.contains(forbidden), "{forbidden} in {page:?}");
            }
        }
    }

    #[test]
    fn text_is_fixed_per_outcome_and_localized() {
        let pt = render_with(CallbackPage::Success, &CallbackStyle::default());
        assert!(pt.contains("Você pode fechar esta janela e voltar ao Cuotai."));
        for (locale, needle) in [
            ("en-US", "close this window"),
            ("es", "cerrar esta ventana"),
            ("it", "chiudere questa finestra"),
        ] {
            let style = CallbackStyle {
                locale: locale.into(),
                theme: None,
            };
            for page in ALL {
                let html = render_with(page, &style);
                assert!(!html.contains("oauth.page."), "{locale} {page:?}");
            }
            assert!(
                render_with(CallbackPage::Success, &style).contains(needle),
                "{locale}"
            );
        }
    }

    #[test]
    fn failure_uses_the_danger_role_and_never_echoes_request_data() {
        let failed = render_with(CallbackPage::Failed, &CallbackStyle::default());
        assert!(failed.contains("h1.bad{color:var(--bad)}"));
        assert!(failed.contains("class=\"title bad\""));
        let ok = render_with(CallbackPage::Success, &CallbackStyle::default());
        assert!(!ok.contains("title bad"));
        // The renderer takes no request input, so nothing of it can appear.
        for page in ALL {
            let html = render_with(page, &CallbackStyle::default());
            for secret in ["code=", "state=", "access_token", "127.0.0.1"] {
                assert!(!html.contains(secret));
            }
        }
    }

    #[test]
    fn a_frontend_theme_pins_the_scheme_and_colours() {
        let style = CallbackStyle {
            locale: "en-US".into(),
            theme: Some((CallbackScheme::Dark, palette())),
        };
        let html = render_with(CallbackPage::Success, &style);
        assert!(html.contains("color-scheme:dark;--bg:#101010"));
        assert!(!html.contains("prefers-color-scheme"));
        assert!(html.contains("--a2:#ff79c6"), "logo keeps its dark colours");
    }

    #[test]
    fn invalid_palette_colours_are_rejected() {
        let mut bad = palette();
        bad.bg = "red;}</style><script>".into();
        assert!(!set_callback_style(CallbackStyle {
            locale: String::new(),
            theme: Some((CallbackScheme::Light, bad)),
        }));
        assert!(is_hex("#abc") && is_hex("#A1b2C3") && !is_hex("#abcd") && !is_hex("abc"));
    }
}
