//! PRD 0017 RF3: the isolated runner — reduced and approved-only
//! environment, closed `stdin`, cwd and argv exact, time limit killing the
//! whole process tree, bounded output without deadlock, at most
//! [`MAX_RUNNING`] at once, per-extension deadline and backoff.
//!
//! Real execution uses `#!/bin/sh` fixtures, so those tests are Unix-only
//! (Windows covers spawn/timeout with a copied `ping.exe`); the schedule,
//! the environment filter and the consent gate run everywhere.

use super::super::consent::{Approved, Consent};
use super::super::{discover, Manifest, MANIFEST_FILE};
use super::*;
use std::fs;
use std::path::{Path, PathBuf};

/// A sandbox: an extensions `root`, its `consent` and a runner whose
/// parent environment is the given one.
struct Rig {
    _dir: tempfile::TempDir,
    root: PathBuf,
    consent: Consent,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("extensions");
        fs::create_dir_all(&root).unwrap();
        let consent = Consent::at(dir.path().join("consent.json"));
        Self {
            _dir: dir,
            root,
            consent,
        }
    }

    fn path(&self, name: &str) -> PathBuf {
        self._dir.path().join(name)
    }

    /// Approve the extension `id` whose `run.sh` holds `script`.
    fn approve(&self, id: &str, script: &str, extra_toml: &str) -> Approved {
        self.approve_with_args(id, script, &[], extra_toml)
    }

    /// [`Self::approve`] with `args` added to the command vector.
    fn approve_with_args(
        &self,
        id: &str,
        script: &str,
        args: &[&str],
        extra_toml: &str,
    ) -> Approved {
        let dir = self.root.join(id);
        fs::create_dir_all(&dir).unwrap();
        let mut command = vec!["./run.sh".to_string()];
        command.extend(args.iter().map(|arg| arg.to_string()));
        let manifest = format!(
            "id = \"{id}\"\nname = \"{id}\"\nversion = \"1\"\nschema = 1\n\
             command = {}\ninterval_seconds = 60\n{extra_toml}",
            serde_json::to_string(&command).unwrap()
        );
        fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
        let program = dir.join("run.sh");
        fs::write(&program, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let discovery = discover(&self.root);
        assert!(discovery.rejected.is_empty(), "{:?}", discovery.rejected);
        let manifest: Manifest = discovery.get(id).expect("discovered").clone();
        self.consent.approve(&manifest).unwrap()
    }
}

/// A runner with a fixed parent environment: the allowed variables, one
/// the manifest asks for and one nobody may see.
fn runner() -> Runner {
    Runner::with_parent_env(
        [
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/home/someone"),
            ("LANG", "en_US.UTF-8"),
            ("APPROVED_VAR", "approved-marker"),
            ("SECRET_VAR", "secret-marker"),
        ]
        .into_iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value))),
    )
}

fn note_ids(reading: &Reading) -> Vec<&str> {
    reading
        .checked
        .notes
        .iter()
        .map(|note| note.id.as_ref())
        .collect()
}

/// The manifest's `env` asks for one variable the parent has and one it
/// has not; the child receives the inherited ones plus the approved one,
/// never anything else.
#[test]
fn the_child_environment_is_reduced_to_the_inherited_and_the_approved() {
    let rig = Rig::new();
    let approved = rig.approve(
        "env",
        "#!/bin/sh\n",
        "env = [\"APPROVED_VAR\", \"MISSING_VAR\"]\n",
    );
    let parent: BTreeMap<OsString, OsString> = [
        ("PATH", "/usr/bin:/bin"),
        ("HOME", "/home/someone"),
        ("LANG", "en_US.UTF-8"),
        ("APPROVED_VAR", "approved-marker"),
        ("SECRET_VAR", "secret-marker"),
    ]
    .into_iter()
    .map(|(name, value)| (OsString::from(name), OsString::from(value)))
    .collect();
    let env: BTreeMap<OsString, OsString> = child_env(&parent, &approved).into_iter().collect();
    let expected = BTreeMap::from([
        (OsString::from("PATH"), OsString::from("/usr/bin:/bin")),
        (OsString::from("HOME"), OsString::from("/home/someone")),
        (OsString::from("LANG"), OsString::from("en_US.UTF-8")),
        (
            OsString::from("APPROVED_VAR"),
            OsString::from("approved-marker"),
        ),
    ]);
    assert_eq!(env, expected);
}

