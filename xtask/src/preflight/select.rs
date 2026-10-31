//! Which preflight steps a set of changed files needs. This is the core of
//! `cargo xtask preflight`: pure functions from paths to steps, with no I/O,
//! so every routing decision has a table test.
//!
//! The documentation allow-list ([`is_docs`]) and the release tooling list
//! ([`is_release_tooling`]) mirror the `changes` job of `ci.yml`, and a test
//! reads that job to keep them equal. CI stays the authority. A path this
//! module does not know is code, as in CI, and runs every Rust check.

use std::collections::BTreeSet;

/// One preflight step, declared from cheap to expensive: that is the run
/// order, because a run stops at its first failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Step {
    /// Identity and skip marker of the commits of the range.
    CommitPolicy,
    /// `rustfmt --check` of the staged blobs.
    FmtStaged,
    Fmt,
    FmtDesktop,
    ChangelogGate,
    ChangelogCheck,
    ReleaseCheck,
    WorkflowCheck,
    Actionlint,
    Privacy,
    /// `scripts/release/test-collect-linux.sh`.
    ReleaseTooling,
    Deny,
    DenyDesktop,
    SiteBuild,
    UiLint,
    UiTests,
    UiCheck,
    /// The rest of the `ui-checks` job of CI (`--full` only).
    UiBrowser,
    Clippy,
    ClippyDesktop,
    Test,
    TestDesktop,
    /// The no-network TUI smoke of CI (`--full` only).
    TuiSmoke,
    /// `build-appex.sh`, whose output check is the only local proof of the
    /// extension entry point.
    Widgets,
    CodeqlJs,
    CodeqlRust,
    /// CodeQL for the remaining languages (`--full` only).
    CodeqlOther,
}

impl Step {
    pub const ALL: [Step; 27] = [
        Step::CommitPolicy,
        Step::FmtStaged,
        Step::Fmt,
        Step::FmtDesktop,
        Step::ChangelogGate,
        Step::ChangelogCheck,
        Step::ReleaseCheck,
        Step::WorkflowCheck,
        Step::Actionlint,
        Step::Privacy,
        Step::ReleaseTooling,
        Step::Deny,
        Step::DenyDesktop,
        Step::SiteBuild,
        Step::UiLint,
        Step::UiTests,
        Step::UiCheck,
        Step::UiBrowser,
        Step::Clippy,
        Step::ClippyDesktop,
        Step::Test,
        Step::TestDesktop,
        Step::TuiSmoke,
        Step::Widgets,
        Step::CodeqlJs,
        Step::CodeqlRust,
        Step::CodeqlOther,
    ];

    /// The name printed in the report and accepted by `CUOTAI_SKIP`.
    pub fn id(self) -> &'static str {
        match self {
            Step::CommitPolicy => "commit-policy",
            Step::FmtStaged | Step::Fmt => "fmt",
            Step::FmtDesktop => "fmt-desktop",
            Step::ChangelogGate => "changelog-gate",
            Step::ChangelogCheck => "changelog-check",
            Step::ReleaseCheck => "release-check",
            Step::WorkflowCheck => "workflow-check",
            Step::Actionlint => "actionlint",
            Step::Privacy => "privacy",
            Step::ReleaseTooling => "release-tooling",
            Step::Deny => "deny",
            Step::DenyDesktop => "deny-desktop",
            Step::SiteBuild => "site-build",
            Step::UiLint => "ui-lint",
            Step::UiTests => "ui-tests",
            Step::UiCheck => "ui-check",
            Step::UiBrowser => "ui-browser",
            Step::Clippy => "clippy",
            Step::ClippyDesktop => "clippy-desktop",
            Step::Test => "test",
            Step::TestDesktop => "test-desktop",
            Step::TuiSmoke => "tui-smoke",
            Step::Widgets => "widgets",
            Step::CodeqlJs => "codeql-js",
            Step::CodeqlRust => "codeql-rust",
            Step::CodeqlOther => "codeql-other",
        }
    }
}

