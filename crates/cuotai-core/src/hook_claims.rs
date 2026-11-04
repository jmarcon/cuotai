//! PRD 0054 RF2: cross-process claims for hooks.
//!
//! The desktop app and `cuotai --watch --hooks` read the same `hooks.toml`
//! and observe the same accounts, so without coordination each process
//! would run a rule for the same transition. Before a real hook is queued,
//! [`HookClaims::claim`] records `(hooks.toml identity, rules digest, rule,
//! transition)` in `<OS local data dir>/cuotai/hooks/claims.json` under the
//! advisory `claims.lock` (the same exclusive-file lock as `notified.json`,
//! [`crate::resets::LedgerLock`]): it re-reads the file, prunes claims older
//! than [`KEEP`], appends the new one and saves atomically, then releases the
//! lock before any command starts. Only the first process to claim runs the
//! rule; the other skips it.
//!
//! Delivery is at most once: a process that claims and then dies loses that
//! run (hooks are not a durable queue, PRD 0013). When the store or its lock
//! cannot be used, [`Claim::Unavailable`] tells the runner to run nothing
//! (fail closed). The file holds only a SHA-256 key and two times per claim:
//! never a payload, account name, command, URL or token.

use std::io;
use std::path::{Path, PathBuf};

use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};

use crate::resets::{LedgerLock, DRIFT_TOLERANCE};
use crate::watch::Transition;

/// How long a claim is kept.
pub const KEEP: TimeDelta = TimeDelta::days(8);

/// The claims file's name, beside `consent.json` and `runs.log`.
pub const FILE_NAME: &str = "claims.json";

/// The outcome of [`HookClaims::claim`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// This process owns the run.
    Won,
    /// Another process (or an earlier observation here) already claimed it.
    Taken,
    /// The store or its lock could not be used: nothing may run.
    Unavailable,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Record {
    #[serde(default)]
    claims: Vec<Entry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct Entry {
    /// SHA-256 of the hooks file's identity, the rules' digest, the rule's
    /// name and the transition's stable parts.
    key: String,
    /// The transition's instance: a window's reset time or the source
    /// reading's time.
    at: DateTime<Utc>,
    /// `at` is a window's reset time, matched within the drift tolerance.
    windowed: bool,
    /// When the claim was made, for pruning.
    claimed_at: DateTime<Utc>,
}

impl Entry {
    fn same(&self, other: &Entry) -> bool {
        self.key == other.key
            && self.windowed == other.windowed
            && if self.windowed {
                (self.at - other.at).abs() <= DRIFT_TOLERANCE
            } else {
                self.at == other.at
            }
    }
}

/// The shared claim store.
#[derive(Debug, Clone)]
pub struct HookClaims {
    file: PathBuf,
}

impl HookClaims {
    pub fn at(file: impl Into<PathBuf>) -> Self {
        Self { file: file.into() }
    }

    /// `<OS local data dir>/cuotai/hooks/claims.json`; `None` without a local
    /// data directory (and then no hook runs).
    pub fn in_data_dir() -> Option<Self> {
        dirs::data_local_dir().map(|dir| Self::at(dir.join("cuotai").join("hooks").join(FILE_NAME)))
    }

    pub fn path(&self) -> &Path {
        &self.file
    }

    /// Claim the run of `rule` (of the rules `rules_digest` in the file whose
    /// identity is `file_identity`) for `transition`, at `now`.
    pub fn claim(
        &self,
        file_identity: &str,
        rules_digest: &str,
        rule: &str,
        transition: &Transition,
        now: DateTime<Utc>,
    ) -> Claim {
        let entry = Entry {
            key: key(file_identity, rules_digest, rule, transition),
            at: transition.at(),
            windowed: transition.is_windowed(),
            claimed_at: now,
        };
        match self.record(entry, now) {
            Ok(true) => Claim::Won,
            Ok(false) => Claim::Taken,
            Err(_) => Claim::Unavailable,
        }
    }

    /// Under the lock: re-read, prune, and append `entry` unless an equal one
    /// is there. `Ok(true)` when it was appended and saved.
    fn record(&self, entry: Entry, now: DateTime<Utc>) -> io::Result<bool> {
        if let Some(dir) = self.file.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            crate::private_fs::ensure_dir(dir)?;
        }
        let _lock = LedgerLock::acquire(&self.file)
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "claims lock"))?;
        let mut record = self.read()?;
        let before = record.claims.len();
        record.claims.retain(|kept| now - kept.claimed_at <= KEEP);
        if record.claims.iter().any(|kept| kept.same(&entry)) {
            if record.claims.len() != before {
                self.write(&record)?;
            }
            return Ok(false);
        }
        record.claims.push(entry);
        self.write(&record)?;
        Ok(true)
    }

    /// A missing file holds no claim, and so does a damaged one (it is
    /// replaced at the next save); a file that cannot be read is an error.
    fn read(&self) -> io::Result<Record> {
        match std::fs::read(&self.file) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).unwrap_or_default()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Record::default()),
            Err(error) => Err(error),
        }
    }

    fn write(&self, record: &Record) -> io::Result<()> {
        crate::private_fs::write_file(&self.file, &serde_json::to_vec(record)?)
    }
}

fn key(file_identity: &str, rules_digest: &str, rule: &str, transition: &Transition) -> String {
    use sha2::{Digest, Sha256};
    let mut hash = Sha256::new();
    for part in [file_identity, rules_digest, rule, transition.key()] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    format!("{:x}", hash.finalize())
}

