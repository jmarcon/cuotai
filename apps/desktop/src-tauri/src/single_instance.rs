//! The app's single instance on macOS (security review 2026-11-21, L-10).
//!
//! `tauri-plugin-single-instance` 2.5.5 keeps its socket at
//! `/tmp/<identifier>_si.sock`. `/tmp` is shared by every account of the
//! Mac and sticky, and a launch trusts whatever listens there: another
//! user who created a listening socket at that path received every later
//! launch's working directory and arguments, and each launch then exited
//! with status 0, so the app could not start for the victim, who cannot
//! remove the file either.
//!
//! This module does the same job with the socket in the user's own
//! temporary folder (`confstr(_CS_DARWIN_USER_TEMP_DIR)`, a `0700` folder
//! under `/var/folders` that no other user can enter), and trusts a socket
//! there only when it belongs to the current user. Without that folder the
//! app starts without a single instance rather than fall back to `/tmp`.
//! Windows and Linux keep the plugin, whose named mutex and session bus are
//! already per user.

use std::io::{ErrorKind, Write};
use std::os::unix::fs::MetadataExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{AppHandle, Manager, RunEvent, Runtime};
use tokio::io::AsyncReadExt;

/// The socket's name inside the user's temporary folder.
const SOCKET_NAME: &str = "cuotai-desktop-si.sock";

/// The longest message a second launch may send (working directory and
/// arguments).
const MAX_MESSAGE: u64 = 64 * 1024;

type Callback<R> = Box<dyn Fn(&AppHandle<R>, Vec<String>, String) + Send + Sync + 'static>;

/// The socket's path, `None` when the per-user temporary folder is unknown.
fn socket_path() -> Option<PathBuf> {
    let mut buffer = vec![0u8; libc::PATH_MAX as usize];
    // SAFETY: `buffer` is writable for its whole length, which is passed.
    let needed = unsafe {
        libc::confstr(
            libc::_CS_DARWIN_USER_TEMP_DIR,
            buffer.as_mut_ptr().cast(),
            buffer.len(),
        )
    };
    if needed == 0 || needed > buffer.len() {
        return None;
    }
    buffer.truncate(needed - 1);
    let dir = PathBuf::from(String::from_utf8(buffer).ok()?);
    dir.is_absolute().then(|| dir.join(SOCKET_NAME))
}

/// The current user's effective uid.
fn me() -> u32 {
    // SAFETY: `geteuid` has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// Whether `path` exists (never followed when a link) and belongs to `uid`.
fn owned_by(path: &Path, uid: u32) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.uid() == uid)
}

/// What a launch should do with the socket at `path`.
#[derive(Debug, PartialEq, Eq)]
enum Start {
    /// Another instance of this user took the message: exit.
    Notified,
    /// Nobody listens: become the instance.
    Claim,
    /// The socket cannot be trusted or used: start without a single
    /// instance.
    Alone,
}

/// Hand `message` to the instance listening at `path`, if any, and only
/// when the socket belongs to `uid`.
fn notify(path: &Path, uid: u32, message: &[u8]) -> Start {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => return Start::Claim,
        Err(_) => return Start::Alone,
        Ok(_) if !owned_by(path, uid) => return Start::Alone,
        Ok(_) => {}
    }
    match UnixStream::connect(path) {
        Ok(mut stream) => {
            if stream.write_all(message).is_ok() {
                Start::Notified
            } else {
                Start::Alone
            }
        }
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::NotFound | ErrorKind::ConnectionRefused
            ) =>
        {
            Start::Claim
        }
        Err(_) => Start::Alone,
    }
}

/// The working directory and arguments, as the plugin sends them.
fn message() -> Vec<u8> {
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let args = std::env::args_os()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\0");
    format!("{cwd}\0\0{args}").into_bytes()
}

/// A second launch: hand the working directory and arguments to the
/// instance of this user already running. `true` when it took them, and
/// this process should end without opening anything.
pub fn hand_over() -> bool {
    socket_path().is_some_and(|path| notify(&path, me(), &message()) == Start::Notified)
}

/// The plugin of the first instance: it holds the socket and hands each
/// later launch's arguments to `callback`. Registered after [`hand_over`]
/// answered `false`.
pub fn init<R: Runtime>(
    callback: impl Fn(&AppHandle<R>, Vec<String>, String) + Send + Sync + 'static,
) -> TauriPlugin<R> {
    let callback: Callback<R> = Box::new(callback);
    Builder::new("cuotai-single-instance")
        .setup(move |app, _api| {
            let Some(path) = socket_path() else {
                return Ok(());
            };
            // A stale socket of this user is replaced; someone else's file
            // (impossible in a private folder) is left, with no instance.
            let claimable = match std::fs::symlink_metadata(&path) {
                Err(error) => error.kind() == ErrorKind::NotFound,
                Ok(_) => owned_by(&path, me()),
            };
            if claimable {
                let _ = std::fs::remove_file(&path);
                if let Ok(listener) = std::os::unix::net::UnixListener::bind(&path) {
                    app.manage(Claimed(path));
                    listen(listener, app.clone(), callback);
                }
            }
            Ok(())
        })
        .on_event(|app, event| {
            if let RunEvent::Exit = event {
                if let Some(claimed) = app.try_state::<Claimed>() {
                    let _ = std::fs::remove_file(&claimed.0);
                }
            }
        })
        .build()
}

/// The socket this instance owns, removed on exit.
struct Claimed(PathBuf);

fn listen<R: Runtime>(
    listener: std::os::unix::net::UnixListener,
    app: AppHandle<R>,
    callback: Callback<R>,
) {
    tauri::async_runtime::spawn(async move {
        let _ = listener.set_nonblocking(true);
        let Ok(listener) = tokio::net::UnixListener::from_std(listener) else {
            return;
        };
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            // The folder is private, but the peer is checked anyway.
            if stream.peer_cred().map(|cred| cred.uid()).ok() != Some(me()) {
                continue;
            }
            let mut text = String::new();
            if stream
                .take(MAX_MESSAGE)
                .read_to_string(&mut text)
                .await
                .is_err()
            {
                continue;
            }
            let (cwd, args) = text.split_once("\0\0").unwrap_or_default();
            let args = args.split('\0').map(String::from).collect();
            callback(&app, args, cwd.to_owned());
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_socket_lives_in_the_users_private_temporary_folder() {
        let path = socket_path().expect("macOS names a per-user temporary folder");
        assert!(!path.starts_with("/tmp"), "{}", path.display());
        let dir = path.parent().unwrap();
        let meta = std::fs::metadata(dir).unwrap();
        assert_eq!(meta.uid(), me());
        assert_eq!(meta.mode() & 0o077, 0, "no access for others");
        assert!(path.as_os_str().len() < 104, "fits sun_path");
    }

    /// L-10: a socket owned by another user is never trusted with the
    /// launch's message, and never makes the launch exit.
    #[test]
    fn a_foreign_socket_is_not_trusted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("si.sock");
        let _listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert_eq!(notify(&path, me().wrapping_add(1), b"x\0\0y"), Start::Alone);
        assert_eq!(notify(&path, me(), b"x\0\0y"), Start::Notified);
        assert_eq!(
            notify(&dir.path().join("absent.sock"), me(), b""),
            Start::Claim
        );
    }
}