/// A package of the root workspace. All of them together run as
/// `--workspace`, exactly as CI does.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Package {
    Core,
    Tui,
    Xtask,
}

impl Package {
    pub const ALL: [Package; 3] = [Package::Core, Package::Tui, Package::Xtask];

    pub fn name(self) -> &'static str {
        match self {
            Package::Core => "cuotai-core",
            Package::Tui => "cuotai-tui",
            Package::Xtask => "xtask",
        }
    }
}

/// What a run executes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    pub steps: Vec<Step>,
    /// The root-workspace packages `Clippy` and `Test` cover.
    pub packages: Vec<Package>,
}

/// One effect of a changed path.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Area {
    Core,
    Tui,
    Xtask,
    Desktop,
    Ui,
    Workflows,
    Site,
    RootLock,
    DesktopLock,
    DenyConfig,
    Widgets,
}

/// Mirrors the `kind=docs` patterns of the `changes` job of `ci.yml`, where
/// `*` also matches `/`. `docs/providers.md` is code: a core test compares
/// its matrix with the code.
pub fn is_docs(path: &str) -> bool {
    if path == "docs/providers.md" {
        return false;
    }
    matches!(
        path,
        "README.md"
            | "CONTRIBUTING.md"
            | "SECURITY.md"
            | "SUPPORT.md"
            | "AGENTS.md"
            | "apps/desktop/README.md"
    ) || (path.starts_with("docs/") && path.ends_with(".md"))
        || path.starts_with("docs/assets/")
        || path.starts_with(".github/ISSUE_TEMPLATE/")
        || (path.starts_with(".github/") && path.ends_with(".md"))
        || path.starts_with("site/")
}

/// Mirrors the `release_tooling` patterns of the `changes` job of `ci.yml`.
pub fn is_release_tooling(path: &str) -> bool {
    path.starts_with("xtask/")
        || path.starts_with(".github/workflows/")
        || path.starts_with("scripts/release/")
        || path.starts_with("packaging/")
        || path == "deny.toml"
        || (path.starts_with("apps/desktop/src-tauri/tauri") && path.ends_with(".json"))
}

pub fn is_workflow(path: &str) -> bool {
    path.starts_with(".github/workflows/") && (path.ends_with(".yml") || path.ends_with(".yaml"))
}

fn is_rust(path: &str) -> bool {
    path.ends_with(".rs")
}

fn is_desktop_rust(path: &str) -> bool {
    is_rust(path) && path.starts_with("apps/desktop/src-tauri/")
}

/// JavaScript CodeQL scans. The documentation allow-list holds `site/`, which
/// the CodeQL workflow skips too.
fn is_javascript(path: &str) -> bool {
    !is_docs(path)
        && [".js", ".mjs", ".cjs", ".jsx", ".ts", ".tsx"]
            .iter()
            .any(|extension| path.ends_with(extension))
}

/// Code that handles credentials, tokens, a listening socket, downloads or
/// the desktop permission surface: worth the minutes of a Rust CodeQL run.
fn is_security_sensitive(path: &str) -> bool {
    const PATHS: [&str; 14] = [
        "crates/cuotai-core/src/oauth/",
        "crates/cuotai-core/src/serve",
        "crates/cuotai-core/src/secret_input",
        "crates/cuotai-core/src/user_session",
        "crates/cuotai-core/src/private_fs",
        "crates/cuotai-core/src/transport",
        "crates/cuotai-core/src/update_check",
        "crates/cuotai-core/src/hooks",
        "crates/cuotai-tui/src/serve",
        "apps/desktop/src-tauri/capabilities/",
        "apps/desktop/src-tauri/src/local_server",
        "apps/desktop/src-tauri/src/session_capture",
        "apps/desktop/src-tauri/src/updater",
        "apps/desktop/src-tauri/src/hooks",
    ];
    PATHS.iter().any(|prefix| path.starts_with(prefix))
}