/// The hooks can run no command because their claims cannot be recorded.
pub fn unavailable() -> crate::text::Localized {
    crate::text::Localized::fixed(
        "hooks.error.claims",
        "Hooks are paused: Cuotai could not record which runs it claimed in the local data directory, so no command runs.",
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn t(minutes: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 11, 4, 12, 0, 0).unwrap() + TimeDelta::minutes(minutes)
    }

    #[test]
    fn the_first_claim_wins_and_an_equal_one_is_taken() {
        let dir = tempfile::tempdir().unwrap();
        let claims = HookClaims::at(dir.path().join("hooks").join(FILE_NAME));
        let other = HookClaims::at(claims.path());
        let reading = Transition::observed("k", t(0));
        assert_eq!(
            claims.claim("id", "rules", "sound", &reading, t(0)),
            Claim::Won
        );
        // A second process with its own store value on the same file.
        assert_eq!(
            other.claim("id", "rules", "sound", &reading, t(1)),
            Claim::Taken
        );
        // Another rule, another hooks file or other rules each run once.
        assert_eq!(
            other.claim("id", "rules", "log", &reading, t(1)),
            Claim::Won
        );
        assert_eq!(
            other.claim("id2", "rules", "sound", &reading, t(1)),
            Claim::Won
        );
        assert_eq!(
            other.claim("id", "rules2", "sound", &reading, t(1)),
            Claim::Won
        );
        // A later distinct reading runs again.
        let later = Transition::observed("k", t(5));
        assert_eq!(
            other.claim("id", "rules", "sound", &later, t(5)),
            Claim::Won
        );
    }

    #[test]
    fn a_window_instance_matches_within_the_drift_tolerance() {
        let dir = tempfile::tempdir().unwrap();
        let claims = HookClaims::at(dir.path().join(FILE_NAME));
        let window = Transition::windowed("k", t(300));
        assert_eq!(claims.claim("id", "r", "a", &window, t(0)), Claim::Won);
        let drifted = Transition::windowed("k", t(301));
        assert_eq!(claims.claim("id", "r", "a", &drifted, t(0)), Claim::Taken);
        let next = Transition::windowed("k", t(300 + 300));
        assert_eq!(claims.claim("id", "r", "a", &next, t(0)), Claim::Won);
        // A reading time never matches a window time.
        let reading = Transition::observed("k", t(300));
        assert_eq!(claims.claim("id", "r", "a", &reading, t(0)), Claim::Won);
    }

    #[test]
    fn claims_are_pruned_after_eight_days() {
        let dir = tempfile::tempdir().unwrap();
        let claims = HookClaims::at(dir.path().join(FILE_NAME));
        let reading = Transition::observed("k", t(0));
        assert_eq!(claims.claim("id", "r", "a", &reading, t(0)), Claim::Won);
        let other = Transition::observed("k", t(1));
        let later = t(0) + KEEP + TimeDelta::minutes(1);
        assert_eq!(claims.claim("id", "r", "a", &other, later), Claim::Won);
        let record = claims.read().unwrap();
        assert_eq!(record.claims.len(), 1, "the old claim is gone");
        // Pruned, the old transition could run again (at most once is per
        // retention, not forever).
        assert_eq!(claims.claim("id", "r", "a", &reading, later), Claim::Won);
    }

    #[test]
    fn the_file_holds_only_hashes_and_times() {
        let dir = tempfile::tempdir().unwrap();
        let claims = HookClaims::at(dir.path().join(FILE_NAME));
        let reading = Transition::observed("claude:Work quota_exhausted", t(0));
        claims.claim(
            "/Users/someone/hooks.toml",
            "rules",
            "say \"hi\"",
            &reading,
            t(0),
        );
        let text = std::fs::read_to_string(claims.path()).unwrap();
        for word in ["Work", "claude", "someone", "say", "hooks.toml"] {
            assert!(!text.contains(word), "{word} is in the claims file");
        }
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let entry = &value["claims"][0];
        assert_eq!(entry["key"].as_str().unwrap().len(), 64);
        assert_eq!(
            entry.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["at", "claimed_at", "key", "windowed"]
        );
    }

    #[test]
    fn a_damaged_file_is_replaced_and_a_held_lock_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let claims = HookClaims::at(dir.path().join(FILE_NAME));
        std::fs::write(claims.path(), b"{not json").unwrap();
        let reading = Transition::observed("k", t(0));
        assert_eq!(claims.claim("id", "r", "a", &reading, t(0)), Claim::Won);
        // Another process holds the lock past the wait: nothing runs.
        std::fs::write(crate::resets::lock_path(claims.path()), "1").unwrap();
        let other = Transition::observed("k", t(1));
        assert_eq!(
            claims.claim("id", "r", "a", &other, t(1)),
            Claim::Unavailable
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_store_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let hooks = dir.path().join("hooks");
        let claims = HookClaims::at(hooks.join(FILE_NAME));
        claims.claim("id", "r", "a", &Transition::observed("k", t(0)), t(0));
        let mode = |path: &Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&hooks), 0o700);
        assert_eq!(mode(claims.path()), 0o600);
    }

    #[test]
    fn an_unwritable_store_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        // A file where the directory should be.
        let blocker = dir.path().join("hooks");
        std::fs::write(&blocker, b"").unwrap();
        let claims = HookClaims::at(blocker.join(FILE_NAME));
        assert_eq!(
            claims.claim("id", "r", "a", &Transition::observed("k", t(0)), t(0)),
            Claim::Unavailable
        );
    }
}
