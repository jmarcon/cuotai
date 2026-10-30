//! PRD 0014 RF1.4/RF2.2: publishing the widget snapshot of this device.
//!
//! The core builds the file ([`cuotai_core::widget_snapshot`]) from the
//! `export::v1` the app already shows; this module owns where it lives, who
//! decided that, and when it is written. Publishing runs in its own task,
//! never inside the refresh: every state change only marks it dirty, and
//! the task coalesces those marks and writes at most once every
//! [`MIN_INTERVAL`]. With the switch off nothing is written and an existing
//! file is deleted.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cuotai_core::widget_snapshot::{self, WidgetSnapshot};
use serde::Serialize;
use tokio::sync::Notify;

use crate::settings::{WidgetPrefs, MAX_WIDGET_ACCOUNTS};

/// RF1.4: at most one write per 15 seconds. Changes inside that window are
/// coalesced into the next write instead of lost.
pub const MIN_INTERVAL: Duration = Duration::from_secs(15);

/// The *app group* of earlier releases (see [`legacy_snapshot_path`]).
#[cfg(target_os = "macos")]
const LEGACY_APP_GROUP: &str = "WL9XSC6H36.dev.cuotai.desktop";

/// PRD 0014 RF3.1: the extension the release embeds in the app bundle.
#[cfg(target_os = "macos")]
const EXTENSION: &str = "CuotaiWidgets.appex";

/// PRD 0014 RF5: the file name every consumer looks for.
pub const FILE_NAME: &str = "widget.json";

/// Where the snapshot lives: this user's local data directory on every
/// system. The macOS extension reads this one file through a read-only
/// sandbox exception (`CuotaiWidgets.entitlements`).
pub fn snapshot_path() -> Option<PathBuf> {
    Some(data_dir()?.join(FILE_NAME))
}

/// The *app group* file of earlier releases. macOS ignores that entitlement
/// without a provisioning profile, so nothing reads it; it only goes away
/// with the data.
fn legacy_snapshot_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        Some(
            dirs::home_dir()?
                .join("Library")
                .join("Group Containers")
                .join(LEGACY_APP_GROUP)
                .join(FILE_NAME),
        )
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Whether this executable runs from a bundle that embeds the widget
/// extension (`Cuotai.app/Contents/{MacOS/cuotai,PlugIns/…appex}`).
fn embeds_extension() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent()?.parent().map(Path::to_path_buf))
            .is_some_and(|contents| contents.join("PlugIns").join(EXTENSION).is_dir())
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// `<local data>/cuotai`: the per-user directory (RF5.1).
fn data_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("cuotai"))
}

/// The snapshot path as Settings shows it, and whether a WidgetKit extension
/// reads it.
#[derive(Debug, Clone, Serialize)]
pub struct WidgetsView {
    pub prefs: WidgetPrefs,
    pub path: Option<String>,
    /// PRD 0014 RF3.1: this bundle embeds the extension that reads the file;
    /// without it only third-party integrations do.
    pub extension_reads: bool,
    pub macos: bool,
    pub accounts_min: usize,
    pub accounts_max: usize,
}

/// The current state, for the settings screen and the change command.
pub fn view(prefs: WidgetPrefs) -> WidgetsView {
    let path = snapshot_path();
    WidgetsView {
        prefs,
        extension_reads: embeds_extension(),
        path: path.map(|path| path.display().to_string()),
        macos: cfg!(target_os = "macos"),
        accounts_min: 1,
        accounts_max: MAX_WIDGET_ACCOUNTS,
    }
}

/// PRD 0014 RF1.4: the publishing task's state. A state change only stores
/// a flag and wakes the task, so it never waits for a file write; the flag
/// coalesces the changes of a busy refresh into the next write.
#[derive(Debug, Default)]
pub struct Publisher {
    /// `[off, on]` of the switch; read by the task and by Settings, never
    /// written by the task, so a plain load is enough.
    prefs: std::sync::atomic::AtomicU8,
    dirty: AtomicBool,
    last_write: std::sync::Mutex<Option<Instant>>,
    nudge: Notify,
}

