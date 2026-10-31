//! `cargo xtask preflight`: the local run of what CI checks, the single source
//! the versioned hooks in `.githooks/` call. [`select`] decides what a change
//! needs, [`message`] checks commit messages, and this module runs the steps
//! and reports them. CI stays the authority: a bypassed or missing check here
//! only moves the failure to the pull request.
//!
//! A tool the repository does not require (see [`Tool`]) is reported and
//! skipped. A run stops at its first failure, so the expensive steps never run
//! for nothing, and the report lists what did not run.
//!
//! Every step runs without `GIT_*` in its environment. Git exports `GIT_DIR`
//! and `GIT_INDEX_FILE` to hooks, and a test that creates a temporary
//! repository would otherwise act on the real one.

pub mod message;
pub mod select;

use std::hash::{Hash, Hasher};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::{err, git, Error, Res};
use select::{Package, Plan, Step};

/// Bypasses steps by name, for example `CUOTAI_SKIP=clippy,test`.
const SKIP_VARIABLE: &str = "CUOTAI_SKIP";
const DESKTOP_MANIFEST: &str = "apps/desktop/src-tauri/Cargo.toml";
const DEFAULT_BASE: &str = "origin/main";
const NO_VERIFY_NOTE: &str = "`--no-verify` bypasses a hook entirely, and CI still applies.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Commit,
    Push,
    Full,
}

impl Mode {
    fn flag(self) -> &'static str {
        match self {
            Mode::Commit => "--commit",
            Mode::Push => "--push",
            Mode::Full => "--full",
        }
    }
}

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    CommitMessage(PathBuf),
    Run {
        mode: Mode,
        base: String,
        stdin_refs: bool,
        dry_run: bool,
    },
}

/// Parses the arguments after `preflight`.
pub fn parse_args(args: &[String]) -> Res<Request> {
    let mut mode = None;
    let mut message = None;
    let mut base = None;
    let mut stdin_refs = false;
    let mut dry_run = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--commit" => mode = Some(Mode::Commit),
            "--push" => mode = Some(Mode::Push),
            "--full" => mode = Some(Mode::Full),
            "--stdin-refs" => stdin_refs = true,
            "--dry-run" => dry_run = true,
            "--commit-msg" => {
                let file = it
                    .next()
                    .ok_or_else(|| Error::Msg("--commit-msg needs the message file".into()))?;
                message = Some(PathBuf::from(file));
            }
            "--base" => {
                let reference = it
                    .next()
                    .ok_or_else(|| Error::Msg("--base needs a reference".into()))?;
                base = Some(reference.clone());
            }
            other => return err(format!("unknown preflight argument '{other}'")),
        }
    }
    const USAGE: &str =
        "usage: cargo xtask preflight --commit | --commit-msg FILE | --push | --full";
    match (message, mode) {
        (Some(file), None) => {
            if base.is_some() || stdin_refs || dry_run {
                return err("--commit-msg takes no other option");
            }
            Ok(Request::CommitMessage(file))
        }
        (None, Some(mode)) => {
            if mode != Mode::Push && (base.is_some() || stdin_refs) {
                return err("--base and --stdin-refs apply only to --push");
            }
            Ok(Request::Run {
                mode,
                base: base.unwrap_or_else(|| DEFAULT_BASE.to_string()),
                stdin_refs,
                dry_run,
            })
        }
        _ => err(USAGE),
    }
}

/// The branch tips of the pre-push ref lines
/// (`<local ref> <local sha> <remote ref> <remote sha>`). Deleted refs and
/// tags have no pull request to prepare for: `release.yml` owns the tags.
pub fn pushed_heads(lines: &str) -> Vec<String> {
    lines
        .lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let (reference, sha) = (fields.next()?, fields.next()?);
            let deleted = sha.bytes().all(|b| b == b'0');
            (reference.starts_with("refs/heads/") && !deleted).then(|| sha.to_string())
        })
        .collect()
}

/// A tool a step needs and the repository does not require.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tool {
    Actionlint,
    Bash,
    Browser,
    CargoDeny,
    Codeql,
    Jq,
    Node,
    Python,
    Sh,
    Xcode,
}