/// A parent environment spelled `Path` and an approved name requested in
/// another case resolve to the same variable on Windows (variable names
/// are case-insensitive there) and do not on Unix (names are exact).
#[test]
fn the_variable_lookup_follows_the_platform_spelling_rules() {
    let rig = Rig::new();
    let approved = rig.approve("case", "#!/bin/sh\n", "env = [\"approved_var\"]\n");
    let parent: BTreeMap<OsString, OsString> =
        [("Path", "/usr/bin"), ("approved_var", "approved-marker")]
            .into_iter()
            .map(|(name, value)| (OsString::from(name), OsString::from(value)))
            .collect();
    let env: BTreeMap<String, String> = child_env(&parent, &approved)
        .into_iter()
        .map(|(name, value)| {
            (
                name.to_string_lossy().into_owned(),
                value.to_string_lossy().into_owned(),
            )
        })
        .collect();

    #[cfg(windows)]
    {
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin"));
        assert_eq!(
            env.get("approved_var").map(String::as_str),
            Some("approved-marker"),
            "an approved name is found whatever the parent's spelling"
        );
    }
    #[cfg(not(windows))]
    {
        assert!(!env.contains_key("PATH"), "Unix names are exact");
        assert_eq!(
            env.get("approved_var").map(String::as_str),
            Some("approved-marker")
        );
    }
}

/// The schedule is per extension: a failure doubles the wait (the
/// `RefreshCycle`'s backoff), a usable reading resets it.
#[test]
fn failures_back_off_and_a_success_resets() {
    let mut schedule = Schedule::default();
    let interval = Duration::from_secs(60);
    let now = Instant::now();
    assert_eq!(schedule.next_in(now), Duration::ZERO);

    schedule.finish(interval, false, now);
    assert_eq!(schedule.next_in(now), Duration::from_secs(120));
    schedule.finish(interval, false, now);
    assert_eq!(schedule.next_in(now), Duration::from_secs(240));
    schedule.finish(interval, true, now);
    assert_eq!(schedule.next_in(now), Duration::from_secs(60));
}

/// RF4.2 at the runner's door: an extension changed since the approval
/// never runs, even holding an `Approved` from before the change.
#[tokio::test]
async fn a_changed_extension_never_runs() {
    let rig = Rig::new();
    let marker = rig.path("pwned");
    let approved = rig.approve(
        "changed",
        &format!("#!/bin/sh\ntouch '{}'\nprintf '{{}}'\n", marker.display()),
        "",
    );
    let manifest = rig.root.join("changed").join(MANIFEST_FILE);
    fs::write(
        &manifest,
        fs::read_to_string(&manifest).unwrap().replace("60", "120"),
    )
    .unwrap();

    let reading = runner().run(&approved).await;
    assert_eq!(reading.run.outcome, Outcome::Changed);
    assert_eq!(reading.checked.status, Status::Error);
    assert_eq!(note_ids(&reading), [CHANGED_NOTE]);
    assert!(!marker.exists(), "nothing ran");
}

#[cfg(unix)]
mod execution {
    use super::*;
    use std::time::Duration as StdDuration;

    const READY: &str = r#"{"schema":1,"status":"ready","metrics":[],"notes":[]}"#;

    /// Whether `pid` still runs (a zombie waiting to be reaped is dead).
    fn alive(pid: &str) -> bool {
        let out = std::process::Command::new("ps")
            .args(["-o", "stat=", "-p", pid.trim()])
            .output()
            .unwrap();
        let stat = String::from_utf8_lossy(&out.stdout);
        let stat = stat.trim();
        !stat.is_empty() && !stat.starts_with('Z')
    }

