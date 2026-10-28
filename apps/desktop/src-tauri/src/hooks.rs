//! PRD 0013 RF4: the core's hooks inside the desktop app.
//!
//! Off until the user turns them on in Settings → Integration (RF3.2). The
//! rules live in the `hooks.toml` beside the accounts' TOML, the same file
//! `cuotai --watch --hooks` reads; the editor saves it through the core
//! ([`Document::save`]). Commands run only while the switch is on and the
//! rules on disk are exactly the ones the user confirmed in the dialog that
//! lists them ([`Consent`], shared with the command line). The events come
//! from the core's [`Watcher`] over the `export::v1` of the app's own
//! refresh cycle (the backend hands every state change to
//! [`Integration::observe`]), so hooks add no query; the core's [`Runner`]
//! runs them with its isolation and limits and appends each run to the
//! shared [`RunLog`], which Settings shows.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use cuotai_core::{
    events::{EventKind, EventState},
    export::v1::Export,
    hooks::{
        self, Confirmed, Consent, Document, Hooks, LoggedRun, OnRun, RuleDraft, RunLog, Runner,
        DEFAULT_TIMEOUT_SECONDS, MAX_TIMEOUT_SECONDS,
    },
    text::Localized,
    watch::Watcher,
};
use serde::Serialize;

/// What the watcher asks per account: its refresh interval and the recent
/// points of each metric of the report (`Monitor::recent_points`).
pub type PaceFn<'a> = dyn Fn(
        usize,
        &cuotai_core::Report,
    ) -> Option<(std::time::Duration, Vec<Vec<cuotai_core::pace::Point>>)>
    + 'a;

/// Emitted (without a payload) after each run, so Settings reads the log
/// again.
pub const HOOKS_EVENT: &str = "hooks";

/// Runs shown in Settings, newest first.
pub const LATEST_RUNS: usize = 20;

/// The confirmed rules running over the app's events.
struct Active {
    hooks: Hooks,
    runner: Runner,
    watcher: Watcher,
}

impl Active {
    fn runs(&self, hooks: &Hooks) -> bool {
        self.hooks.path() == hooks.path() && self.hooks.digest() == hooks.digest()
    }
}

/// The hooks' switch, where confirmations and runs are kept, and the
/// runner while the confirmed rules run.
pub struct Integration {
    enabled: bool,
    /// `None` without a local data directory: nothing can be confirmed.
    consent: Option<Consent>,
    log: Option<PathBuf>,
    /// Called after each run (the app tells Settings).
    on_run: OnRun,
    active: Option<Active>,
    /// The `hooks.toml` last reconciled (that of the loaded configuration).
    path: Option<PathBuf>,
    /// Runs dropped from the full queue since the rules started.
    dropped: usize,
    /// A run's line could not be written to the log.
    log_failed: Arc<AtomicBool>,
    /// PRD 0032 RF5.1: the quiet hours a rule with `quiet = "skip"` honours.
    quiet: cuotai_core::notify_prefs::Source,
}

impl Default for Integration {
    fn default() -> Self {
        let mut integration = Self::new(
            false,
            Consent::in_data_dir().ok(),
            RunLog::in_data_dir().map(|log| log.path().to_path_buf()),
        );
        integration.quiet = cuotai_core::notify_prefs::Source::open();
        integration
    }
}

/// What the main window needs: whether the switch is on and, when it is,
/// why nothing runs (`unconfirmed`, `invalid`, `missing`) or `on`.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HooksBadge {
    pub state: &'static str,
}

impl Default for HooksBadge {
    fn default() -> Self {
        Self { state: "off" }
    }
}

/// One run of the log, with its line for people.
#[derive(Serialize, Clone, Debug)]
pub struct RunView {
    #[serde(flatten)]
    run: LoggedRun,
    summary: Localized,
}

