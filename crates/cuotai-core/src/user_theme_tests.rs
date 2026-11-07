use super::*;

const OCEAN: &str = r#"/*! cuotai-theme {"label":"Ocean","group":"custom","modes":["dark","light"]} */
/* A plain comment is dropped. */
:root[data-theme="ocean"][data-mode="dark"] {
  color-scheme: dark;
  --bg: #0B1A24; --surface: rgba(17, 41, 58, 0.9);
  --font-ui: -apple-system, "Segoe UI", system-ui, sans-serif;
  --radius-lg: 14px;
  --shadow: 0 1px 2px rgba(0, 0, 0, 0.3), 0 8px 24px rgb(0 0 0 / 25%);
  --hatch: repeating-linear-gradient(135deg, var(--track) 0 2px, transparent 2px 5px);
  --scrim: color-mix(in srgb, #0b0e14 55%, transparent);
  --motion-fast: 0;
  --ease-out: cubic-bezier(0.22, 1, 0.36, 1);
  --ease-spring: steps(4, end);
}
:root[data-theme='ocean'][data-mode='light'] {
  color-scheme: light;
  --bg: #eef6fb;
  --body-bg: linear-gradient(180deg, #f3ecff 0%, #ffe6f1 100%);
}
"#;

fn reason(id: &str, css: &str) -> &'static str {
    parse(id, css).map(|_| "ok").unwrap_or_else(|e| e.reason())
}

/// Wrap `decl` in a valid two-mode theme named `t`.
fn with(decl: &str) -> String {
    format!(
        ":root[data-theme=\"t\"][data-mode=\"dark\"] {{ {decl} }}\n:root[data-theme=\"t\"][data-mode=\"light\"] {{ --bg: #ffffff; }}"
    )
}

// security-invariant: SEC-USER-THEME
#[test]
fn a_scoped_token_theme_is_accepted_and_rebuilt_from_its_tokens() {
    let theme = parse("ocean", OCEAN).unwrap();
    assert_eq!(theme.label, "Ocean");
    assert_eq!(theme.group, "custom");
    assert_eq!(theme.modes, ["dark", "light"]);
    assert!(
        !theme.css.contains("/*"),
        "comments never reach the webview"
    );
    assert!(theme.css.contains("--bg: #0b1a24;"));
    assert!(theme
        .css
        .contains(":root[data-theme=\"ocean\"][data-mode=\"light\"] {"));
    assert!(theme
        .css
        .contains("--shadow: 0 1px 2px rgba(0, 0, 0, 0.3), 0 8px 24px rgb(0 0 0 / 25%);"));
    assert!(theme
        .css
        .contains("--font-ui: -apple-system, \"Segoe UI\", system-ui, sans-serif;"));
    // The canonical text is a fixed point.
    assert_eq!(parse("ocean", &theme.css).unwrap().css, theme.css);
}

#[test]
fn without_a_header_the_id_names_the_theme() {
    let theme = parse("t", &with("--bg: #000;")).unwrap();
    assert_eq!(
        (theme.label.as_str(), theme.group.as_str()),
        ("t", "custom")
    );
}

#[test]
fn css_escapes_and_control_characters_are_refused() {
    assert_eq!(reason("t", &with("--bg: \\23 000;")), "syntax");
    assert_eq!(reason("t", &with("--font-ui: \"A\\\"B\";")), "syntax");
    assert_eq!(
        reason("t", &with("--b\\67: #000;").replace("--b\\67", "--\\62 g")),
        "syntax"
    );
    assert_eq!(reason("t", &with("--bg: #000;\u{0}")), "syntax");
}

#[test]
fn url_tokens_and_image_functions_are_refused() {
    assert_eq!(
        reason("t", &with("--body-bg: url(https://example.com/pixel);")),
        "externalUrl"
    );
    assert_eq!(
        reason("t", &with("--body-bg: URL( \"data:x\");")),
        "externalUrl"
    );
    assert_eq!(
        reason("t", &with("--body-bg: image-set(\"a.png\" 1x);")),
        "function"
    );
    for f in [
        "cross-fade",
        "element",
        "paint",
        "image",
        "src",
        "calc",
        "env",
        "expression",
    ] {
        assert_eq!(
            reason("t", &with(&format!("--body-bg: {f}(x);"))),
            "function",
            "{f}"
        );
    }
    // A url hidden in a nested function is still found.
    assert_eq!(
        reason("t", &with("--body-bg: linear-gradient(url(x), #000);")),
        "externalUrl"
    );
    // An identifier merely containing "url" is fine.
    assert_eq!(reason("t", &with("--font-ui: curly, sans-serif;")), "ok");
}

#[test]
fn nested_functions_are_bounded_and_checked() {
    assert_eq!(
        reason(
            "t",
            &with(
                "--body-bg: linear-gradient(color-mix(in srgb, var(--fg) 5%, transparent), #000);"
            )
        ),
        "ok"
    );
    assert_eq!(
        reason(
            "t",
            &with("--body-bg: linear-gradient(color-mix(in srgb, rgba(0, 0, 0, hsl(1, 2%, 3%)) 5%, #000), #000);")
        ),
        "value"
    );
    assert_eq!(reason("t", &with("--bg: rgba(0, 0, 0, 0.5;")), "syntax");
    assert_eq!(reason("t", &with("--bg: var(--unknown);")), "value");
    assert_eq!(
        reason("t", &with("--bg: var(--fg, url(x));")),
        "externalUrl"
    );
    assert_eq!(reason("t", &with("--bg: var(--fg, #000);")), "value");
}

#[test]
fn modes_must_be_exactly_one_dark_and_one_light() {
    let dark = ":root[data-theme=\"t\"][data-mode=\"dark\"] { --bg: #000; }";
    let light = ":root[data-theme=\"t\"][data-mode=\"light\"] { --bg: #fff; }";
    assert_eq!(reason("t", dark), "modes");
    assert_eq!(reason("t", &format!("{dark}{dark}{light}")), "modes");
    assert_eq!(reason("t", &format!("{light}{dark}")), "ok");
    assert_eq!(reason("t", ""), "modes");
}

#[test]
fn selectors_outside_the_scope_are_refused() {
    let light = ":root[data-theme=\"t\"][data-mode=\"light\"] { --bg: #fff; }";
    for prelude in [
        ":root[data-theme=\"other\"][data-mode=\"dark\"]",
        ":root[data-theme=\"t\"]",
        ":root[data-theme=\"t\"][data-mode=\"dark\"] .card",
        ":root[data-theme=\"t\"][data-mode=\"dark\"], body",
        "body",
    ] {
        assert_eq!(
            reason("t", &format!("{prelude} {{ --bg: #000; }}{light}")),
            "scope",
            "{prelude}"
        );
    }
}

#[test]
fn properties_outside_the_tokens_and_important_are_refused() {
    assert_eq!(reason("t", &with("background: #000;")), "property");
    assert_eq!(reason("t", &with("--my-token: #000;")), "property");
    assert_eq!(reason("t", &with("--bg: #000 !important;")), "value");
    assert_eq!(reason("t", &with("--bg: #00;")), "value");
    assert_eq!(reason("t", &with("--bg: 1px solid;")), "value");
    assert_eq!(reason("t", &with("--radius-lg: 14vw;")), "value");
    assert_eq!(reason("t", &with("--motion-fast: 150px;")), "value");
    assert_eq!(reason("t", &with("color-scheme: purple;")), "value");
    assert_eq!(reason("t", &with("--font-ui: #000;")), "value");
    assert_eq!(reason("t", &with("--bg: ;")), "value");
    assert_eq!(reason("t", &with("--bg #000;")), "syntax");
}

#[test]
fn at_rules_and_imports_are_refused() {
    assert_eq!(
        reason("t", &format!("@import \"x.css\";{}", with("--bg: #000;"))),
        "externalUrl"
    );
    assert_eq!(
        reason("t", &format!("@font-face {{ }}{}", with("--bg: #000;"))),
        "externalUrl"
    );
    assert_eq!(
        reason("t", &format!("@media print {{ }}{}", with("--bg: #000;"))),
        "atRule"
    );
    // An at-sign inside a string is not an at-rule.
    assert_eq!(
        reason("t", &with("--font-ui: \"Font@Home\", sans-serif;")),
        "ok"
    );
}

#[test]
fn the_header_and_the_id_are_checked() {
    assert_eq!(reason("Bad_Id", &with("--bg: #000;")), "invalidId");
    assert_eq!(
        reason(
            "t",
            &format!("/*! cuotai-theme {{nope */{}", with("--bg: #000;"))
        ),
        "invalidMeta"
    );
    assert_eq!(
        reason(
            "t",
            &format!("/*! cuotai-theme {{\"label\":3}} */{}", with("--bg: #000;"))
        ),
        "invalidMeta"
    );
    assert_eq!(
        reason(
            "t",
            &format!(
                "/*! cuotai-theme {{}} */ /*! cuotai-theme {{}} */{}",
                with("--bg: #000;")
            )
        ),
        "invalidMeta"
    );
    assert_eq!(
        reason("t", &format!("/* unterminated {}", with("--bg: #000;"))),
        "syntax"
    );
}

/// Every bundled example and the token blocks of every built-in theme fit
/// the user contract, so the documented tokens and grammar stay in step.
#[test]
fn the_bundled_example_and_the_built_in_token_blocks_pass() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../apps/desktop");
    let examples = root.join("examples/user/themes");
    for entry in std::fs::read_dir(&examples).unwrap() {
        let path = entry.unwrap().path();
        let id = path.file_stem().unwrap().to_str().unwrap().to_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        parse(&id, &text).unwrap_or_else(|e| panic!("{id}: {e}"));
    }
    let themes = root.join("ui/css/themes");
    let mut checked = 0;
    for entry in std::fs::read_dir(&themes).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("css") {
            continue;
        }
        let id = path.file_stem().unwrap().to_str().unwrap().to_owned();
        if id == "index" {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let mut blocks = String::new();
        for mode in ["dark", "light"] {
            let head = format!(":root[data-theme=\"{id}\"][data-mode=\"{mode}\"] {{");
            let start = text.find(&head).unwrap_or_else(|| panic!("{id} {mode}"));
            let end = start + text[start..].find("\n}").unwrap() + 2;
            blocks.push_str(&text[start..end]);
            blocks.push('\n');
        }
        parse(&id, &blocks).unwrap_or_else(|e| panic!("{id}: {e}"));
        checked += 1;
    }
    assert!(checked >= 15);
}