    /// RF3.1/RF3.2: the arguments reach the program as data (no shell),
    /// `stdin` is closed, the working directory is the extension's folder
    /// and the environment is reduced to the inherited and the approved.
    #[tokio::test]
    async fn argv_cwd_closed_stdin_and_a_reduced_environment() {
        let rig = Rig::new();
        let dump = rig.path("dump");
        fs::create_dir(&dump).unwrap();
        let pwned = rig.root.join("env").join("pwned");
        let approved = rig.approve_with_args(
            "env",
            &format!(
                "#!/bin/sh\n\
                 env > '{0}/env'\n\
                 pwd -P > '{0}/pwd'\n\
                 for arg in \"$@\"; do printf 'arg=%s\\n' \"$arg\" >> '{0}/args'; done\n\
                 if read -r line; then echo open > '{0}/stdin'; else echo closed > '{0}/stdin'; fi\n\
                 printf '{{\"schema\":1,\"status\":\"ready\",\"metrics\":[{{\"label\":\"Saldo\",\"unit\":\"cr\",\"kind\":\"quota\",\"used\":1,\"limit\":2}}],\"notes\":[]}}'\n",
                dump.display()
            ),
            &["a b", "it's \"quoted\"", &format!("$(touch {})", pwned.display())],
            "env = [\"APPROVED_VAR\"]\n",
        );
        let reading = runner().run(&approved).await;
        assert!(reading.run.succeeded(), "{reading:?}");
        assert_eq!(reading.checked.status, Status::Ready, "{reading:?}");
        assert_eq!(reading.checked.metrics[0].label, "Saldo");
        assert!(!pwned.exists(), "an argument is data, never shell code");

        let args = fs::read_to_string(dump.join("args")).unwrap();
        assert_eq!(
            args,
            format!(
                "arg=a b\narg=it's \"quoted\"\narg=$(touch {})\n",
                pwned.display()
            )
        );
        assert_eq!(fs::read_to_string(dump.join("stdin")).unwrap(), "closed\n");
        let pwd = fs::read_to_string(dump.join("pwd")).unwrap();
        assert_eq!(Path::new(pwd.trim()), approved.manifest().dir);

        let env = fs::read_to_string(dump.join("env")).unwrap();
        let vars: BTreeMap<&str, &str> = env
            .lines()
            .filter_map(|line| line.split_once('='))
            .collect();
        assert_eq!(vars["PATH"], "/usr/bin:/bin");
        assert_eq!(vars["HOME"], "/home/someone");
        assert_eq!(vars["LANG"], "en_US.UTF-8");
        assert_eq!(vars["APPROVED_VAR"], "approved-marker");
        // What `sh` itself adds (PWD, SHLVL, `_`) is no inheritance.
        let shell_own = ["PWD", "OLDPWD", "SHLVL", "_", "__CF_USER_TEXT_ENCODING"];
        let unexpected: Vec<&&str> = vars
            .keys()
            .filter(|name| {
                !INHERITED_ENV.contains(name)
                    && **name != "APPROVED_VAR"
                    && !shell_own.contains(name)
            })
            .collect();
        assert!(unexpected.is_empty(), "{unexpected:?}");
        assert!(!env.contains("secret-marker"), "{env}");
    }

    /// RF3.3: the time limit kills the whole process group, including a
    /// child the extension left in the background.
    #[tokio::test]
    async fn a_timeout_kills_the_whole_process_tree() {
        let rig = Rig::new();
        let pid = rig.path("pid");
        let approved = rig.approve(
            "sleeper",
            &format!(
                "#!/bin/sh\nsleep 30 &\necho $! > '{}'\nwait\n",
                pid.display()
            ),
            "timeout_seconds = 1\n",
        );
        let reading = runner().run(&approved).await;
        assert_eq!(reading.run.outcome, Outcome::TimedOut);
        assert_eq!(reading.run.exit_code, None);
        assert!(
            reading.run.duration < StdDuration::from_secs(10),
            "{reading:?}"
        );
        assert_eq!(note_ids(&reading), [TIMED_OUT_NOTE]);
        assert_eq!(reading.checked.status, Status::Error);

        let pid = fs::read_to_string(&pid).unwrap();
        let deadline = Instant::now() + StdDuration::from_secs(10);
        while alive(&pid) {
            assert!(Instant::now() < deadline, "the orphan survived the kill");
            tokio::time::sleep(StdDuration::from_millis(20)).await;
        }
    }

    /// A run dropped in flight (a cancelled cycle) kills the whole tree,
    /// not only the direct child.
    #[tokio::test]
    async fn a_cancelled_run_kills_the_whole_process_tree() {
        let rig = Rig::new();
        let pid = rig.path("pid");
        let approved = rig.approve(
            "sleeper",
            &format!(
                "#!/bin/sh\nsleep 30 &\necho $! > '{}'\nwait\n",
                pid.display()
            ),
            "",
        );
        let runner = runner();
        let task = tokio::spawn(async move { runner.run(&approved).await });
        let deadline = Instant::now() + StdDuration::from_secs(20);
        let pid = loop {
            if let Some(pid) = fs::read_to_string(&pid)
                .ok()
                .filter(|pid| pid.ends_with('\n'))
            {
                break pid;
            }
            assert!(Instant::now() < deadline, "the extension never started");
            tokio::time::sleep(StdDuration::from_millis(20)).await;
        };
        task.abort();
        let _ = task.await;
        let deadline = Instant::now() + StdDuration::from_secs(10);
        while alive(&pid) {
            assert!(Instant::now() < deadline, "the orphan survived the cancel");
            tokio::time::sleep(StdDuration::from_millis(20)).await;
        }
    }