/// RF4.1/RF4.2: Settings → Integration's hooks: the switch, the state, the
/// file's rules for the editor and, while they wait for a confirmation,
/// the exact listing the dialog shows.
#[derive(Serialize, Debug)]
pub struct HooksView {
    /// `no_config`, `missing` (no `hooks.toml`), `invalid`, `off`,
    /// `unconfirmed` (on, waiting for the user) or `on`.
    pub state: &'static str,
    pub enabled: bool,
    pub path: Option<String>,
    /// The file's revision the editor saves over.
    pub revision: Option<String>,
    /// The rules as written; `None` when the file is not TOML the editor
    /// can show.
    pub rules: Option<Vec<RuleDraft>>,
    /// Why the rules cannot run (an invalid file).
    pub error: Option<Localized>,
    /// The current rules are the ones confirmed.
    pub confirmed: bool,
    /// While valid and not confirmed: what a confirmation covers and the
    /// exact listing of every command.
    pub digest: Option<String>,
    pub listing: Vec<Localized>,
    /// The names of the confirmed rules running now (the ones a test can
    /// fire).
    pub running: Vec<String>,
    pub runs: Vec<RunView>,
    pub warnings: Vec<Localized>,
    pub events: [&'static str; EventKind::ALL.len()],
    pub default_timeout: u64,
    pub max_timeout: u64,
}

impl Integration {
    /// Stopped; [`Self::reconcile`] starts the confirmed rules when on.
    pub fn new(enabled: bool, consent: Option<Consent>, log: Option<PathBuf>) -> Self {
        Self {
            enabled,
            consent,
            log,
            on_run: Arc::new(|_, _| {}),
            active: None,
            path: None,
            dropped: 0,
            log_failed: Arc::new(AtomicBool::new(false)),
            quiet: cuotai_core::notify_prefs::Source::new(None),
        }
    }