impl Tool {
    fn describe(self) -> &'static str {
        match self {
            Tool::Actionlint => "actionlint not found (brew install actionlint)",
            Tool::Bash => "bash not found",
            Tool::Browser => "Chrome not found (set CHROME to its path)",
            Tool::CargoDeny => "cargo-deny not found (cargo install cargo-deny --locked)",
            Tool::Codeql => "codeql not found (see scripts/codeql-local.sh)",
            Tool::Jq => "jq not found",
            Tool::Node => "node not found",
            Tool::Python => "python3 not found",
            Tool::Sh => "sh not found (this step needs a POSIX shell)",
            Tool::Xcode => "full Xcode not found (this step needs macOS)",
        }
    }

    fn available(self) -> bool {
        match self {
            Tool::Actionlint => on_path("actionlint"),
            Tool::Bash => on_path("bash"),
            Tool::Browser => browser_available(),
            Tool::CargoDeny => on_path("cargo-deny"),
            Tool::Codeql => on_path("codeql"),
            Tool::Jq => on_path("jq"),
            Tool::Node => on_path("node"),
            Tool::Python => on_path("python3"),
            Tool::Sh => on_path("sh"),
            Tool::Xcode => {
                cfg!(target_os = "macos")
                    && Command::new("xcodebuild")
                        .arg("-version")
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status()
                        .is_ok_and(|status| status.success())
            }
        }
    }
}

fn on_path(name: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    let executable = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    std::env::split_paths(&path).any(|dir| dir.join(&executable).is_file())
}

/// The same lookup as `apps/desktop/tools/ui-shots/driver.mjs`.
fn browser_available() -> bool {
    if std::env::var_os("CHROME").is_some() {
        return true;
    }
    if cfg!(target_os = "macos") {
        Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome").is_file()
    } else if cfg!(windows) {
        Path::new("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe").is_file()
    } else {
        on_path("google-chrome")
    }
}

/// The optional tools of a step.
fn tools(step: Step) -> &'static [Tool] {
    match step {
        Step::Actionlint => &[Tool::Actionlint],
        Step::Deny | Step::DenyDesktop => &[Tool::CargoDeny],
        Step::SiteBuild => &[Tool::Sh, Tool::Python],
        Step::ReleaseTooling => &[Tool::Bash, Tool::Jq],
        Step::UiLint => &[Tool::Python],
        Step::UiTests => &[Tool::Node],
        Step::UiCheck | Step::UiBrowser => &[Tool::Node, Tool::Python, Tool::Browser],
        Step::Widgets => &[Tool::Sh, Tool::Xcode],
        Step::CodeqlJs | Step::CodeqlRust | Step::CodeqlOther => &[Tool::Sh, Tool::Codeql],
        _ => &[],
    }
}

/// One process of a step.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Task {
    program: PathBuf,
    args: Vec<String>,
    env: Vec<(String, String)>,
    /// Exit codes that pass (`[0]` for almost everything).
    pass: &'static [i32],
    /// Exit codes that mean "could not check": reported, not a failure.
    inconclusive: &'static [i32],
}

impl Task {
    fn new(program: impl Into<PathBuf>, args: &[&str]) -> Task {
        Task {
            program: program.into(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
            env: Vec::new(),
            pass: &[0],
            inconclusive: &[],
        }
    }

    fn cargo(args: &[&str]) -> Task {
        Task::new("cargo", args)
    }

    fn describe(&self) -> String {
        let mut line = self.program.display().to_string();
        for arg in &self.args {
            line.push(' ');
            line.push_str(arg);
        }
        line
    }
}

/// How a step ended.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Outcome {
    Passed,
    /// Ran, but could not reach a verdict (a CodeQL tool error, a CDN).
    Inconclusive(String),
    Failed(String),
    Skipped(String),
    NotRun,
}

/// What a run knows about its change.
struct Context {
    root: PathBuf,
    mode: Mode,
    base: String,
    head: String,
    files: Vec<String>,
    packages: Vec<Package>,
    /// This binary: its subcommands run without another `cargo run`.
    xtask: PathBuf,
}

/// `--workspace`, or `-p` per package when the plan is narrower.
fn package_args(packages: &[Package]) -> Vec<String> {
    if packages.len() == Package::ALL.len() {
        return vec!["--workspace".to_string()];
    }
    packages
        .iter()
        .flat_map(|package| ["-p".to_string(), package.name().to_string()])
        .collect()
}