    /// RF3.3: output past the contract's limit is read and discarded
    /// (never a deadlock on a full pipe) and the reading is refused as
    /// oversized.
    #[tokio::test]
    async fn excessive_output_is_capped_and_refused_without_deadlock() {
        let rig = Rig::new();
        let approved = rig.approve(
            "chatty",
            "#!/bin/sh\nhead -c 300000 /dev/zero | tr '\\0' 'a'\n",
            "",
        );
        let reading = runner().run(&approved).await;
        assert!(reading.run.succeeded(), "{reading:?}");
        assert!(reading.run.truncated);
        assert_eq!(reading.run.stdout.len(), output::MAX_BYTES + 1);
        assert_eq!(reading.checked.status, Status::Error);
        let violations: Vec<&str> = reading
            .checked
            .violations
            .iter()
            .map(|violation| violation.id.as_ref())
            .collect();
        assert_eq!(violations, ["extensions.output.too_large"]);
    }

    /// RF3.3: at most [`MAX_RUNNING`] extensions run at once; the rest
    /// wait and still complete.
    #[tokio::test]
    async fn at_most_two_extensions_run_at_once() {
        let rig = Rig::new();
        let running = rig.path("running");
        let seen = rig.path("seen");
        fs::create_dir(&running).unwrap();
        let script = format!(
            "#!/bin/sh\ntouch '{running}/'$$\nls '{running}' | wc -l >> '{seen}'\nsleep 0.3\nrm '{running}/'$$\nprintf '{READY}'\n",
            running = running.display(),
            seen = seen.display()
        );
        let runner = runner();
        let approved: Vec<Approved> = ["a", "b", "c", "d"]
            .into_iter()
            .map(|id| rig.approve(id, &script, ""))
            .collect();
        let (one, two, three, four) = tokio::join!(
            runner.run(&approved[0]),
            runner.run(&approved[1]),
            runner.run(&approved[2]),
            runner.run(&approved[3]),
        );
        for reading in [&one, &two, &three, &four] {
            assert_eq!(reading.checked.status, Status::Ready, "{reading:?}");
        }
        let counts: Vec<usize> = fs::read_to_string(&seen)
            .unwrap()
            .lines()
            .map(|line| line.trim().parse().unwrap())
            .collect();
        assert_eq!(counts.len(), 4, "{counts:?}");
        let most = counts.iter().copied().max().unwrap();
        assert!(most <= MAX_RUNNING, "{counts:?}");
    }

    /// RF3.4: a slow extension occupies one slot at most — the healthy one
    /// finishes long before it, with its own schedule untouched, while the
    /// slow one backs off alone.
    #[tokio::test]
    async fn a_slow_extension_never_delays_a_healthy_one() {
        let rig = Rig::new();
        let slow = rig.approve("slow", "#!/bin/sh\nsleep 30\n", "timeout_seconds = 2\n");
        let fast = rig.approve("fast", &format!("#!/bin/sh\nprintf '{READY}'\n"), "");
        let runner = runner();
        let (slow_reading, fast_reading) = tokio::join!(runner.run(&slow), runner.run(&fast));
        assert_eq!(slow_reading.run.outcome, Outcome::TimedOut);
        assert_eq!(
            fast_reading.checked.status,
            Status::Ready,
            "{fast_reading:?}"
        );
        assert!(
            fast_reading.run.duration < StdDuration::from_secs(2),
            "the healthy extension waited for the slow one: {fast_reading:?}"
        );

        let now = Instant::now();
        let fast_wait = runner.next_run_in("fast", now);
        assert!(
            (StdDuration::from_secs(50)..=StdDuration::from_secs(60)).contains(&fast_wait),
            "no backoff after a success: {fast_wait:?}"
        );
        let slow_wait = runner.next_run_in("slow", now);
        assert!(
            (StdDuration::from_secs(110)..=StdDuration::from_secs(120)).contains(&slow_wait),
            "the failed extension backs off alone: {slow_wait:?}"
        );
    }

