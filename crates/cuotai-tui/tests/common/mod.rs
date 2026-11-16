//! Helpers shared by the TUI integration tests (`mod common;`).
//!
//! Every test that writes a fake program (a script) or copies a binary and
//! then executes it goes through [`write_executable`] or
//! [`copy_executable`]. On Linux, executing a file that some process still
//! holds open for writing fails with `ETXTBSY` ("text file busy"). Writing
//! the file from the test process leaves that window open: another test
//! thread that spawns a child in the meantime gives the child a copy of the
//! write handle until the child's own `exec` closes it (`O_CLOEXEC` closes it
//! at `exec`, not at `fork`). No `sync` or close in this thread helps, and
//! neither does a rename (the inode is the same). A short-lived `/bin/sh`
//! child writes the file instead, so the write handle exists only in that
//! child, which has exited before the helper returns. Same fix as the core's
//! `write_executable` (cfg(test)).

#![allow(dead_code)] // each test binary uses only some of the helpers

use std::io;
use std::path::Path;

/// Writes `contents` to `path` as an executable (mode 755 on Unix).
pub fn write_executable(path: &Path, contents: impl AsRef<[u8]>) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut child = Command::new("/bin/sh")
            .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
            .arg(path)
            .stdin(Stdio::piped())
            .spawn()?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        stdin.write_all(contents.as_ref())?;
        drop(stdin);
        finished(child.wait()?, path)
    }
    #[cfg(not(unix))]
    std::fs::write(path, contents)
}

/// Copies the executable at `from` to `to` (mode 755 on Unix).
pub fn copy_executable(from: &Path, to: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        let status = std::process::Command::new("/bin/sh")
            .args(["-c", "cp \"$1\" \"$2\" && chmod 755 \"$2\"", "sh"])
            .arg(from)
            .arg(to)
            .status()?;
        finished(status, to)
    }
    #[cfg(not(unix))]
    std::fs::copy(from, to).map(drop)
}

#[cfg(unix)]
fn finished(status: std::process::ExitStatus, path: &Path) -> io::Result<()> {
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "/bin/sh could not write {} ({status})",
            path.display()
        )))
    }
}