fn workflows(context: &Context) -> Vec<String> {
    if context.mode == Mode::Commit {
        return context
            .files
            .iter()
            .filter(|path| select::is_workflow(path) && context.root.join(path).is_file())
            .cloned()
            .collect();
    }
    let dir = context.root.join(".github/workflows");
    let mut found: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| format!(".github/workflows/{}", entry.file_name().to_string_lossy()))
        .filter(|path| select::is_workflow(path))
        .collect();
    found.sort();
    found
}

fn ui_test_scripts(root: &Path) -> Vec<String> {
    let mut found: Vec<String> = std::fs::read_dir(root.join("scripts"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("test-") && name.ends_with("-ui.mjs"))
        .map(|name| format!("scripts/{name}"))
        .collect();
    found.sort();
    found
}

/// CodeQL through its script, in a directory this worktree reuses: the
/// script's default is a new directory per run, which a hook would pile up.
fn codeql(context: &Context, languages: &[&str]) -> Task {
    let mut args = vec!["scripts/codeql-local.sh"];
    args.extend_from_slice(languages);
    let mut task = Task::new("sh", &args);
    task.inconclusive = &[1];
    if std::env::var_os("CUOTAI_CODEQL_DIR").is_none() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        context.root.hash(&mut hasher);
        let dir =
            std::env::temp_dir().join(format!("cuotai-preflight-codeql-{:016x}", hasher.finish()));
        task.env.push((
            "CUOTAI_CODEQL_DIR".to_string(),
            dir.to_string_lossy().into_owned(),
        ));
    }
    task
}

/// The processes of a step, or `None` for a step this module runs itself.
fn tasks(step: Step, context: &Context) -> Option<Vec<Task>> {
    let xtask = |args: &[&str]| Task::new(context.xtask.clone(), args);
    let with_manifest = |mut prefix: Vec<&'static str>, rest: &[&'static str]| {
        prefix.extend_from_slice(&["--manifest-path", DESKTOP_MANIFEST]);
        prefix.extend_from_slice(rest);
        Task::cargo(&prefix)
    };
    let packages = package_args(&context.packages);
    let cargo_on_packages = |command: &str, rest: &[&str]| {
        let mut args: Vec<&str> = vec![command];
        args.extend(packages.iter().map(String::as_str));
        args.extend_from_slice(rest);
        Task::cargo(&args)
    };
    let list = match step {
        Step::CommitPolicy | Step::FmtStaged | Step::Widgets => return None,
        Step::Fmt => vec![Task::cargo(&["fmt", "--all", "--", "--check"])],
        Step::FmtDesktop => vec![with_manifest(vec!["fmt"], &["--", "--check"])],
        Step::ChangelogGate => {
            vec![xtask(&[
                "changelog-gate",
                "--base",
                &context.base,
                "--head",
                &context.head,
            ])]
        }
        Step::ChangelogCheck => vec![xtask(&["changelog-check"])],
        Step::ReleaseCheck => vec![xtask(&["release-check"])],
        Step::WorkflowCheck => vec![xtask(&["workflow-check"])],
        Step::Actionlint => {
            let files = workflows(context);
            let mut args = vec!["-color"];
            args.extend(files.iter().map(String::as_str));
            vec![Task::new("actionlint", &args)]
        }
        Step::Privacy => vec![Task::cargo(&[
            "test", "-q", "-p", "xtask", "--locked", "--test", "privacy",
        ])],
        Step::ReleaseTooling => vec![Task::new(
            "bash",
            &["scripts/release/test-collect-linux.sh"],
        )],
        Step::Deny => vec![Task::cargo(&["deny", "--config", "deny.toml", "check"])],
        Step::DenyDesktop => vec![with_manifest(
            vec!["deny"],
            &["--config", "deny.toml", "check"],
        )],
        Step::SiteBuild => vec![Task::new("sh", &["scripts/site-build.sh"])],
        Step::UiLint => {
            let mut list = vec![
                Task::new("python3", &["apps/desktop/tools/lint_ui.py"]),
                Task::new("python3", &["apps/desktop/tools/contrast_gate.py"]),
            ];
            let themes = context.root.join("apps/desktop/ui/css/themes");
            let mut names: Vec<String> = std::fs::read_dir(themes)
                .into_iter()
                .flatten()
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .filter(|name| name.ends_with(".css") && name != "index.css")
                .collect();
            names.sort();
            for name in names {
                let theme = format!("apps/desktop/ui/css/themes/{name}");
                list.push(Task::new(
                    "python3",
                    &["apps/desktop/tools/lint_theme.py", &theme],
                ));
            }
            list
        }
        Step::UiTests => ui_test_scripts(&context.root)
            .iter()
            .map(|script| Task::new("node", &[script]))
            .collect(),
        Step::UiCheck => vec![Task::new(
            "node",
            &["apps/desktop/tools/ui-shots/check.mjs"],
        )],
        Step::UiBrowser => {
            const SHOTS: &str = "apps/desktop/tools/ui-shots";
            let script = |name: &str| format!("{SHOTS}/{name}");
            let mut a11y = Task::new("node", &[&script("a11y.mjs")]);
            // Exit 3: axe-core could not be fetched from its CDN.
            a11y.inconclusive = &[3];
            vec![
                Task::new("node", &[&script("check.mjs"), "--zoom", "en-US", "pt-BR"]),
                Task::new("node", &[&script("check.mjs"), "--selftest"]),
                Task::new("node", &[&script("locale-smoke.mjs")]),
                Task::new("node", &[&script("keyboard.mjs")]),
                a11y,
                Task::new("node", &[&script("perf.mjs"), "--strict"]),
            ]
        }
        Step::Clippy => vec![cargo_on_packages(
            "clippy",
            &["--all-targets", "--locked", "--", "-D", "warnings"],
        )],
        Step::ClippyDesktop => vec![with_manifest(
            vec!["clippy"],
            &["--all-targets", "--locked", "--", "-D", "warnings"],
        )],
        Step::Test => vec![cargo_on_packages("test", &["--locked"])],
        Step::TestDesktop => vec![with_manifest(vec!["test"], &["--locked"])],
        Step::TuiSmoke => {
            let binary = format!("target/debug/cuotai{}", std::env::consts::EXE_SUFFIX);
            let mut once = Task::new(
                context.root.join(&binary),
                &[
                    "--config",
                    "crates/cuotai-tui/tests/fixtures/smoke.toml",
                    "--once",
                    "--no-status",
                ],
            );
            // Every fixture account is missing its credential: exit 2.
            once.pass = &[2];
            vec![
                Task::cargo(&["build", "-p", "cuotai-tui", "--locked"]),
                Task::new(context.root.join(&binary), &["--list-providers"]),
                once,
            ]
        }
        Step::CodeqlJs => vec![codeql(context, &["javascript-typescript"])],
        Step::CodeqlRust => vec![codeql(context, &["rust"])],
        Step::CodeqlOther => vec![codeql(context, &["python", "actions"])],
    };
    Some(list)
}