    /// RF3.4: each failure doubles the wait of that extension alone, a
    /// success resets it, and a failed run's note never carries the
    /// extension's stderr.
    #[tokio::test]
    async fn failures_back_off_alone_and_stderr_never_leaks() {
        let rig = Rig::new();
        let ok = rig.path("ok");
        let approved = rig.approve(
            "flaky",
            &format!(
                "#!/bin/sh\nif [ -f '{ok}' ]; then\n  printf '{READY}'\nelse\n  echo 'secret-stderr-marker' >&2\n  exit 3\nfi\n",
                ok = ok.display()
            ),
            "",
        );
        let runner = runner();
        let reading = runner.run(&approved).await;
        assert_eq!(reading.run.outcome, Outcome::Exited);
        assert_eq!(reading.run.exit_code, Some(3));
        assert_eq!(note_ids(&reading), [FAILED_NOTE]);
        assert!(reading.run.stdout.is_empty());
        let text: String = reading
            .checked
            .notes
            .iter()
            .map(|note| note.text().to_string())
            .collect();
        assert!(!text.contains("secret-stderr-marker"), "{text}");

        let now = Instant::now();
        let wait = runner.next_run_in("flaky", now);
        assert!(
            (StdDuration::from_secs(110)..=StdDuration::from_secs(120)).contains(&wait),
            "{wait:?}"
        );
        runner.run(&approved).await;
        let wait = runner.next_run_in("flaky", Instant::now());
        assert!(
            (StdDuration::from_secs(230)..=StdDuration::from_secs(240)).contains(&wait),
            "the backoff doubles: {wait:?}"
        );

        fs::write(&ok, "").unwrap();
        let reading = runner.run(&approved).await;
        assert_eq!(reading.checked.status, Status::Ready, "{reading:?}");
        let wait = runner.next_run_in("flaky", Instant::now());
        assert!(
            (StdDuration::from_secs(50)..=StdDuration::from_secs(60)).contains(&wait),
            "a success resets: {wait:?}"
        );
    }

    /// A program whose execute bit was removed after the approval (the
    /// content digest does not change) fails to start; nothing else runs.
    #[tokio::test]
    async fn an_unexecutable_program_is_not_started() {
        use std::os::unix::fs::PermissionsExt;
        let rig = Rig::new();
        let approved = rig.approve("blocked", "#!/bin/sh\nexit 0\n", "");
        fs::set_permissions(
            approved.manifest().dir.join("run.sh"),
            fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let reading = runner().run(&approved).await;
        assert_eq!(reading.run.outcome, Outcome::NotStarted);
        assert_eq!(note_ids(&reading), [NOT_STARTED_NOTE]);
    }
}

/// Windows covers spawn, argv, capture, the Job Object and the timeout
/// with a copied `ping.exe`; process-tree fixtures remain Unix-only.
#[cfg(windows)]
mod windows_execution {
    use super::*;
    use std::time::Duration as StdDuration;

    fn ping(rig: &Rig, id: &str, args: &[&str], extra_toml: &str) -> Approved {
        let dir = rig.root.join(id);
        fs::create_dir_all(&dir).unwrap();
        let system32 = Path::new(&std::env::var_os("SystemRoot").expect("SystemRoot"))
            .join("System32")
            .join("ping.exe");
        fs::copy(system32, dir.join("ping.exe")).unwrap();
        let mut command = vec!["./ping.exe".to_string()];
        command.extend(args.iter().map(|arg| arg.to_string()));
        let manifest = format!(
            "id = \"{id}\"\nname = \"{id}\"\nversion = \"1\"\nschema = 1\n\
             command = {}\ninterval_seconds = 60\n{extra_toml}",
            serde_json::to_string(&command).unwrap()
        );
        fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
        let discovery = discover(&rig.root);
        assert!(discovery.rejected.is_empty(), "{:?}", discovery.rejected);
        let manifest: Manifest = discovery.get(id).expect("discovered").clone();
        rig.consent.approve(&manifest).unwrap()
    }

    #[tokio::test]
    async fn a_run_captures_stdout_and_checks_the_contract() {
        let rig = Rig::new();
        let approved = ping(&rig, "ping", &["-n", "1", "-w", "500", "127.0.0.1"], "");
        let reading = runner().run(&approved).await;
        assert!(reading.run.succeeded(), "{reading:?}");
        assert!(!reading.run.stdout.is_empty());
        // ping's output is not the contract: violations, never raw text.
        assert_eq!(reading.checked.status, Status::Error);
        assert!(!reading.checked.violations.is_empty());
    }

    #[tokio::test]
    async fn a_timeout_terminates_the_job() {
        let rig = Rig::new();
        let approved = ping(
            &rig,
            "slow",
            &["-n", "30", "127.0.0.1"],
            "timeout_seconds = 1\n",
        );
        let reading = runner().run(&approved).await;
        assert_eq!(reading.run.outcome, Outcome::TimedOut);
        assert!(
            reading.run.duration < StdDuration::from_secs(10),
            "{reading:?}"
        );
        assert_eq!(note_ids(&reading), [TIMED_OUT_NOTE]);
    }
}