    pub fn set_on_run(&mut self, on_run: OnRun) {
        self.on_run = on_run;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Make what runs match the switch, the file at `path` (the
    /// `hooks.toml` of the loaded configuration) and the confirmations:
    /// the confirmed rules keep running while nothing changed, anything
    /// else stops them, and confirmed rules not running yet start (with new
    /// baselines). Returns whether the rules that ran before still run.
    pub fn reconcile(&mut self, path: Option<&Path>) -> bool {
        self.path = path.map(Path::to_path_buf);
        let wanted = self
            .enabled
            .then(|| path.map(Document::read))
            .flatten()
            .and_then(|doc| doc.into_hooks().ok())
            .and_then(|hooks| self.consent.as_ref()?.check(hooks).ok());
        let Some(confirmed) = wanted else {
            let ran = self.active.is_none();
            self.active = None;
            return ran;
        };
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.runs(confirmed.hooks()))
        {
            return true;
        }
        self.start(confirmed);
        false
    }

    fn start(&mut self, confirmed: Confirmed) {
        let hooks = confirmed.hooks().clone();
        let log_failed = Arc::clone(&self.log_failed);
        let notify = Arc::clone(&self.on_run);
        let on_run: OnRun = Arc::new(move |run, error| {
            if error.is_some() {
                log_failed.store(true, Ordering::Relaxed);
            }
            notify(run, error);
        });
        // The previous runner (if any) goes first: its queue is dropped and
        // its running hooks killed.
        self.active = None;
        self.dropped = 0;
        self.active = Some(Active {
            hooks,
            runner: Runner::start(confirmed, self.log.as_ref().map(RunLog::at), on_run),
            watcher: Watcher::new(EventState::default(), None),
        });
    }

    /// Whether confirmed rules run now (the backend then hands it every
    /// state change).
    pub fn is_running(&self) -> bool {
        self.active.is_some()
    }

    /// The events of `export` (the app's cycle at `now`) after the one last
    /// observed, run by the confirmed rules. The first export after the
    /// rules started is the baseline, and so is the first one of another
    /// configuration (its `hooks.toml` is reconciled first). Every export
    /// first checks that the file at `path` still holds the confirmed rules
    /// (RF3.2), so an edit made outside the app shows at the next state
    /// change, not only at the next event; otherwise they stop (or
    /// restart, when the new rules are confirmed too) and these events run
    /// nothing. Returns whether what runs changed. Only the Unix-only
    /// runner test calls it.
    #[cfg(all(test, unix))]
    pub fn observe(&mut self, path: &Path, export: &Export, now: DateTime<Utc>) -> bool {
        self.observe_with(path, export, now, &|_, _| None)
    }

    /// [`Self::observe`] with each account's pace input (PRD 0024 RF5), so
    /// `quota_exhaustion_projected` rules can fire: `pace(index, report)` is
    /// the account's refresh interval and the recent points of each metric
    /// of `report`.
    pub fn observe_with(
        &mut self,
        path: &Path,
        export: &Export,
        now: DateTime<Utc>,
        pace: &PaceFn<'_>,
    ) -> bool {
        let same_file = self.path.as_deref() == Some(path);
        if !self.reconcile(Some(path)) || !same_file {
            self.baseline(export, now, pace);
            return true;
        }
        let Some(active) = &mut self.active else {
            return false;
        };
        for event in &active.watcher.observe_with(export, now, pace) {
            let in_quiet = self.quiet.quiet_at(event.occurred_at);
            self.dropped += active.runner.submit_in(event, in_quiet);
        }
        false
    }

    fn baseline(&mut self, export: &Export, now: DateTime<Utc>, pace: &PaceFn<'_>) {
        if let Some(active) = &mut self.active {
            active.watcher.observe_with(export, now, pace);
        }
    }

    /// RF4.1: run the confirmed rule `name` alone with its sample event.
    /// Errors: `hooks_off` (no confirmed rule runs), `hook_unknown` (not
    /// among the confirmed rules: unsaved or changed).
    pub fn test(&mut self, path: Option<&Path>, name: &str) -> Result<(), &'static str> {
        self.reconcile(path);
        let active = self.active.as_ref().ok_or("hooks_off")?;
        let rule = active
            .hooks
            .rules()
            .iter()
            .find(|rule| rule.name == name)
            .ok_or("hook_unknown")?;
        let sample = rule.sample(Utc::now());
        self.dropped += active.runner.submit_to(name, &sample).unwrap_or(0);
        Ok(())
    }

    /// RF4.2: record the user's confirmation of the rules now in the file
    /// at `path`, which must be the ones the dialog listed (`digest`), and
    /// run them when on. Errors: the core's `hooks.error.*` text.
    pub fn confirm(&mut self, path: Option<&Path>, digest: &str) -> Result<(), Localized> {
        let shown = Document::read(path.ok_or_else(hooks::changed)?).into_hooks()?;
        if shown.digest() != digest {
            return Err(hooks::changed());
        }
        self.consent
            .as_ref()
            .ok_or_else(hooks::consent_storage)?
            .confirm(shown)?;
        self.reconcile(path);
        Ok(())
    }