fn command(task: &Task, root: &Path) -> Command {
    let mut command = Command::new(&task.program);
    command
        .args(&task.args)
        .current_dir(root)
        .stdin(Stdio::null());
    for (name, _) in std::env::vars_os() {
        if name.to_string_lossy().starts_with("GIT_") {
            command.env_remove(name);
        }
    }
    command.envs(task.env.iter().map(|(name, value)| (name, value)));
    command
}

fn run_task(task: &Task, root: &Path) -> Outcome {
    let status = match command(task, root).status() {
        Ok(status) => status,
        Err(e) => return Outcome::Failed(format!("cannot run {}: {e}", task.describe())),
    };
    match status.code() {
        Some(code) if task.pass.contains(&code) => Outcome::Passed,
        Some(code) if task.inconclusive.contains(&code) => {
            Outcome::Inconclusive(format!("{} exited {code}", task.describe()))
        }
        Some(code) => Outcome::Failed(format!("{} exited {code}", task.describe())),
        None => Outcome::Failed(format!("{} was killed by a signal", task.describe())),
    }
}

fn run_tasks(list: &[Task], root: &Path) -> Outcome {
    let mut outcome = Outcome::Passed;
    for task in list {
        match run_task(task, root) {
            Outcome::Passed => {}
            inconclusive @ Outcome::Inconclusive(_) => outcome = inconclusive,
            failed => return failed,
        }
    }
    outcome
}

