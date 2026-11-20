//! PRD 0014 RF1.4/RF2.2: publishing the widget snapshot of this device.
//!
//! The core builds the file ([`cuotai_core::widget_snapshot`]) from the
//! `export::v1` the app already shows; this module owns where it lives, who
//! decided that, and when it is written. Publishing runs in its own task,
//! never inside the refresh: every state change only marks it dirty, and
//! the task coalesces those marks and writes at most once every
//! [`MIN_INTERVAL`]. With the switch off nothing is written and an existing
//! file is deleted.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use cuotai_core::widget_snapshot::{self, WidgetSnapshot};
use serde::Serialize;
use tokio::sync::Notify;

use crate::settings::{WidgetPrefs, MAX_WIDGET_ACCOUNTS};

/// RF1.4: at most one write per 15 seconds. Changes inside that window are
/// coalesced into the next write instead of lost.
pub const MIN_INTERVAL: Duration = Duration::from_secs(15);

/// PRD 0014 RF3.1: the extension the release embeds in the app bundle.
#[cfg(target_os = "macos")]
const EXTENSION: &str = "CuotaiWidgets.appex";

/// PRD 0014 RF5: the file name every consumer looks for.
pub const FILE_NAME: &str = "widget.json";

/// Where the widget file lives and every place an earlier copy may be,
/// built from a home directory and a data root so the tests use temporary
/// folders and never this user's real files.
#[derive(Debug, Clone, Default)]
pub struct Locations {
    pub home: Option<PathBuf>,
    /// The data root ([`cuotai_core::data_dir::root`]): `CUOTAI_DATA_DIR`
    /// or the Settings data folder, else the OS default.
    pub data_root: Option<PathBuf>,
    pub macos: bool,
}

impl Locations {
    /// This user's home and data root.
    #[cfg(not(test))]
    pub fn system() -> Self {
        Self {
            home: dirs::home_dir(),
            data_root: cuotai_core::data_dir::root(),
            macos: cfg!(target_os = "macos"),
        }
    }

    /// Under test, a fixed temporary home and data root: no test can reach
    /// the real widget file, whatever code path it drives.
    #[cfg(test)]
    pub fn system() -> Self {
        let base = std::env::temp_dir().join("cuotai-widget-tests");
        Self {
            home: Some(base.join("home")),
            data_root: Some(base.join("data")),
            macos: cfg!(target_os = "macos"),
        }
    }

    /// Where the app writes the snapshot. PRD 0014 RF2.1: on macOS always
    /// `~/Library/Application Support/cuotai/widget.json`, the one file the
    /// extension's sandbox may read (`CuotaiWidgets.entitlements`), even
    /// when the data folder moves; elsewhere the data root (RF5).
    pub fn snapshot(&self) -> Option<PathBuf> {
        if self.macos {
            Some(self.home.as_deref()?.join(widget_snapshot::MACOS_PATH))
        } else {
            Some(self.data_root.as_deref()?.join(FILE_NAME))
        }
    }

    /// RF2.2: every copy turning widgets off removes: the file, the one an
    /// earlier release left in a moved data root, the *app group* file of
    /// releases before 0.82.2, and the `.bak` kept beside each of the
    /// first two (PRD 0051 RF3.4).
    pub fn copies(&self) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = Vec::new();
        files.extend(self.snapshot());
        files.extend(self.data_root.as_deref().map(|root| root.join(FILE_NAME)));
        files.dedup();
        let backups: Vec<PathBuf> = files
            .iter()
            .map(|file| {
                let mut name = file.as_os_str().to_os_string();
                name.push(".bak");
                PathBuf::from(name)
            })
            .collect();
        files.extend(backups);
        if self.macos {
            files.extend(
                self.home
                    .as_deref()
                    .map(|home| home.join(widget_snapshot::MACOS_LEGACY_PATH)),
            );
        }
        files
    }
}

