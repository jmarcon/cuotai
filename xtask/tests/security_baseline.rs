//! PRD 0050 RF1.4/RF7.5: `cargo xtask security-baseline` refuses a weak
//! desktop policy, an inline script and a registry that drifted from the
//! tests, and passes on the repository itself.

use xtask::security::{
    check_client_builders, check_csp, check_gitignore, check_html, check_registry,
    check_tauri_config, markers, parse_registry, strip_cfg_test, EXPECTED_CSP,
};

fn baseline_csp() -> String {
    EXPECTED_CSP
        .iter()
        .map(|(name, sources)| format!("{name} {}", sources.join(" ")))
        .collect::<Vec<_>>()
        .join("; ")
}

fn config(csp: serde_json::Value) -> String {
    serde_json::json!({ "app": { "security": { "csp": csp }, "withGlobalTauri": true } })
        .to_string()
}

// security-invariant: SEC-WEBVIEW
#[test]
fn the_csp_fixtures_are_refused() {
    assert!(check_csp(&baseline_csp()).is_empty());
    assert!(check_tauri_config(&config(baseline_csp().into())).is_empty());

    let null = check_tauri_config(&config(serde_json::Value::Null));
    assert!(null[0].contains("must be a string"), "{null:?}");

    let swap = |from: &str, to: &str| check_csp(&baseline_csp().replacen(from, to, 1));
    let eval = swap("script-src 'self'", "script-src 'self' 'unsafe-eval'");
    assert!(eval.iter().any(|p| p.contains("eval")), "{eval:?}");
    let wildcard = swap("img-src 'self'", "img-src *");
    assert!(
        wildcard.iter().any(|p| p.contains("wildcard")),
        "{wildcard:?}"
    );
    let data = swap("img-src 'self'", "img-src 'self' data:");
    assert!(data.iter().any(|p| p.contains("data or blob")), "{data:?}");
    let remote = swap("img-src 'self'", "img-src 'self' https://example.com");
    assert!(remote.iter().any(|p| p.contains("remote")), "{remote:?}");
    let http = swap(
        "connect-src 'self'",
        "connect-src 'self' http://example.com",
    );
    assert!(http.iter().any(|p| p.contains("remote")), "{http:?}");
    let ipc = swap("img-src 'self'", "img-src 'self' ipc:");
    assert!(ipc.iter().any(|p| p.contains("IPC")), "{ipc:?}");
    let inline = swap("script-src 'self'", "script-src 'self' 'unsafe-inline'");
    assert!(inline.iter().any(|p| p.contains("inline")), "{inline:?}");
    let missing = check_csp(&baseline_csp().replace("; frame-ancestors 'none'", ""));
    assert!(
        missing.iter().any(|p| p.contains("frame-ancestors")),
        "{missing:?}"
    );
    let extra = check_csp(&format!("{}; worker-src 'self'", baseline_csp()));
    assert!(extra.iter().any(|p| p.contains("worker-src")), "{extra:?}");

    let unsafe_config = serde_json::json!({ "app": { "security": {
        "csp": baseline_csp(), "dangerousDisableAssetCspModification": true
    } } })
    .to_string();
    assert!(!check_tauri_config(&unsafe_config).is_empty());
}