/// `rustfmt --check` of the blobs about to be committed, not of the working
/// tree, so an unstaged edit neither hides a problem nor reports one. Reading
/// standard input, rustfmt does not follow `mod` declarations.
fn fmt_staged(context: &Context) -> Outcome {
    let staged = git::run(
        &context.root,
        &[
            "diff",
            "--cached",
            "--name-only",
            "--no-renames",
            "--diff-filter=d",
            "-z",
            "--",
            "*.rs",
        ],
    );
    let staged = match staged {
        Ok(listing) => message::split_paths(&listing),
        Err(e) => return Outcome::Failed(e.to_string()),
    };
    let mut bad = Vec::new();
    for path in staged {
        let blob = match git::run(&context.root, &["show", &format!(":{path}")]) {
            Ok(blob) => blob,
            Err(e) => return Outcome::Failed(e.to_string()),
        };
        match rustfmt_check(&context.root, &blob) {
            Ok(None) => {}
            Ok(Some(diff)) => {
                println!("{path}:\n{diff}");
                bad.push(path);
            }
            Err(e) => return Outcome::Failed(e),
        }
    }
    if bad.is_empty() {
        return Outcome::Passed;
    }
    Outcome::Failed(format!(
        "not formatted: {}. Run `cargo fmt --all` and stage the result",
        bad.join(", ")
    ))
}