const OFF: u8 = 0;
const ON: u8 = 1;

impl Publisher {
    pub fn new() -> Self {
        Self::default()
    }

    /// The switch, as the task sees it.
    pub fn is_enabled(&self) -> bool {
        self.prefs.load(Ordering::Acquire) == ON
    }

    /// The switch from Settings (or from `desktop.json` at startup).
    pub fn set_enabled(&self, on: bool) {
        self.prefs
            .store(if on { ON } else { OFF }, Ordering::Release);
    }

    /// A state change happened: mark the snapshot dirty and wake the task.
    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Relaxed);
        self.nudge.notify_one();
    }

    /// How long a write must still wait, `None` when it may happen now.
    /// Pure, so the rate limit is testable without a clock.
    pub fn wait(&self, now: Instant) -> Option<Duration> {
        let last = *self
            .last_write
            .lock()
            .unwrap_or_else(|err| err.into_inner());
        let elapsed = last.map(|last| now.saturating_duration_since(last))?;
        MIN_INTERVAL
            .checked_sub(elapsed)
            .filter(|wait| !wait.is_zero())
    }

    /// Records that a write just happened, starting the next rate-limit
    /// window (called after the file is on disk).
    pub fn wrote(&self, at: Instant) {
        *self
            .last_write
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = Some(at);
    }

    /// Forget the pending change and the rate-limit window (the switch is
    /// off: nothing is waiting to be written).
    pub fn reset(&self) {
        self.dirty.store(false, Ordering::Relaxed);
        *self
            .last_write
            .lock()
            .unwrap_or_else(|err| err.into_inner()) = None;
    }

    /// Take the dirty flag, if set: the write that follows clears it, and
    /// a change during the write marks it again.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    /// Wait for a state change. The task only reads this flag after
    /// waking, so a change that arrives before the wait starts is not
    /// lost (the flag stays set).
    pub async fn wait_nudged(&self) {
        self.nudge.notified().await;
    }

    /// Sleep until the next write may happen, or until a change wakes the
    /// task first (it then re-checks the flag and the window).
    pub async fn wait_for_window(&self) {
        let wait = self.wait(Instant::now()).unwrap_or_default();
        let sleep = tokio::time::sleep(wait);
        tokio::select! {
            () = sleep => {}
            () = self.nudge.notified() => {}
        }
    }
}

/// PRD 0014 RF2.2: remove the snapshot (an absent file is not an error).
pub fn delete(path: Option<&Path>) -> bool {
    if let Some(legacy) = legacy_snapshot_path() {
        let _ = std::fs::remove_file(legacy);
    }
    match path {
        Some(path) => std::fs::remove_file(path).is_ok(),
        None => false,
    }
}

/// Write `snapshot` at `path` off the async runtime (the write is small,
/// but it is disk work the refresh must not wait for). The result is what
/// the core did: bytes, accounts and whether the size cap cut something.
pub async fn publish(
    snapshot: WidgetSnapshot,
    path: PathBuf,
) -> std::io::Result<widget_snapshot::WriteOutcome> {
    tauri::async_runtime::spawn_blocking(move || snapshot.write(&path))
        .await
        .unwrap_or_else(|e| Err(std::io::Error::other(e.to_string())))
}