    /// The state of the file at `path` and of the runs, as Settings shows
    /// it (after [`Self::reconcile`]).
    pub fn view(&self, path: Option<&Path>) -> HooksView {
        let doc = path.map(Document::read);
        let valid = doc.as_ref().and_then(|doc| doc.hooks().ok());
        let confirmed = valid.is_some_and(|valid| {
            self.consent
                .as_ref()
                .is_some_and(|consent| consent.check(valid.clone()).is_ok())
        });
        let state = state_of(doc.as_ref(), self.enabled, confirmed, self.is_running());
        let pending = valid.filter(|_| !confirmed);
        let mut warnings = Vec::new();
        if self.consent.is_none() {
            warnings.push(hooks::consent_storage());
        }
        if self.dropped > 0 {
            warnings.push(hooks::dropped(self.dropped));
        }
        if let Some(log) = self.log.as_ref() {
            if self.log_failed.load(Ordering::Relaxed) {
                warnings.push(hooks::log_unwritable(log));
            }
        }
        HooksView {
            state,
            enabled: self.enabled,
            path: path.map(|path| path.display().to_string()),
            revision: doc
                .as_ref()
                .and_then(|doc| doc.revision().map(str::to_owned)),
            rules: doc.as_ref().and_then(|doc| doc.drafts().map(<[_]>::to_vec)),
            error: doc
                .as_ref()
                .and_then(|doc| doc.hooks().err())
                .filter(|_| state == "invalid")
                .cloned(),
            confirmed,
            digest: pending.map(Hooks::digest),
            listing: pending.map(Hooks::listing).unwrap_or_default(),
            running: self
                .active
                .iter()
                .flat_map(|active| active.hooks.rules().iter().map(|rule| rule.name.clone()))
                .collect(),
            runs: self
                .log
                .as_ref()
                .map(|log| RunLog::at(log).latest(LATEST_RUNS))
                .unwrap_or_default()
                .into_iter()
                .map(|run| RunView {
                    summary: run.summary(),
                    run,
                })
                .collect(),
            warnings,
            events: EventKind::ALL.map(EventKind::name),
            default_timeout: DEFAULT_TIMEOUT_SECONDS,
            max_timeout: MAX_TIMEOUT_SECONDS,
        }
    }
}

fn state_of(doc: Option<&Document>, enabled: bool, confirmed: bool, running: bool) -> &'static str {
    let Some(doc) = doc else {
        return "no_config";
    };
    match doc.hooks() {
        Err(_) if doc.revision().is_none() && doc.drafts().is_some_and(<[_]>::is_empty) => {
            "missing"
        }
        Err(_) => "invalid",
        Ok(_) if !enabled => "off",
        Ok(_) if confirmed && running => "on",
        Ok(_) => "unconfirmed",
    }
}