/// Where the snapshot lives on this system ([`Locations::snapshot`]).
pub fn snapshot_path() -> Option<PathBuf> {
    Locations::system().snapshot()
}

/// Whether this executable runs from a bundle that embeds the widget
/// extension (`Cuotai.app/Contents/{MacOS/cuotai,PlugIns/…appex}`).
fn embeds_extension() -> bool {
    #[cfg(target_os = "macos")]
    {
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent()?.parent().map(std::path::Path::to_path_buf))
            .is_some_and(|contents| contents.join("PlugIns").join(EXTENSION).is_dir())
    }
    #[cfg(not(target_os = "macos"))]
    {
        false
    }
}

/// The extension's executable inside a bundle's `Contents` folder.
#[cfg(target_os = "macos")]
fn extension_executable(contents: &std::path::Path) -> PathBuf {
    contents
        .join("PlugIns")
        .join(EXTENSION)
        .join("Contents")
        .join("MacOS")
        .join("CuotaiWidgets")
}

/// When the extension's executable reached the disk: the later of its
/// modification and status-change times. An update keeps the build's
/// modification time, but writing the file at install sets its change time,
/// which nothing can set back.
// Only macOS embeds the extension; the tests run everywhere.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn installed_at(meta: &std::fs::Metadata) -> Option<std::time::SystemTime> {
    let modified = meta.modified().ok()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let changed = std::time::UNIX_EPOCH
            .checked_add(Duration::from_secs(u64::try_from(meta.ctime()).ok()?))?;
        Some(modified.max(changed))
    }
    #[cfg(not(unix))]
    {
        Some(modified)
    }
}

/// PRD 0014 RF3.1: whether a running extension process is older than the
/// extension on disk. Its start time has a one-second resolution, so a
/// process started in the second of the install counts as current.
// Only macOS embeds the extension; the tests run everywhere.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn predates(started_secs: u64, installed: std::time::SystemTime) -> bool {
    installed
        .duration_since(std::time::UNIX_EPOCH)
        .is_ok_and(|installed| started_secs < installed.as_secs())
}

/// PRD 0014 RF3.1: stop the widget extension processes an update left
/// running, and return how many were stopped.
///
/// WidgetKit keeps an extension process alive for days. When an update
/// replaces the app, that process still runs the previous build, and chronod
/// refuses everything it draws ("Bundle version did not match"): every
/// widget stays on the grey placeholder until the process ends (shipped from
/// 0.87.1 to 0.87.6). A stopped process costs nothing: the system keeps
/// showing the last timeline and starts the installed build for the next
/// one. Only processes of this user, started from this bundle's extension
/// and before that extension was installed, are stopped.
#[cfg(target_os = "macos")]
pub fn retire_stale_extension() -> usize {
    let Some(executable) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent()?.parent().map(extension_executable))
    else {
        return 0;
    };
    let Some(installed) = std::fs::metadata(&executable)
        .ok()
        .as_ref()
        .and_then(installed_at)
    else {
        return 0;
    };
    let mut stopped = 0;
    for pid in process::all() {
        if process::path(pid).as_deref() != Some(executable.as_path()) {
            continue;
        }
        let Some(started) = process::started_by_this_user(pid) else {
            continue;
        };
        if predates(started, installed) && process::terminate(pid) {
            stopped += 1;
        }
    }
    stopped
}

/// The few process calls [`retire_stale_extension`] needs (libproc).
#[cfg(target_os = "macos")]
mod process {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::path::PathBuf;