/// Every path that is not documentation and that no rule names runs every
/// Rust check: the same fail-safe as `kind=code` in CI.
fn route(path: &str) -> &'static [Area] {
    use Area::*;
    const EVERY_RUST: &[Area] = &[Core, Tui, Xtask, Desktop];
    if is_docs(path) {
        // The Pages workflow also builds on the shared screenshots.
        let site = path.starts_with("site/") || path.starts_with("docs/assets/screenshots/");
        return if site { &[Site] } else { &[] };
    }
    match path {
        "CHANGELOG.md" => return &[Core, Xtask],
        "docs/providers.md" => return &[Core],
        "Cargo.lock" => return &[Core, Tui, Xtask, RootLock],
        "apps/desktop/src-tauri/Cargo.lock" => return &[Desktop, DesktopLock],
        "deny.toml" => return &[DenyConfig, Xtask],
        // xtask compiles this core file with `#[path]`.
        "crates/cuotai-core/src/changelog.rs" => return &[Core, Xtask],
        "scripts/site-build.sh" => return &[Site],
        "scripts/codeql-local.sh" => return &[],
        ".github/workflows/pages.yml" => return &[Workflows, Site, Xtask],
        _ => {}
    }
    if path.starts_with("docs/") && path.ends_with(".json") {
        return &[Core];
    }
    if path.starts_with("crates/cuotai-core/locales/") {
        // Several UI scripts read the core catalogs.
        return &[Core, Ui];
    }
    if path.starts_with("crates/cuotai-core/") {
        return &[Core];
    }
    if path.starts_with("crates/cuotai-tui/") {
        return &[Tui];
    }
    if path.starts_with("xtask/") || path.starts_with(".githooks/") {
        return &[Xtask];
    }
    if path.starts_with(".github/workflows/") {
        return &[Workflows, Xtask];
    }
    if path.starts_with("scripts/release/") || path.starts_with("packaging/") {
        return &[Xtask];
    }
    if path.starts_with("scripts/") && path.ends_with(".mjs") {
        return &[Ui];
    }
    if path.starts_with("apps/desktop/ui/locales/") {
        // `desk_text.rs` compiles these catalogs in.
        return &[Ui, Desktop];
    }
    if path.starts_with("apps/desktop/ui/") || path.starts_with("apps/desktop/tools/") {
        return &[Ui];
    }
    if path.starts_with("apps/desktop/widgets/") {
        return &[Widgets];
    }
    if path.starts_with("apps/desktop/") {
        return &[Desktop];
    }
    EVERY_RUST
}

/// `--commit`: only what the staged files can break, in seconds.
pub fn for_commit(files: &[String]) -> Plan {
    let mut steps = BTreeSet::new();
    if !files.is_empty() {
        steps.insert(Step::Privacy);
    }
    for path in files {
        if is_rust(path) {
            steps.insert(Step::FmtStaged);
        }
        if path == "CHANGELOG.md" || path.starts_with("docs/plans/") {
            steps.insert(Step::ChangelogCheck);
        }
        if is_workflow(path) {
            steps.insert(Step::Actionlint);
        }
    }
    Plan {
        steps: steps.into_iter().collect(),
        packages: Vec::new(),
    }
}

