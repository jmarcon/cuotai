//! PRD 0050 RF9 (re-validation of PRD 0006, 2026-11): the files the desktop
//! writes or reads at the person's request (a history export, a backup, the
//! backup a restore reads) are chosen in an operating-system dialog that
//! Rust opens. The webview asks for the action only and never supplies a
//! path, so a compromised page cannot aim a write at an arbitrary file or
//! read one. A restore reads the chosen file twice (the preview, then the
//! apply): the preview keeps the path in [`PendingRestore`] under a one-shot
//! token, and the apply accepts only that token. The core's
//! [`cuotai_core::output_file`] guard still refuses Cuotai's own files as
//! defense in depth.
//!
//! The commands take any [`FilePicker`], so the tests drive them with a fake
//! that picks a path or cancels.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

/// What a save dialog proposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveRequest {
    /// The suggested file name.
    pub name: String,
    /// The filter's label and its one extension (no dot).
    pub filter: String,
    pub extension: String,
}

/// Shows the native file dialogs; `None` when the person cancels or the
/// dialog cannot be shown.
pub trait FilePicker: Send + Sync {
    fn save<'a>(
        &'a self,
        request: SaveRequest,
    ) -> Pin<Box<dyn Future<Output = Option<PathBuf>> + Send + 'a>>;
    fn open<'a>(
        &'a self,
        filter: String,
        extension: String,
    ) -> Pin<Box<dyn Future<Output = Option<PathBuf>> + Send + 'a>>;
}

/// The dialogs of `tauri-plugin-dialog`, created by Rust.
pub struct DialogPicker {
    app: tauri::AppHandle,
}

impl DialogPicker {
    pub fn new(app: &tauri::AppHandle) -> Self {
        Self { app: app.clone() }
    }
}

impl FilePicker for DialogPicker {
    fn save<'a>(
        &'a self,
        request: SaveRequest,
    ) -> Pin<Box<dyn Future<Output = Option<PathBuf>> + Send + 'a>> {
        use tauri_plugin_dialog::DialogExt;
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.app
                .dialog()
                .file()
                .set_file_name(request.name)
                .add_filter(request.filter, &[request.extension.as_str()])
                .save_file(move |path| {
                    let _ = tx.send(path.and_then(|path| path.into_path().ok()));
                });
            rx.await.ok().flatten()
        })
    }

    fn open<'a>(
        &'a self,
        filter: String,
        extension: String,
    ) -> Pin<Box<dyn Future<Output = Option<PathBuf>> + Send + 'a>> {
        use tauri_plugin_dialog::DialogExt;
        Box::pin(async move {
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.app
                .dialog()
                .file()
                .add_filter(filter, &[extension.as_str()])
                .pick_file(move |path| {
                    let _ = tx.send(path.and_then(|path| path.into_path().ok()));
                });
            rx.await.ok().flatten()
        })
    }
}

/// The backup file the last restore dialog chose, under its token. The
/// token only ties the apply to the file the preview read (the security
/// property is that the path never comes from the webview), so a process
/// counter is enough.
#[derive(Default)]
pub struct PendingRestore {
    next: std::sync::atomic::AtomicU64,
    slot: std::sync::Mutex<Option<(u64, PathBuf)>>,
}

impl PendingRestore {
    /// Keep `path` for the apply; any earlier pick is forgotten. Returns its
    /// token.
    pub fn put(&self, path: PathBuf) -> u64 {
        let token = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if let Ok(mut slot) = self.slot.lock() {
            *slot = Some((token, path));
        }
        token
    }

    /// The path kept under `token`; `None` for any other token.
    pub fn get(&self, token: u64) -> Option<PathBuf> {
        let slot = self.slot.lock().ok()?;
        slot.as_ref()
            .filter(|(kept, _)| *kept == token)
            .map(|(_, path)| path.clone())
    }

    /// Forget the pick under `token` (after a successful restore).
    pub fn forget(&self, token: u64) {
        if let Ok(mut slot) = self.slot.lock() {
            if slot.as_ref().is_some_and(|(kept, _)| *kept == token) {
                *slot = None;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;

    /// A picker that answers every dialog with `answer` (or cancels with
    /// `None`) and records the save requests it was shown.
    pub(crate) struct FakePicker {
        pub answer: Option<PathBuf>,
        pub asked: Mutex<Vec<SaveRequest>>,
    }

    impl FakePicker {
        pub(crate) fn new(answer: Option<PathBuf>) -> Self {
            Self {
                answer,
                asked: Mutex::new(Vec::new()),
            }
        }
    }

    impl FilePicker for FakePicker {
        fn save<'a>(
            &'a self,
            request: SaveRequest,
        ) -> Pin<Box<dyn Future<Output = Option<PathBuf>> + Send + 'a>> {
            self.asked.lock().unwrap().push(request);
            Box::pin(async move { self.answer.clone() })
        }

        fn open<'a>(
            &'a self,
            _filter: String,
            _extension: String,
        ) -> Pin<Box<dyn Future<Output = Option<PathBuf>> + Send + 'a>> {
            Box::pin(async move { self.answer.clone() })
        }
    }

    #[test]
    fn a_restore_pick_is_found_only_by_its_own_token() {
        let pending = PendingRestore::default();
        assert_eq!(pending.get(1), None);
        let first = pending.put(PathBuf::from("/a.zip"));
        let second = pending.put(PathBuf::from("/b.zip"));
        assert_ne!(first, second);
        assert_eq!(pending.get(first), None, "a newer pick replaces the older");
        assert_eq!(pending.get(second), Some(PathBuf::from("/b.zip")));
        pending.forget(first);
        assert!(
            pending.get(second).is_some(),
            "another token forgets nothing"
        );
        pending.forget(second);
        assert_eq!(pending.get(second), None);
    }
}