impl HooksView {
    pub fn badge(&self) -> HooksBadge {
        HooksBadge {
            state: if self.enabled { self.state } else { "off" },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::{
        config_file::ConfigIdentity, export::v1::Options, refresh::RefreshCycle, Account, Config,
        Provider, Report, Snapshot, Status,
    };
    use std::time::{Duration, Instant};

    #[cfg_attr(not(unix), allow(dead_code))]
    fn account() -> Account {
        Account {
            id: None,
            name: "Work".into(),
            provider: Provider::AlibabaCloud,
            credential_env: Some("ALIBABA_KEY".into()),
            ..Account::default()
        }
    }

    /// The app's cycle after one reading of `status`, as an export.
    #[cfg_attr(not(unix), allow(dead_code))]
    fn export(status: Status, at: DateTime<Utc>) -> Export {
        let config = Config {
            accounts: vec![account()],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let token = cycle.begin_full().unwrap();
        let report = Report {
            account_id: None,
            account: "Work".into(),
            provider: Provider::AlibabaCloud,
            checked_at: at,
            status,
            metrics: vec![],
            notes: vec![],
            notes_l10n: vec![],
            source: "test".into(),
            session_expires_at: None,
            diagnostic: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        };
        let snapshot = Snapshot {
            started_at: at,
            finished_at: at,
            reports: vec![report],
        };
        assert!(cycle.finish_full(Some(snapshot), token, Instant::now()));
        Export::from_cycle(
            &config,
            &ConfigIdentity::Explicit("test".into()),
            &cycle,
            &[],
            at,
            Options::default(),
        )
    }

    struct Rig {
        dir: tempfile::TempDir,
    }

    impl Rig {
        fn new() -> Self {
            Self {
                dir: tempfile::tempdir().unwrap(),
            }
        }

        fn path(&self, name: &str) -> PathBuf {
            self.dir.path().join(name)
        }

        fn hooks_file(&self) -> PathBuf {
            self.path("hooks.toml")
        }

        fn integration(&self, enabled: bool) -> Integration {
            Integration::new(
                enabled,
                Some(Consent::at(self.path("data/consent.json"))),
                Some(self.path("data/runs.log")),
            )
        }

        /// One rule on `refresh_failed` appending `$CUOTAI_ACCOUNT` (and
        /// whether the payload is a sample) to `out` through `sh`.
        fn save_rule(&self, name: &str, revision: Option<&str>) -> Document {
            let script = self.path("append.sh");
            std::fs::write(
                &script,
                format!(
                    "grep -q '\"sample\":true' && s=sample || s=real\nprintf '%s %s %s\\n' \"$1\" \"$CUOTAI_ACCOUNT\" \"$s\" >> '{}'\n",
                    self.path("out").display()
                ),
            )
            .unwrap();
            Document::save(
                &self.hooks_file(),
                revision,
                vec![RuleDraft {
                    name: name.into(),
                    events: vec!["refresh_failed".into()],
                    command: vec!["sh".into(), script.display().to_string(), name.into()],
                    ..RuleDraft::default()
                }],
            )
            .unwrap()
        }

        #[cfg_attr(not(unix), allow(dead_code))]
        fn out(&self) -> Vec<String> {
            std::fs::read_to_string(self.path("out"))
                .unwrap_or_default()
                .lines()
                .map(str::to_owned)
                .collect()
        }

        #[cfg_attr(not(unix), allow(dead_code))]
        async fn wait_for_out(&self, lines: usize) -> Vec<String> {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let out = self.out();
                if out.len() >= lines {
                    return out;
                }
                assert!(Instant::now() < deadline, "only {out:?}");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    }

    #[cfg_attr(not(unix), allow(dead_code))]
    fn t(minutes: i64) -> DateTime<Utc> {
        use chrono::TimeZone;
        Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap() + chrono::TimeDelta::minutes(minutes)
    }

    /// RF3.2 in the app: off by default, and turning hooks on with rules
    /// nobody confirmed runs nothing: Settings gets the exact listing to
    /// confirm instead.
    #[tokio::test]
    async fn hooks_stay_off_until_turned_on_and_confirmed() {
        let rig = Rig::new();
        let path = rig.hooks_file();
        let mut hooks = rig.integration(false);
        assert_eq!(hooks.view(None).state, "no_config");
        assert_eq!(hooks.view(Some(&path)).state, "missing");
        let doc = rig.save_rule("journal", None);

        hooks.reconcile(Some(&path));
        let view = hooks.view(Some(&path));
        assert_eq!(
            (view.state, view.enabled, view.confirmed),
            ("off", false, false)
        );
        assert!(!hooks.is_running());

        hooks.set_enabled(true);
        hooks.reconcile(Some(&path));
        let view = hooks.view(Some(&path));
        assert_eq!(view.state, "unconfirmed");
        assert_eq!(view.badge().state, "unconfirmed");
        assert!(!hooks.is_running(), "nothing runs before the confirmation");
        assert_eq!(
            view.digest.as_deref(),
            Some(doc.hooks().unwrap().digest().as_str())
        );
        let listing: Vec<String> = view
            .listing
            .iter()
            .map(|line| line.resolve("en-US"))
            .collect();
        assert!(
            listing
                .iter()
                .any(|line| line.starts_with("command: [\"sh\",")
                    && line.ends_with("\"journal\"]")),
            "{listing:?}"
        );
        assert_eq!(hooks.test(Some(&path), "journal"), Err("hooks_off"));

        // A confirmation of rules other than the ones listed is refused.
        assert_eq!(
            hooks.confirm(Some(&path), "0000").unwrap_err().id,
            "hooks.error.changed"
        );
        hooks
            .confirm(Some(&path), view.digest.as_deref().unwrap())
            .unwrap();
        let view = hooks.view(Some(&path));
        assert_eq!((view.state, view.confirmed), ("on", true));
        assert_eq!(view.running, ["journal"]);
        assert!(view.listing.is_empty());

        hooks.set_enabled(false);
        hooks.reconcile(Some(&path));
        assert_eq!(hooks.view(Some(&path)).state, "off");
        assert!(!hooks.is_running());
    }

    /// RF4.1: the app's own readings fire the confirmed rule once per real
    /// transition (the first export is the baseline), the test button runs
    /// its sample, and every run reaches the log Settings shows, newest
    /// first. A file changed since the confirmation runs nothing more.
    #[cfg(unix)]
    #[tokio::test]
    async fn readings_and_the_test_run_the_confirmed_rule_and_reach_the_log() {
        let rig = Rig::new();
        let path = rig.hooks_file();
        let doc = rig.save_rule("journal", None);
        let mut hooks = rig.integration(true);
        let digest = doc.hooks().unwrap().digest();
        hooks.confirm(Some(&path), &digest).unwrap();
        assert!(hooks.is_running());

        hooks.observe(&path, &export(Status::Error, t(0)), t(0));
        hooks.observe(&path, &export(Status::Ready, t(1)), t(1));
        hooks.observe(&path, &export(Status::Error, t(2)), t(2));
        // The same reading again is no new transition.
        hooks.observe(&path, &export(Status::Error, t(2)), t(3));
        assert_eq!(rig.wait_for_out(1).await, ["journal Work real"]);

        hooks.test(Some(&path), "journal").unwrap();
        assert_eq!(
            rig.wait_for_out(2).await,
            ["journal Work real", "journal sample sample"]
        );
        assert_eq!(hooks.test(Some(&path), "other"), Err("hook_unknown"));
        let deadline = Instant::now() + Duration::from_secs(20);
        let runs = loop {
            let runs = hooks.view(Some(&path)).runs;
            if runs.len() == 2 {
                break runs;
            }
            assert!(Instant::now() < deadline);
            tokio::time::sleep(Duration::from_millis(20)).await;
        };
        assert!(runs.iter().all(|run| run.run.rule == "journal"
            && run.run.event == EventKind::RefreshFailed
            && run.run.exit_code == Some(0)));
        assert!(runs[0].run.at >= runs[1].run.at, "newest first");
        assert!(runs[0]
            .summary
            .resolve("en-US")
            .starts_with("journal (refresh_failed): done in"));

        // Edited outside the app: the next state change stops the rules
        // even when it is no transition (the main window's banner), and the
        // next transitions run nothing until a new confirmation.
        let edited = rig.save_rule("renamed", doc.revision());
        assert!(hooks.observe(&path, &export(Status::Error, t(2)), t(4)));
        assert!(!hooks.is_running());
        hooks.observe(&path, &export(Status::Ready, t(5)), t(5));
        hooks.observe(&path, &export(Status::Error, t(6)), t(6));
        assert!(!hooks.is_running());
        let view = hooks.view(Some(&path));
        assert_eq!(view.state, "unconfirmed");
        assert_eq!(
            view.digest,
            Some(edited.hooks().unwrap().digest()),
            "the new rules are listed to confirm"
        );
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(rig.out().len(), 2);
    }

    /// An invalid file is shown with its reason and never runs.
    #[test]
    fn an_invalid_file_is_shown_with_its_reason() {
        let rig = Rig::new();
        let path = rig.hooks_file();
        std::fs::write(
            &path,
            "[[hooks]]\nname = \"x\"\nevents = [\"refresh_failed\"]\ncommand = [\"rel/run\"]\n",
        )
        .unwrap();
        let mut hooks = rig.integration(true);
        hooks.reconcile(Some(&path));
        let view = hooks.view(Some(&path));
        assert_eq!(view.state, "invalid");
        assert_eq!(view.error.unwrap().id, "hooks.error.command");
        assert_eq!(view.rules.unwrap()[0].command, ["rel/run"]);
        assert!(view.revision.is_some());
        assert!(!hooks.is_running());
    }
}