/// A compact series of the binding metric for the History widget (RF3.2):
/// `used / limit` over `points`, at most [`widget_snapshot::MAX_HISTORY_POINTS`]
/// samples (the newest kept). An empty or unknown series is no series.
pub fn history(
    points: &[cuotai_core::pace::Point],
    limit: Option<f64>,
) -> Vec<widget_snapshot::HistoryPoint> {
    let limit = limit.filter(|limit| limit.is_finite() && *limit > 0.0);
    let mut series: Vec<widget_snapshot::HistoryPoint> = points
        .iter()
        .filter_map(|point| {
            let limit = limit?;
            (point.used.is_finite() && point.used >= 0.0).then(|| widget_snapshot::HistoryPoint {
                taken_at: point.taken_at,
                used_fraction: point.used / limit,
            })
        })
        .collect();
    if series.len() > widget_snapshot::MAX_HISTORY_POINTS {
        // Keep the tail: the newest samples are what a chart shows.
        series.drain(..series.len() - widget_snapshot::MAX_HISTORY_POINTS);
    }
    series
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_always_names_widget_json_and_is_reported() {
        let path = snapshot_path().expect("a per-user directory on every system");
        assert_eq!(path.file_name().unwrap(), FILE_NAME);
        let shown = view(WidgetPrefs::default());
        assert_eq!(
            shown.path.as_deref(),
            Some(path.display().to_string().as_str())
        );
        assert_eq!(shown.macos, cfg!(target_os = "macos"));
        assert_eq!(shown.accounts_min, 1);
        assert_eq!(shown.accounts_max, MAX_WIDGET_ACCOUNTS);
    }

    #[test]
    fn writes_are_rate_limited_and_the_first_is_immediate() {
        let publisher = Publisher::new();
        let now = Instant::now();
        assert!(
            publisher.wait(now).is_none(),
            "the first write waits for nothing"
        );
        publisher.wrote(now);
        let wait = publisher.wait(now + Duration::from_secs(5)).unwrap();
        assert_eq!(wait, Duration::from_secs(10));
        assert!(publisher.wait(now + Duration::from_secs(15)).is_none());
        publisher.reset();
        assert!(
            publisher.wait(now).is_none(),
            "turning widgets off clears the window"
        );
    }

    #[test]
    fn the_dirty_flag_coalesces_changes_and_takes_the_pending_one() {
        let publisher = Publisher::new();
        assert!(!publisher.is_dirty());
        publisher.mark_dirty();
        publisher.mark_dirty();
        assert!(publisher.take_dirty());
        assert!(
            !publisher.take_dirty(),
            "one write covers the changes before it"
        );
        publisher.mark_dirty();
        publisher.reset();
        assert!(!publisher.is_dirty(), "off: nothing stays pending");
    }

    #[test]
    fn deleting_an_absent_file_is_not_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(FILE_NAME);
        assert!(!delete(Some(&path)));
        std::fs::write(&path, b"{}").unwrap();
        assert!(delete(Some(&path)));
        assert!(!path.exists());
        assert!(!delete(None));
    }

    #[test]
    fn history_keeps_the_newest_points_and_needs_a_known_limit() {
        let taken_at = chrono::Utc::now();
        let point = |used| cuotai_core::pace::Point { taken_at, used };
        assert!(history(&[], Some(100.0)).is_empty());
        assert!(
            history(&[point(10.0), point(20.0)], None).is_empty(),
            "no capacity, no percentage"
        );
        let series = history(
            &[
                point(1.0),
                point(f64::NAN),
                point(3.0),
                point(-1.0),
                point(6.0),
            ],
            Some(10.0),
        );
        assert_eq!(series.len(), 3);
        assert!(
            series[0].used_fraction < series[2].used_fraction,
            "the series is the history's own order"
        );
        let long: Vec<_> = (0..widget_snapshot::MAX_HISTORY_POINTS + 40)
            .map(|i| point(i as f64))
            .collect();
        let capped = history(&long, Some(10_000.0));
        assert_eq!(capped.len(), widget_snapshot::MAX_HISTORY_POINTS);
        assert_eq!(
            capped.last().unwrap().used_fraction,
            (widget_snapshot::MAX_HISTORY_POINTS as f64 + 39.0) / 10_000.0,
            "the newest sample survives"
        );
    }
}