    /// Every process id of the system.
    pub fn all() -> Vec<libc::pid_t> {
        // SAFETY: a null buffer asks for the count only.
        let count = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
        let Ok(count) = usize::try_from(count) else {
            return Vec::new();
        };
        // Room for processes started between the two calls.
        let mut pids = vec![0 as libc::pid_t; count + 64];
        let bytes = i32::try_from(std::mem::size_of_val(pids.as_slice())).unwrap_or(i32::MAX);
        // SAFETY: the buffer holds `bytes` bytes of pid_t.
        let filled = unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast(), bytes) };
        pids.truncate(usize::try_from(filled).unwrap_or(0).min(pids.len()));
        pids.retain(|&pid| pid > 0);
        pids
    }

    /// The executable a process was started from.
    pub fn path(pid: libc::pid_t) -> Option<PathBuf> {
        let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let size = u32::try_from(buf.len()).ok()?;
        // SAFETY: the buffer holds `size` bytes.
        let len = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), size) };
        let len = usize::try_from(len).ok().filter(|&len| len > 0)?;
        Some(PathBuf::from(OsStr::from_bytes(&buf[..len.min(buf.len())])))
    }

    /// When a process of this user started, in seconds since the epoch;
    /// `None` for another user's process or one that already ended.
    pub fn started_by_this_user(pid: libc::pid_t) -> Option<u64> {
        let mut info = std::mem::MaybeUninit::<libc::proc_bsdinfo>::zeroed();
        let size = i32::try_from(std::mem::size_of::<libc::proc_bsdinfo>()).ok()?;
        // SAFETY: the buffer is one `proc_bsdinfo`, the flavour asked for.
        let filled = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                info.as_mut_ptr().cast(),
                size,
            )
        };
        if filled != size {
            return None;
        }
        // SAFETY: the call filled the whole structure.
        let info = unsafe { info.assume_init() };
        // SAFETY: getuid cannot fail.
        (info.pbi_uid == unsafe { libc::getuid() }).then_some(info.pbi_start_tvsec)
    }

    /// Ask a process to end.
    pub fn terminate(pid: libc::pid_t) -> bool {
        // SAFETY: a plain signal to a process id.
        unsafe { libc::kill(pid, libc::SIGTERM) == 0 }
    }
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

/// PRD 0014 RF2.2: remove every copy of the snapshot at `locations`
/// ([`Locations::copies`]). `true` when one was there; an absent file is
/// not an error.
pub fn delete(locations: &Locations) -> bool {
    let mut removed = false;
    for path in locations.copies() {
        removed |= std::fs::remove_file(path).is_ok();
    }
    removed
}