/// The diff `rustfmt --check` reports for `source`, if any.
fn rustfmt_check(root: &Path, source: &str) -> Result<Option<String>, String> {
    let mut child = Command::new("rustfmt")
        .args(["--check", "--edition", "2021"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("cannot run rustfmt: {e} (rustup component add rustfmt)"))?;
    let mut stdin = child.stdin.take().ok_or("rustfmt has no standard input")?;
    let source = source.to_string();
    // Written from another thread: rustfmt may fill its output pipe first.
    let writer = std::thread::spawn(move || stdin.write_all(source.as_bytes()));
    let output = child
        .wait_with_output()
        .map_err(|e| format!("cannot read rustfmt: {e}"))?;
    let _ = writer.join();
    if output.status.success() {
        return Ok(None);
    }
    let mut report = String::from_utf8_lossy(&output.stdout).into_owned();
    report.push_str(&String::from_utf8_lossy(&output.stderr));
    Ok(Some(report))
}

/// Builds the extension in a temporary directory: its script ends by proving
/// the entry point of both architectures.
fn widgets(context: &Context) -> Outcome {
    let out = std::env::temp_dir().join(format!("cuotai-preflight-appex-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&out) {
        return Outcome::Failed(format!("cannot create {}: {e}", out.display()));
    }
    let task = Task::new(
        "sh",
        &[
            "apps/desktop/widgets/macos/build-appex.sh",
            &out.to_string_lossy(),
            "0.0.0",
        ],
    );
    let outcome = run_task(&task, &context.root);
    let _ = std::fs::remove_dir_all(&out);
    outcome
}

fn run_step(step: Step, context: &Context) -> Outcome {
    match step {
        Step::CommitPolicy => {
            match message::check_range(&context.root, &context.base, &context.head) {
                Ok(_) => Outcome::Passed,
                Err(e) => Outcome::Failed(e.to_string()),
            }
        }
        Step::FmtStaged => fmt_staged(context),
        Step::Widgets => widgets(context),
        _ => {
            let list = tasks(step, context).unwrap_or_default();
            if list.is_empty() {
                return Outcome::Skipped("nothing to check".to_string());
            }
            run_tasks(&list, &context.root)
        }
    }
}

fn clock(elapsed: Duration) -> String {
    let seconds = elapsed.as_secs_f64();
    if seconds < 60.0 {
        format!("{seconds:.1}s")
    } else {
        format!("{}m{:02}s", elapsed.as_secs() / 60, elapsed.as_secs() % 60)
    }
}

/// The reason `step` does not run, if any.
fn skip_reason(step: Step, skipped: &[String]) -> Option<String> {
    if select::is_skipped(step, skipped) {
        return Some(format!("bypassed with {SKIP_VARIABLE}"));
    }
    tools(step)
        .iter()
        .find(|tool| !tool.available())
        .map(|tool| tool.describe().to_string())
}

/// Runs `plan` in order, prints each step and the final report, and fails
/// when a step failed.
fn execute(context: &Context, plan: &Plan, skipped: &[String]) -> Res<()> {
    let total = plan.steps.len();
    let started = Instant::now();
    let mut results: Vec<(Step, Outcome, Duration)> = Vec::new();
    let mut stopped = false;
    for (index, step) in plan.steps.iter().copied().enumerate() {
        if stopped {
            results.push((step, Outcome::NotRun, Duration::ZERO));
            continue;
        }
        let id = step.id();
        if let Some(reason) = skip_reason(step, skipped) {
            println!("[{}/{total}] {id}: skipped, {reason}", index + 1);
            results.push((step, Outcome::Skipped(reason), Duration::ZERO));
            continue;
        }
        println!("[{}/{total}] {id}", index + 1);
        let clock_start = Instant::now();
        let outcome = run_step(step, context);
        stopped = matches!(outcome, Outcome::Failed(_));
        results.push((step, outcome, clock_start.elapsed()));
    }

    println!("\npreflight {} report:", context.mode.flag());
    let mut warnings = Vec::new();
    let mut failure = None;
    for (step, outcome, elapsed) in &results {
        let id = step.id();
        let time = clock(*elapsed);
        match outcome {
            Outcome::Passed => println!("  ok       {id} ({time})"),
            Outcome::Inconclusive(reason) => {
                println!("  warning  {id} ({time})");
                warnings.push(format!("{id} reached no verdict: {reason}"));
            }
            Outcome::Failed(reason) => {
                println!("  FAILED   {id} ({time})");
                failure = Some(format!("{id}: {reason}"));
            }
            Outcome::Skipped(reason) => {
                println!("  skipped  {id}");
                warnings.push(format!("{id} skipped: {reason}"));
            }
            Outcome::NotRun => println!("  not run  {id}"),
        }
    }
    println!("  total {}", clock(started.elapsed()));
    for warning in &warnings {
        println!("warning: {warning}");
    }
    if !warnings.is_empty() {
        println!("Skipped checks still run in CI.");
    }
    match failure {
        None => Ok(()),
        Some(failure) => err(format!(
            "preflight {} failed at {failure}\n{SKIP_VARIABLE}=<step> bypasses one step. {NO_VERIFY_NOTE}",
            context.mode.flag()
        )),
    }
}

fn dry_run(context: &Context, plan: &Plan, skipped: &[String]) {
    for step in &plan.steps {
        let reason = skip_reason(*step, skipped)
            .map(|reason| format!(" (skipped: {reason})"))
            .unwrap_or_default();
        println!("  {}{reason}", step.id());
    }
    if !plan.packages.is_empty() {
        println!("  packages: {}", package_args(&plan.packages).join(" "));
    }
    println!("dry run: {} ran nothing", context.mode.flag());
}

/// What `--push` compares: the merge base, as CI's `base...head` does. `None`
/// means the diff cannot be computed.
fn changed_files(root: &Path, base: &str, head: &str) -> Option<Vec<String>> {
    let range = format!("{base}...{head}");
    git::run(
        root,
        &["diff", "-z", "--name-only", "--no-renames", &range, "--"],
    )
    .ok()
    .map(|listing| message::split_paths(&listing))
}

fn read_stdin() -> Res<String> {
    let mut text = String::new();
    for line in std::io::stdin().lock().lines() {
        text.push_str(&line?);
        text.push('\n');
    }
    Ok(text)
}

fn root() -> Res<PathBuf> {
    let top = git::run(Path::new("."), &["rev-parse", "--show-toplevel"])?;
    Ok(PathBuf::from(top.trim()))
}

fn skip_names() -> Vec<String> {
    let value = std::env::var(SKIP_VARIABLE).unwrap_or_default();
    let names = select::parse_skip(&value);
    for name in select::unknown_skip_names(&names) {
        println!("warning: {SKIP_VARIABLE} names no step called '{name}'");
    }
    names
}

fn check_message_file(root: &Path, file: &Path) -> Res<()> {
    if select::is_skipped(Step::CommitPolicy, &skip_names()) {
        println!("commit-msg skipped: bypassed with {SKIP_VARIABLE}. CI still applies.");
        return Ok(());
    }
    // The hook may name the file relative to the top of the worktree.
    let file = if file.is_absolute() {
        file.to_path_buf()
    } else {
        root.join(file)
    };
    let result = message::check_file(root, &file);
    if result.is_err() {
        eprintln!("{SKIP_VARIABLE}=commit-policy bypasses this check. {NO_VERIFY_NOTE}");
    }
    result
}

/// `cargo xtask preflight`.
pub fn run(args: &[String]) -> Res<()> {
    let request = parse_args(args)?;
    let root = root()?;
    let (mode, base, stdin_refs, is_dry_run) = match request {
        Request::CommitMessage(file) => return check_message_file(&root, &file),
        Request::Run {
            mode,
            base,
            stdin_refs,
            dry_run,
        } => (mode, base, stdin_refs, dry_run),
    };
    let skipped = skip_names();

    let mut head = "HEAD".to_string();
    if stdin_refs {
        let current = git::run(&root, &["rev-parse", "HEAD"])?;
        let heads = pushed_heads(&read_stdin()?);
        let Some(first) = heads.first() else {
            println!("preflight --push: no branch in this push, nothing to check");
            return Ok(());
        };
        if heads.len() > 1 || first != current.trim() {
            println!(
                "warning: this push is not just HEAD; Clippy and the tests see the working tree"
            );
        }
        head = first.clone();
    }

    let (files, mut plan) = match mode {
        Mode::Commit => {
            let files = message::staged_files(&root)?;
            let plan = select::for_commit(&files);
            (files, plan)
        }
        Mode::Full => (Vec::new(), select::for_full()),
        Mode::Push => match changed_files(&root, &base, &head) {
            Some(files) => {
                let plan = select::for_push(&files);
                (files, plan)
            }
            None => {
                println!(
                    "warning: cannot diff against {base}; running everything, as CI does when \
                     a diff cannot be computed"
                );
                (Vec::new(), select::for_full())
            }
        },
    };

    if mode != Mode::Commit {
        let known = git::run(&root, &["rev-parse", "--verify", "--quiet", &base]).is_ok();
        if !known {
            println!("warning: {base} does not exist here; the commit rules of the range are not checked");
            plan.steps
                .retain(|step| !matches!(step, Step::ChangelogGate | Step::CommitPolicy));
        }
        let status = git::run(&root, &["status", "--porcelain"]).unwrap_or_default();
        if !status.trim().is_empty() {
            println!("warning: uncommitted changes: these checks see the working tree");
        }
    }
    println!(
        "preflight {}: {} step(s) for {} file(s)",
        mode.flag(),
        plan.steps.len(),
        files.len()
    );

    let context = Context {
        root,
        mode,
        base,
        head,
        files,
        packages: plan.packages.clone(),
        xtask: std::env::current_exe()?,
    };
    if is_dry_run {
        dry_run(&context, &plan, &skipped);
        return Ok(());
    }
    execute(&context, &plan, &skipped)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    fn context(mode: Mode, packages: &[Package]) -> Context {
        Context {
            root: PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/..")),
            mode,
            base: "origin/main".to_string(),
            head: "HEAD".to_string(),
            files: Vec::new(),
            packages: packages.to_vec(),
            xtask: PathBuf::from("xtask"),
        }
    }

    fn first_task(step: Step, packages: &[Package]) -> Task {
        tasks(step, &context(Mode::Push, packages)).unwrap()[0].clone()
    }

    #[test]
    fn modes_parse() {
        let run = |mode| Request::Run {
            mode,
            base: "origin/main".to_string(),
            stdin_refs: false,
            dry_run: false,
        };
        assert_eq!(parse_args(&args(&["--commit"])).unwrap(), run(Mode::Commit));
        assert_eq!(parse_args(&args(&["--push"])).unwrap(), run(Mode::Push));
        assert_eq!(parse_args(&args(&["--full"])).unwrap(), run(Mode::Full));
        assert_eq!(
            parse_args(&args(&["--commit-msg", "file"])).unwrap(),
            Request::CommitMessage(PathBuf::from("file"))
        );
        assert_eq!(
            parse_args(&args(&["--push", "--base", "main", "--stdin-refs"])).unwrap(),
            Request::Run {
                mode: Mode::Push,
                base: "main".to_string(),
                stdin_refs: true,
                dry_run: false,
            }
        );
    }

    #[test]
    fn bad_command_lines_fail() {
        for bad in [
            &[][..],
            &["--nope"],
            &["--commit-msg"],
            &["--base"],
            &["--commit", "--base", "x"],
            &["--commit-msg", "file", "--commit"],
            &["--commit-msg", "file", "--dry-run"],
        ] {
            assert!(parse_args(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn only_branch_updates_are_preflighted() {
        let sha = "a".repeat(40);
        let zero = "0".repeat(40);
        let branch = format!("refs/heads/feat/x {sha} refs/heads/feat/x {zero}\n");
        assert_eq!(pushed_heads(&branch), std::slice::from_ref(&sha));
        let tag = format!("refs/tags/v1.2.3 {sha} refs/tags/v1.2.3 {zero}\n");
        assert!(pushed_heads(&tag).is_empty());
        let deleted = format!("(delete) {zero} refs/heads/feat/x {sha}\n");
        assert!(pushed_heads(&deleted).is_empty());
        let zero_branch = format!("refs/heads/feat/x {zero} refs/heads/feat/x {sha}\n");
        assert!(pushed_heads(&zero_branch).is_empty());
        assert!(pushed_heads("").is_empty());
    }

    #[test]
    fn narrow_plans_name_packages_and_the_whole_set_is_the_workspace() {
        assert_eq!(package_args(&Package::ALL), ["--workspace"]);
        assert_eq!(
            package_args(&[Package::Core, Package::Tui]),
            ["-p", "cuotai-core", "-p", "cuotai-tui"]
        );
        let clippy = first_task(Step::Clippy, &[Package::Xtask]);
        assert_eq!(
            clippy.describe(),
            "cargo clippy -p xtask --all-targets --locked -- -D warnings"
        );
    }

    #[test]
    fn rust_steps_match_ci() {
        assert_eq!(
            first_task(Step::Clippy, &Package::ALL).describe(),
            "cargo clippy --workspace --all-targets --locked -- -D warnings"
        );
        assert_eq!(
            first_task(Step::Test, &Package::ALL).describe(),
            "cargo test --workspace --locked"
        );
        assert_eq!(
            first_task(Step::ClippyDesktop, &[]).describe(),
            format!(
                "cargo clippy --manifest-path {DESKTOP_MANIFEST} --all-targets --locked -- -D warnings"
            )
        );
        assert_eq!(
            first_task(Step::TestDesktop, &[]).describe(),
            format!("cargo test --manifest-path {DESKTOP_MANIFEST} --locked")
        );
    }

    #[test]
    fn the_gate_uses_the_range_of_the_run() {
        assert_eq!(
            first_task(Step::ChangelogGate, &[]).describe(),
            "xtask changelog-gate --base origin/main --head HEAD"
        );
    }

    #[test]
    fn codeql_findings_fail_and_tool_errors_do_not() {
        let task = first_task(Step::CodeqlJs, &[]);
        assert_eq!(task.pass, [0]);
        assert_eq!(task.inconclusive, [1]);
        assert!(task.describe().ends_with("javascript-typescript"));
    }

    #[test]
    fn every_ui_script_ci_runs_is_found() {
        const CI: &str = include_str!("../../../.github/workflows/ci.yml");
        let found = ui_test_scripts(&context(Mode::Push, &[]).root);
        let listed: Vec<&str> = CI
            .lines()
            .filter_map(|line| line.trim().strip_prefix("- run: node "))
            .filter(|script| script.starts_with("scripts/test-"))
            .collect();
        assert!(listed.len() > 20, "{listed:?}");
        for script in listed {
            assert!(found.iter().any(|path| path == script), "{script}");
        }
    }

    #[test]
    fn workflows_are_all_of_them_on_push_and_the_staged_ones_on_commit() {
        let on_push = workflows(&context(Mode::Push, &[]));
        assert!(on_push.iter().any(|path| path.ends_with("/ci.yml")));
        let mut staged = context(Mode::Commit, &[]);
        staged.files = vec![
            ".github/workflows/ci.yml".to_string(),
            ".github/workflows/deleted.yml".to_string(),
            "README.md".to_string(),
        ];
        assert_eq!(workflows(&staged), [".github/workflows/ci.yml"]);
    }

    #[test]
    fn steps_never_inherit_git_variables() {
        std::env::set_var("GIT_DIR_PREFLIGHT_TEST", "x");
        let built = command(&Task::new("true", &[]), Path::new("."));
        let removed = built
            .get_envs()
            .any(|(name, value)| name == "GIT_DIR_PREFLIGHT_TEST" && value.is_none());
        std::env::remove_var("GIT_DIR_PREFLIGHT_TEST");
        assert!(removed);
    }

    #[test]
    fn durations_read_well() {
        assert_eq!(clock(Duration::from_millis(1250)), "1.2s");
        assert_eq!(clock(Duration::from_secs(72)), "1m12s");
    }

    #[test]
    fn a_missing_tool_is_a_reason_and_a_step_without_tools_has_none() {
        assert!(skip_reason(Step::Clippy, &[]).is_none());
        let reason = skip_reason(Step::Clippy, &["clippy".to_string()]).unwrap();
        assert!(reason.contains(SKIP_VARIABLE), "{reason}");
        for step in Step::ALL {
            assert!(tools(step).iter().all(|tool| !tool.describe().is_empty()));
        }
    }
}