/// `--push`: what CI would run for this diff, on one machine. A core change
/// reaches its dependents, the TUI and the desktop app.
pub fn for_push(files: &[String]) -> Plan {
    let mut steps = BTreeSet::from([
        Step::CommitPolicy,
        Step::ChangelogGate,
        Step::ChangelogCheck,
    ]);
    let mut areas = BTreeSet::new();
    for path in files {
        areas.extend(route(path).iter().copied());
        if is_rust(path) {
            steps.insert(if is_desktop_rust(path) {
                Step::FmtDesktop
            } else {
                Step::Fmt
            });
        }
        if !is_docs(path) {
            steps.insert(Step::ReleaseCheck);
        }
        if is_release_tooling(path) {
            steps.insert(Step::ReleaseTooling);
        }
        if is_javascript(path) {
            steps.insert(Step::CodeqlJs);
        }
        if is_security_sensitive(path) {
            steps.insert(Step::CodeqlRust);
        }
    }

    let mut packages = BTreeSet::new();
    let area = |wanted: Area| areas.contains(&wanted);
    if area(Area::Core) {
        packages.extend([Package::Core, Package::Tui]);
    }
    if area(Area::Tui) {
        packages.insert(Package::Tui);
    }
    if area(Area::Xtask) {
        packages.insert(Package::Xtask);
    }
    if !packages.is_empty() {
        steps.extend([Step::Clippy, Step::Test]);
    }
    if area(Area::Desktop) || area(Area::Core) {
        steps.extend([Step::ClippyDesktop, Step::TestDesktop]);
    }
    if area(Area::Ui) {
        steps.extend([Step::UiLint, Step::UiTests, Step::UiCheck]);
    }
    if area(Area::Workflows) {
        steps.extend([Step::WorkflowCheck, Step::Actionlint]);
    }
    if area(Area::Site) {
        steps.insert(Step::SiteBuild);
    }
    if area(Area::RootLock) || area(Area::DenyConfig) {
        steps.insert(Step::Deny);
    }
    if area(Area::DesktopLock) || area(Area::DenyConfig) {
        steps.insert(Step::DenyDesktop);
    }
    if area(Area::Widgets) {
        steps.insert(Step::Widgets);
    }
    // `cargo test -p xtask` already holds the privacy test.
    if !packages.contains(&Package::Xtask) {
        steps.insert(Step::Privacy);
    }
    Plan {
        steps: steps.into_iter().collect(),
        packages: packages.into_iter().collect(),
    }
}

/// `--full`: everything, with no filter. It also stands in for a diff that
/// cannot be computed, as in CI.
pub fn for_full() -> Plan {
    Plan {
        steps: Step::ALL
            .into_iter()
            .filter(|step| !matches!(step, Step::FmtStaged | Step::Privacy))
            .collect(),
        packages: Package::ALL.to_vec(),
    }
}

/// Whether `CUOTAI_SKIP` names `step`: its id, or the first word of its id
/// (`clippy` covers `clippy-desktop`, `codeql` every CodeQL step).
pub fn is_skipped(step: Step, names: &[String]) -> bool {
    let id = step.id();
    names.iter().any(|name| {
        id == name
            || id
                .strip_prefix(name.as_str())
                .is_some_and(|rest| rest.starts_with('-'))
    })
}