/// [`delete`] at this system's [`Locations`].
pub fn delete_all() -> bool {
    delete(&Locations::system())
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
    let mut series: Vec<widget_snapshot::HistoryPoint> = points
        .iter()
        .filter_map(|point| {
            Some(widget_snapshot::HistoryPoint {
                taken_at: point.taken_at,
                used_fraction: widget_snapshot::used_fraction(point.used, limit?)?,
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
    fn an_extension_process_is_stale_only_when_it_started_before_the_install() {
        let installed = std::time::UNIX_EPOCH + Duration::from_millis(1_000_500);
        assert!(predates(999, installed), "started before the install");
        assert!(!predates(1_000, installed), "the second of the install");
        assert!(!predates(1_001, installed), "started from the new build");
        assert!(!predates(0, std::time::UNIX_EPOCH), "no install time");
    }

    #[cfg(unix)]
    #[test]
    fn the_install_time_is_when_the_file_was_written_not_its_build_time() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("CuotaiWidgets");
        std::fs::write(&file, b"x").unwrap();
        // An update unpacks the build with its old modification time.
        let old = std::time::UNIX_EPOCH + Duration::from_secs(1_000);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let meta = std::fs::metadata(&file).unwrap();
        assert_eq!(meta.modified().unwrap(), old);
        let installed = installed_at(&meta).unwrap();
        let now = std::time::SystemTime::now();
        assert!(installed > old + Duration::from_secs(60));
        assert!(installed <= now + Duration::from_secs(1));
        // A process from before the unpack is stale even though the build
        // is older than it.
        assert!(predates(2_000, installed));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_extension_executable_is_the_one_the_bundle_embeds() {
        assert_eq!(
            extension_executable(std::path::Path::new("/A/Cuotai.app/Contents")),
            std::path::Path::new(
                "/A/Cuotai.app/Contents/PlugIns/CuotaiWidgets.appex/Contents/MacOS/CuotaiWidgets"
            )
        );
        // The test binary embeds no extension: nothing is stopped.
        assert_eq!(retire_stale_extension(), 0);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn this_process_is_listed_with_its_path_and_start_time() {
        let me = libc::pid_t::try_from(std::process::id()).unwrap();
        assert!(process::all().contains(&me));
        assert_eq!(
            process::path(me).unwrap().canonicalize().unwrap(),
            std::env::current_exe().unwrap().canonicalize().unwrap()
        );
        let started = process::started_by_this_user(me).unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert!(started <= now && started + 3_600 > now);
        // launchd (pid 1) belongs to root.
        assert_eq!(process::started_by_this_user(1), None);
    }

    fn temp_locations(dir: &std::path::Path, macos: bool, moved: bool) -> Locations {
        let home = dir.join("home");
        let data_root = if moved {
            dir.join("moved")
        } else if macos {
            home.join("Library")
                .join("Application Support")
                .join("cuotai")
        } else {
            dir.join("data")
        };
        Locations {
            home: Some(home),
            data_root: Some(data_root),
            macos,
        }
    }

    #[test]
    fn the_macos_file_stays_where_the_extension_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let fixed = dir
            .path()
            .join("home/Library/Application Support/cuotai")
            .join(FILE_NAME);
        let moved = temp_locations(dir.path(), true, true);
        assert_eq!(moved.snapshot(), Some(fixed.clone()), "a moved data folder");
        let default = temp_locations(dir.path(), true, false);
        assert_eq!(default.snapshot(), Some(fixed));
        let linux = temp_locations(dir.path(), false, true);
        assert_eq!(
            linux.snapshot(),
            Some(dir.path().join("moved").join(FILE_NAME)),
            "elsewhere the file follows the data root"
        );
        assert_eq!(Locations::default().snapshot(), None);
    }

    #[test]
    fn deleting_removes_every_copy_and_an_absent_file_is_not_a_failure() {
        let dir = tempfile::tempdir().unwrap();
        let locations = temp_locations(dir.path(), true, true);
        assert!(!delete(&locations), "nothing there yet");
        let copies = locations.copies();
        let expected = [
            "home/Library/Application Support/cuotai/widget.json",
            "moved/widget.json",
            "home/Library/Application Support/cuotai/widget.json.bak",
            "moved/widget.json.bak",
            "home/Library/Group Containers/WL9XSC6H36.dev.cuotai.desktop/widget.json",
        ]
        .map(|rel| dir.path().join(rel));
        assert_eq!(copies, expected);
        for copy in &copies {
            std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
            std::fs::write(copy, b"{}").unwrap();
        }
        assert!(delete(&locations));
        assert!(copies.iter().all(|copy| !copy.exists()));
        assert!(!delete(&locations));

        // The default macOS data folder is the fixed one: listed once.
        let default = temp_locations(dir.path(), true, false);
        assert_eq!(default.copies().len(), 3);
        // Elsewhere: the file and its backup in the data root.
        let linux = temp_locations(dir.path(), false, false);
        assert_eq!(
            linux.copies(),
            vec![
                dir.path().join("data").join(FILE_NAME),
                dir.path().join("data").join("widget.json.bak"),
            ]
        );
        assert!(!delete(&Locations::default()));
    }

    #[test]
    fn tests_never_reach_the_real_home() {
        let system = Locations::system();
        let base = std::env::temp_dir();
        assert!(system.home.unwrap().starts_with(&base));
        assert!(system.data_root.unwrap().starts_with(&base));
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
        let huge = history(&[point(1e20)], Some(1.0));
        assert_eq!(
            huge[0].used_fraction,
            widget_snapshot::MAX_USED_FRACTION,
            "an extreme sample is capped, never huge"
        );
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