#[test]
fn inline_scripts_handlers_and_remote_resources_are_refused_in_pages() {
    let good = r#"<!doctype html><html><head><!-- <script>x</script> -->
        <script src="js/first-paint.js"></script>
        <link rel="stylesheet" href="css/themes.css" /></head>
        <body><img src="img/logo.svg" alt=""><script type="module" src="js/main.js"></script></body></html>"#;
    assert!(
        check_html("good.html", good).is_empty(),
        "{:?}",
        check_html("good.html", good)
    );

    for (page, needle) in [
        ("<script>alert(1)</script>", "inline <script>"),
        ("<SCRIPT type=module>x()</SCRIPT>", "inline <script>"),
        (
            r#"<script src="https://example.com/a.js"></script>"#,
            "remote script",
        ),
        (r#"<script src="js/a.js">alert(1)</script>"#, "with a body"),
        (
            r#"<button onclick="go()">x</button>"#,
            "event handler onclick",
        ),
        (r#"<body ONLOAD='go()'>"#, "event handler onload"),
        (r#"<a href="javascript:go()">x</a>"#, "javascript: URL"),
        (r#"<img src="https://example.com/pixel">"#, "remote src"),
        (r#"<img src="data:image/png;base64,AA">"#, "remote src"),
        (
            r#"<link rel="stylesheet" href="//example.com/a.css">"#,
            "remote href",
        ),
        ("<iframe src=\"a.html\"></iframe>", "<iframe>"),
        (
            r#"<meta http-equiv="Content-Security-Policy" content="default-src *">"#,
            "http-equiv",
        ),
    ] {
        let problems = check_html("page.html", page);
        assert!(
            problems.iter().any(|p| p.contains(needle)),
            "{page}: {problems:?}"
        );
    }
}

fn registry(rows: &str) -> String {
    format!(
        "# Baseline\n\n| Invariant | Boundary | Result | Owner | Platforms |\n|---|---|---|---|---|\n{rows}"
    )
}

#[test]
fn the_registry_detects_undocumented_markers_and_stale_tests() {
    let marker = |id: &str| format!("// {}{id}\n", concat!("security-invariant", ": "));
    let source = format!("{}#[test]\nfn owning_test() {{}}\n", marker("SEC-ONE"));
    let sources = vec![("src/a.rs".to_string(), source.clone())];
    let doc = registry(
        "| `SEC-ONE` | b | r | `src/a.rs::owning_test` | Linux, macOS (Unix only), Windows |\n",
    );
    let rows = parse_registry(&doc).unwrap();
    assert_eq!(rows[0].platforms, ["Linux", "macOS", "Windows"]);
    assert!(check_registry(&rows, &sources).is_empty());

    // A marker no row documents.
    let extra = vec![
        ("src/a.rs".to_string(), source.clone()),
        ("src/b.rs".to_string(), marker("SEC-TWO")),
    ];
    let problems = check_registry(&rows, &extra);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("undocumented invariant marker SEC-TWO")),
        "{problems:?}"
    );

    // A renamed test.
    let stale = parse_registry(&registry(
        "| `SEC-ONE` | b | r | `src/a.rs::renamed_test` | Linux |\n",
    ))
    .unwrap();
    let problems = check_registry(&stale, &sources);
    assert!(
        problems.iter().any(|p| p.contains("stale test name")),
        "{problems:?}"
    );

    // A row whose marker vanished.
    let problems = check_registry(&rows, &[("src/a.rs".into(), "fn owning_test() {}".into())]);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("no test carries its marker")),
        "{problems:?}"
    );

    // Platforms are mandatory and named.
    assert!(parse_registry(&registry("| `SEC-ONE` | b | r | `x` |  |\n")).is_err());
    assert!(parse_registry(&registry("| `SEC-ONE` | b | r | `x` | BeOS |\n")).is_err());
    assert!(parse_registry(&registry("| `SEC-ONE` | b | r | none | Linux |\n")).is_err());
    assert!(parse_registry(&registry(
        "| `SEC-ONE` | b | r | `x` | Linux |\n| `SEC-ONE` | b | r | `x` | Linux |\n"
    ))
    .is_err());

    assert_eq!(markers(&marker("SEC-A-1")), ["SEC-A-1"]);
    assert!(check_gitignore("target/\ncuotai-dev.sqlite3\n").is_empty());
    assert!(!check_gitignore("target/\n").is_empty());
}

// security-invariant: SEC-REDIRECT
#[test]
fn http_clients_built_outside_the_seam_are_refused() {
    let file = |path: &str, text: &str| vec![(path.to_string(), text.to_string())];
    let direct = "fn f() { let c = reqwest::Client::builder().build(); }\n";
    let problems = check_client_builders(&file("crates/cuotai-core/src/providers/x.rs", direct));
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("x.rs:1"), "{problems:?}");

    for bad in [
        "fn f() { reqwest::Client::new(); }",
        "fn f() { reqwest::ClientBuilder::new(); }",
        "fn f() { reqwest::blocking::Client::new(); }",
        "use reqwest::{Client, Method};\nfn f() { Client::builder(); }",
        "use reqwest::Client;\nfn f() { Client::new(); }",
    ] {
        let problems = check_client_builders(&file("apps/desktop/src-tauri/src/a.rs", bad));
        assert!(!problems.is_empty(), "{bad}");
    }

    // The seam, test modules, test files, comments and other crates' types pass.
    assert!(check_client_builders(&file("crates/cuotai-core/src/network.rs", direct)).is_empty());
    let tested = "fn f() {}\n#[cfg(test)]\nmod tests {\n    const J: &str = r#\"{\"a\": \"}\"}\"#;\n    fn t() { reqwest::Client::new(); }\n}\n";
    assert!(check_client_builders(&file("crates/c/src/a.rs", tested)).is_empty());
    let helper = "#[cfg(test)]\nfn helper() { let _ = '{'; reqwest::Client::new(); }\nfn product() { reqwest::Client::new(); }\n";
    let problems = check_client_builders(&file("crates/c/src/a.rs", helper));
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].contains("a.rs:3"), "{problems:?}");
    for path in [
        "crates/c/tests/proxy.rs",
        "crates/c/src/serve/tests.rs",
        "crates/c/src/hooks_tests.rs",
    ] {
        assert!(
            check_client_builders(&file(path, direct)).is_empty(),
            "{path}"
        );
    }
    let declared = vec![
        (
            "crates/c/src/lib.rs".to_string(),
            "#[cfg(test)]\nmod fixtures;\n".to_string(),
        ),
        ("crates/c/src/fixtures.rs".to_string(), direct.to_string()),
    ];
    assert!(check_client_builders(&declared).is_empty());
    assert!(check_client_builders(&file(
        "crates/c/src/a.rs",
        "// reqwest::Client::new()\nfn f() { serve::Client::new(); }"
    ))
    .is_empty());
    assert!(check_client_builders(&file("xtask/src/a.rs", direct)).is_empty());
    assert_eq!(
        strip_cfg_test("a\n#[cfg(test)]\nmod t { x }\nb")
            .lines()
            .count(),
        4
    );
}

#[test]
fn the_repository_passes_its_own_baseline() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let count = xtask::security::run(&root).unwrap_or_else(|e| panic!("{e}"));
    assert!(count >= 7);
}