/// `CUOTAI_SKIP=clippy,test` as a list of names.
pub fn parse_skip(value: &str) -> Vec<String> {
    value
        .split([',', ' '])
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// The names of `names` that select no step.
pub fn unknown_skip_names(names: &[String]) -> Vec<&str> {
    names
        .iter()
        .filter(|name| {
            !Step::ALL
                .into_iter()
                .any(|step| is_skipped(step, std::slice::from_ref(name)))
        })
        .map(String::as_str)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(paths: &[&str]) -> Vec<String> {
        paths.iter().map(|path| path.to_string()).collect()
    }

    fn push(paths: &[&str]) -> Plan {
        for_push(&files(paths))
    }

    fn commit(paths: &[&str]) -> Vec<Step> {
        for_commit(&files(paths)).steps
    }

    /// Steps every push runs when the xtask tests are not selected.
    const ALWAYS: [Step; 4] = [
        Step::CommitPolicy,
        Step::ChangelogGate,
        Step::ChangelogCheck,
        Step::Privacy,
    ];

    fn sorted(mut steps: Vec<Step>) -> Vec<Step> {
        steps.sort();
        steps
    }

    /// `expected` plus the steps every push runs.
    fn with_always(expected: &[Step]) -> Vec<Step> {
        sorted(ALWAYS.iter().chain(expected).copied().collect())
    }

    #[test]
    fn commit_runs_only_what_is_staged() {
        assert!(commit(&[]).is_empty());
        assert_eq!(commit(&["README.md"]), [Step::Privacy]);
        assert_eq!(
            commit(&["crates/cuotai-core/src/lib.rs"]),
            [Step::FmtStaged, Step::Privacy]
        );
        assert_eq!(
            commit(&["apps/desktop/src-tauri/src/lib.rs"]),
            [Step::FmtStaged, Step::Privacy]
        );
        assert_eq!(
            commit(&["docs/plans/0039_x.md"]),
            [Step::ChangelogCheck, Step::Privacy]
        );
        assert_eq!(
            commit(&["CHANGELOG.md"]),
            [Step::ChangelogCheck, Step::Privacy]
        );
        assert_eq!(
            commit(&[".github/workflows/ci.yml"]),
            [Step::Actionlint, Step::Privacy]
        );
        // A Markdown file next to the workflows is not a workflow.
        assert_eq!(commit(&[".github/workflows/x.md"]), [Step::Privacy]);
    }

    #[test]
    fn push_of_documentation_runs_only_the_gates() {
        for paths in [
            &["README.md"][..],
            &["AGENTS.md", "docs/plans/0039_x.md"],
            &["docs/assets/logo.png"],
            &[".github/ISSUE_TEMPLATE/bug.yml", ".github/pull_request.md"],
            &["apps/desktop/README.md"],
        ] {
            let plan = push(paths);
            assert_eq!(plan.steps, with_always(&[]), "{paths:?}");
            assert!(plan.packages.is_empty(), "{paths:?}");
        }
    }

    #[test]
    fn push_of_nothing_runs_only_the_gates() {
        assert_eq!(push(&[]).steps, with_always(&[]));
    }

    #[test]
    fn core_reaches_its_dependents() {
        let plan = push(&["crates/cuotai-core/src/model.rs"]);
        assert_eq!(plan.packages, [Package::Core, Package::Tui]);
        assert_eq!(
            plan.steps,
            with_always(&[
                Step::Fmt,
                Step::ReleaseCheck,
                Step::Clippy,
                Step::ClippyDesktop,
                Step::Test,
                Step::TestDesktop,
            ])
        );
    }

    #[test]
    fn tui_stays_in_the_tui() {
        let plan = push(&["crates/cuotai-tui/src/ui.rs"]);
        assert_eq!(plan.packages, [Package::Tui]);
        assert_eq!(
            plan.steps,
            with_always(&[Step::Fmt, Step::ReleaseCheck, Step::Clippy, Step::Test])
        );
    }

    #[test]
    fn desktop_rust_stays_in_the_desktop() {
        let plan = push(&["apps/desktop/src-tauri/src/tray_bar.rs"]);
        assert!(plan.packages.is_empty());
        assert_eq!(
            plan.steps,
            with_always(&[
                Step::FmtDesktop,
                Step::ReleaseCheck,
                Step::ClippyDesktop,
                Step::TestDesktop,
            ])
        );
    }

    #[test]
    fn xtask_covers_the_privacy_test_and_the_release_tooling() {
        let plan = push(&["xtask/src/preflight/mod.rs"]);
        assert_eq!(plan.packages, [Package::Xtask]);
        assert_eq!(
            plan.steps,
            sorted(vec![
                Step::CommitPolicy,
                Step::Fmt,
                Step::ChangelogGate,
                Step::ChangelogCheck,
                Step::ReleaseCheck,
                Step::ReleaseTooling,
                Step::Clippy,
                Step::Test,
            ])
        );
    }

    #[test]
    fn the_core_changelog_parser_also_reaches_xtask() {
        let plan = push(&["crates/cuotai-core/src/changelog.rs"]);
        assert_eq!(plan.packages, Package::ALL);
    }

    #[test]
    fn ui_runs_the_node_checks_and_not_the_rust_ones() {
        for path in [
            "apps/desktop/ui/js/app.js",
            "apps/desktop/ui/css/themes.css",
            "scripts/test-meter-ui.mjs",
            "scripts/ui-test-env.mjs",
            "apps/desktop/tools/ui-shots/check.mjs",
        ] {
            let plan = push(&[path]);
            assert!(plan.steps.contains(&Step::UiTests), "{path}");
            assert!(plan.steps.contains(&Step::UiCheck), "{path}");
            assert!(plan.steps.contains(&Step::UiLint), "{path}");
            assert!(!plan.steps.contains(&Step::ClippyDesktop), "{path}");
            assert!(plan.packages.is_empty(), "{path}");
        }
    }

    #[test]
    fn desktop_catalogs_are_also_compiled_into_the_shell() {
        let plan = push(&["apps/desktop/ui/locales/fr.json"]);
        assert!(plan.steps.contains(&Step::UiTests));
        assert!(plan.steps.contains(&Step::TestDesktop));
    }

    #[test]
    fn core_catalogs_are_also_read_by_the_ui_scripts() {
        let plan = push(&["crates/cuotai-core/locales/fr.json"]);
        assert!(plan.steps.contains(&Step::UiTests));
        assert!(plan.packages.contains(&Package::Core));
    }

    #[test]
    fn javascript_outside_the_documentation_starts_codeql() {
        for path in [
            "apps/desktop/ui/js/app.js",
            "scripts/test-meter-ui.mjs",
            "apps/desktop/tools/ui-shots/check.mjs",
        ] {
            assert!(push(&[path]).steps.contains(&Step::CodeqlJs), "{path}");
        }
        let site = push(&["site/app.js"]);
        assert!(!site.steps.contains(&Step::CodeqlJs));
        assert!(site.steps.contains(&Step::SiteBuild));
        assert!(!push(&["apps/desktop/ui/css/themes.css"])
            .steps
            .contains(&Step::CodeqlJs));
    }

    #[test]
    fn security_sensitive_rust_starts_codeql() {
        for path in [
            "crates/cuotai-core/src/oauth/session.rs",
            "crates/cuotai-core/src/oauth/store/keyring.rs",
            "crates/cuotai-core/src/serve/routes.rs",
            "crates/cuotai-core/src/secret_input.rs",
            "apps/desktop/src-tauri/src/updater.rs",
            "apps/desktop/src-tauri/capabilities/default.json",
        ] {
            assert!(push(&[path]).steps.contains(&Step::CodeqlRust), "{path}");
        }
        assert!(!push(&["crates/cuotai-core/src/model.rs"])
            .steps
            .contains(&Step::CodeqlRust));
    }

    #[test]
    fn lockfiles_start_cargo_deny() {
        let root = push(&["Cargo.lock"]);
        assert!(root.steps.contains(&Step::Deny));
        assert!(!root.steps.contains(&Step::DenyDesktop));
        assert_eq!(root.packages, Package::ALL);
        let desktop = push(&["apps/desktop/src-tauri/Cargo.lock"]);
        assert!(desktop.steps.contains(&Step::DenyDesktop));
        assert!(!desktop.steps.contains(&Step::Deny));
        assert!(desktop.steps.contains(&Step::TestDesktop));
        let config = push(&["deny.toml"]);
        assert!(config.steps.contains(&Step::Deny));
        assert!(config.steps.contains(&Step::DenyDesktop));
        assert!(!push(&["crates/cuotai-core/Cargo.toml"])
            .steps
            .contains(&Step::Deny));
    }

    #[test]
    fn workflows_run_their_policy_and_the_xtask_tests_that_embed_them() {
        let plan = push(&[".github/workflows/ci.yml"]);
        for step in [Step::WorkflowCheck, Step::Actionlint, Step::ReleaseTooling] {
            assert!(plan.steps.contains(&step), "{step:?}");
        }
        assert_eq!(plan.packages, [Package::Xtask]);
        assert!(!plan.steps.contains(&Step::SiteBuild));
        assert!(push(&[".github/workflows/pages.yml"])
            .steps
            .contains(&Step::SiteBuild));
    }

    #[test]
    fn the_site_build_follows_the_pages_inputs() {
        for path in [
            "site/index.html",
            "docs/assets/screenshots/tui.png",
            "scripts/site-build.sh",
        ] {
            assert!(push(&[path]).steps.contains(&Step::SiteBuild), "{path}");
        }
        assert!(!push(&["docs/assets/logo.png"])
            .steps
            .contains(&Step::SiteBuild));
    }

    #[test]
    fn the_widget_extension_builds_only_when_it_changes() {
        assert!(push(&["apps/desktop/widgets/macos/build-appex.sh"])
            .steps
            .contains(&Step::Widgets));
        assert!(!push(&["apps/desktop/src-tauri/src/widgets.rs"])
            .steps
            .contains(&Step::Widgets));
    }

    #[test]
    fn what_looks_like_documentation_but_is_code_runs_as_code() {
        // ci.yml: "Deliberately code, although they look like documentation".
        assert!(push(&["CHANGELOG.md"]).packages.contains(&Package::Core));
        assert!(push(&["docs/providers.md"])
            .packages
            .contains(&Package::Core));
        assert!(push(&["docs/export.schema.json"])
            .packages
            .contains(&Package::Core));
        assert_eq!(push(&["LICENSE-MIT"]).packages, Package::ALL);
        assert_eq!(push(&["THIRD_PARTY_NOTICES"]).packages, Package::ALL);
    }

    #[test]
    fn an_unknown_path_runs_every_rust_check() {
        for path in ["Makefile", "run.sh", "cuotai.example.toml", "Cargo.toml"] {
            let plan = push(&[path]);
            assert_eq!(plan.packages, Package::ALL, "{path}");
            assert!(plan.steps.contains(&Step::TestDesktop), "{path}");
        }
    }

    #[test]
    fn the_union_of_a_mixed_diff_is_selected() {
        let plan = push(&[
            "README.md",
            "crates/cuotai-tui/src/ui.rs",
            "xtask/src/git.rs",
            "apps/desktop/ui/js/app.js",
        ]);
        assert_eq!(plan.packages, [Package::Tui, Package::Xtask]);
        for step in [Step::UiTests, Step::CodeqlJs, Step::Clippy, Step::Test] {
            assert!(plan.steps.contains(&step), "{step:?}");
        }
        assert!(!plan.steps.contains(&Step::TestDesktop));
    }

    #[test]
    fn steps_run_in_declared_order_and_all_is_complete() {
        let mut declared = Step::ALL.to_vec();
        declared.sort();
        assert_eq!(declared, Step::ALL);
        declared.dedup();
        assert_eq!(declared.len(), Step::ALL.len());
    }

    #[test]
    fn full_has_no_filter() {
        let plan = for_full();
        assert_eq!(plan.packages, Package::ALL);
        assert_eq!(plan.steps.len(), Step::ALL.len() - 2);
        // Staged blobs do not exist here, and the xtask tests hold privacy.
        assert!(!plan.steps.contains(&Step::FmtStaged));
        assert!(!plan.steps.contains(&Step::Privacy));
    }

    #[test]
    fn skip_names_match_a_step_or_its_family() {
        let names = parse_skip("clippy, codeql-js");
        assert!(is_skipped(Step::Clippy, &names));
        assert!(is_skipped(Step::ClippyDesktop, &names));
        assert!(is_skipped(Step::CodeqlJs, &names));
        assert!(!is_skipped(Step::CodeqlRust, &names));
        assert!(!is_skipped(Step::Test, &names));
        assert!(is_skipped(Step::FmtStaged, &parse_skip("fmt")));
        assert!(unknown_skip_names(&names).is_empty());
        assert_eq!(unknown_skip_names(&parse_skip("clipy")), ["clipy"]);
    }

    // The `changes` job of ci.yml, read as data. A shell `case` tries its
    // patterns in order and `*` also matches `/`.

    const CI: &str = include_str!("../../../.github/workflows/ci.yml");

    fn glob(pattern: &str, text: &str) -> bool {
        match pattern.split_once('*') {
            None => pattern == text,
            Some((head, tail)) => text.strip_prefix(head).is_some_and(|rest| {
                (0..=rest.len())
                    .filter(|&i| rest.is_char_boundary(i))
                    .any(|i| glob(tail, &rest[i..]))
            }),
        }
    }

    /// `(patterns, kind)` of the `case "$file"` arms that set `kind=`.
    fn ci_kind_arms() -> Vec<(Vec<&'static str>, &'static str)> {
        CI.lines()
            .filter_map(|line| {
                let (patterns, action) = line.trim().split_once(") kind=")?;
                let kind = action.split_whitespace().next()?;
                Some((patterns.split(" | ").collect(), kind))
            })
            .collect()
    }

    fn ci_says_docs(arms: &[(Vec<&str>, &str)], path: &str) -> bool {
        let arm = arms
            .iter()
            .find(|(patterns, _)| patterns.iter().any(|pattern| glob(pattern, path)));
        arm.is_some_and(|(_, kind)| *kind == "docs")
    }

    fn ci_tooling_patterns() -> Vec<&'static str> {
        let line = CI
            .lines()
            .map(str::trim)
            .find(|line| line.starts_with("xtask/* |"))
            .expect("the release_tooling arm of ci.yml");
        line.trim_end_matches(')').split(" | ").collect()
    }

    /// Hand-picked edges plus every tracked file.
    fn paths_to_compare() -> Vec<String> {
        let mut paths = files(&[
            "README.md",
            "CONTRIBUTING.md",
            "SECURITY.md",
            "SUPPORT.md",
            "AGENTS.md",
            "CHANGELOG.md",
            "LICENSE-MIT",
            "THIRD_PARTY_NOTICES",
            "deny.toml",
            "docs/providers.md",
            "docs/architecture.md",
            "docs/plans/0039_x.md",
            "docs/export.schema.json",
            "docs/assets/logo.png",
            "apps/desktop/README.md",
            "apps/desktop/ui/README.md",
            "apps/desktop/ui/js/app.js",
            "apps/desktop/src-tauri/tauri.conf.json",
            "apps/desktop/src-tauri/tauri.updater.conf.json",
            "apps/desktop/src-tauri/Cargo.toml",
            ".github/dependabot.yml",
            ".github/pull_request_template.md",
            ".github/ISSUE_TEMPLATE/bug.yml",
            ".github/workflows/ci.yml",
            ".github/workflows/notes.md",
            "site/index.html",
            "site/app.js",
            "scripts/site-build.sh",
            "scripts/release/collect-linux.sh",
            "packaging/homebrew/cuotai.rb",
            "xtask/src/main.rs",
        ]);
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
        if let Ok(listing) = crate::git::run(std::path::Path::new(root), &["ls-files"]) {
            paths.extend(listing.lines().map(str::to_string));
        }
        paths
    }

    #[test]
    fn the_documentation_allow_list_matches_ci() {
        let arms = ci_kind_arms();
        assert!(arms.len() >= 3, "{arms:?}");
        for path in paths_to_compare() {
            assert_eq!(is_docs(&path), ci_says_docs(&arms, &path), "{path}");
        }
    }

    #[test]
    fn the_release_tooling_list_matches_ci() {
        let patterns = ci_tooling_patterns();
        for path in paths_to_compare() {
            let in_ci = patterns.iter().any(|pattern| glob(pattern, &path));
            assert_eq!(is_release_tooling(&path), in_ci, "{path}");
        }
    }

    #[test]
    fn every_documentation_path_is_free_of_rust_checks() {
        let arms = ci_kind_arms();
        for path in paths_to_compare() {
            if ci_says_docs(&arms, &path) {
                let plan = for_push(std::slice::from_ref(&path));
                assert!(plan.packages.is_empty(), "{path}");
                assert!(!plan.steps.contains(&Step::TestDesktop), "{path}");
            }
        }
    }
}
