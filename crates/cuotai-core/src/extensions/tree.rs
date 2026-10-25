//! FIX-04/FIX-05 (2026-10-21 review): the digest of an extension's whole
//! folder, and the Unix ownership and permission checks that make it worth
//! something.
//!
//! [`scan`] walks the folder in a stable order (entries sorted by name
//! bytes, depth first) and hashes one record per entry — nothing is
//! excluded, hidden files and folders included:
//!
//! - a folder: its relative path;
//! - a regular file: its relative path, whether it is executable (Unix) and
//!   the SHA-256 of its bytes;
//! - a symbolic link: its relative path and the link text, **not followed**.
//!   A link must resolve inside the folder (its target is then hashed as its
//!   own entry); a link that leads outside the folder, or nowhere, is
//!   refused, because what it points at could change unseen;
//! - anything else (a FIFO, a socket, a device) is refused.
//!
//! On Unix the folder and everything in it must belong to the current user
//! (or root) and must not be writable by the group or by others: otherwise
//! another account could swap a file between the verification and the
//! spawn of a run, so the extension is refused instead. The walk is bounded
//! ([`MAX_ENTRIES`], [`MAX_BYTES`], [`MAX_DEPTH`]); a larger folder is
//! refused rather than hashed partially.

use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::private_fs::{foreign_owner, writable_by_others};
use crate::text::Localized;

/// Most entries (files, folders and links) a folder may hold.
pub const MAX_ENTRIES: usize = 4096;
/// Most bytes of regular files a folder may hold.
pub const MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Deepest nesting of folders inside the extension's folder.
pub const MAX_DEPTH: usize = 32;

/// Why a folder cannot be hashed. Never carries a path or a file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refused {
    /// Something could not be read.
    Unreadable,
    /// A link leads outside the folder (or nowhere), or an entry is not a
    /// regular file, a folder or a link.
    Escapes,
    /// Writable by the group or by others, or owned by another user (Unix).
    Exposed,
    /// Past [`MAX_ENTRIES`], [`MAX_BYTES`] or [`MAX_DEPTH`].
    TooLarge,
}

impl Refused {
    /// The reason a discovery shows for a refused folder.
    pub(crate) fn reason(self) -> Localized {
        match self {
            Self::Unreadable => Localized::fixed(
                "extensions.error.tree_unreadable",
                "Not every file of the extension's folder could be read.",
            ),
            Self::Escapes => Localized::fixed(
                "extensions.error.tree_escapes",
                "The extension's folder holds a symbolic link that leads out of it (or nowhere), or a file that is not a regular file, folder or link; Cuotai does not approve what it cannot check.",
            ),
            Self::Exposed => Localized::fixed(
                "extensions.error.tree_exposed",
                "The extension's folder, something in it or a folder above it up to the extensions folder can be changed by other users (writable by the group or by everyone, or owned by someone else); fix it with chmod go-w and chown.",
            ),
            Self::TooLarge => Localized::with_params(
                "extensions.error.tree_too_large",
                crate::text::params([
                    ("entries", MAX_ENTRIES.into()),
                    ("mib", (MAX_BYTES / (1024 * 1024)).into()),
                    ("depth", MAX_DEPTH.into()),
                ]),
                "The extension's folder exceeds {entries} entries, {mib} MiB or {depth} levels of folders.",
            ),
        }
    }
}

/// The SHA-256 (lowercase hex) of the whole folder `dir` (with links
/// already resolved, as [`super::Manifest::dir`] is), or why it is refused.
pub(crate) fn scan(dir: &Path) -> Result<String, Refused> {
    let meta = std::fs::symlink_metadata(dir).map_err(|_| Refused::Unreadable)?;
    if !meta.is_dir() {
        return Err(Refused::Unreadable);
    }
    guard(&meta)?;
    let mut walk = Walk {
        root: dir,
        hasher: Sha256::new(),
        entries: 0,
        bytes: 0,
    };
    walk.folder(dir, &mut Vec::new(), 0)?;
    Ok(format!("{:x}", walk.hasher.finalize()))
}

/// FIX-05: [`scan`], after checking that no folder above `dir` up to the
/// extensions `root` (the root included) is writable by the group or by
/// others or owned by another user (Unix): otherwise the extension's folder
/// could be swapped by a rename between the verification and a run. A
/// folder that resolves outside `root` (a symbolic link in it) has its own
/// parent and the root checked.
pub(crate) fn scan_within(dir: &Path, root: &Path) -> Result<String, Refused> {
    let root = root.canonicalize().map_err(|_| Refused::Unreadable)?;
    let mut above: Vec<&Path> = Vec::new();
    if dir.starts_with(&root) {
        above.extend(
            dir.ancestors()
                .skip(1)
                .take_while(|ancestor| ancestor.starts_with(&root)),
        );
    } else {
        above.extend(dir.parent());
        above.push(&root);
    }
    for ancestor in above {
        let meta = std::fs::symlink_metadata(ancestor).map_err(|_| Refused::Unreadable)?;
        guard(&meta)?;
    }
    scan(dir)
}

/// The Unix ownership and permission rule, for one entry.
fn guard(meta: &std::fs::Metadata) -> Result<(), Refused> {
    if writable_by_others(meta) || foreign_owner(meta) {
        return Err(Refused::Exposed);
    }
    Ok(())
}

struct Walk<'a> {
    root: &'a Path,
    hasher: Sha256,
    entries: usize,
    bytes: u64,
}

impl Walk<'_> {
    fn record(&mut self, kind: u8, relative: &[u8], tail: &[u8]) {
        // Lengths first, so no two different trees ever hash the same
        // byte stream.
        self.hasher.update([kind]);
        self.hasher.update((relative.len() as u64).to_le_bytes());
        self.hasher.update(relative);
        self.hasher.update((tail.len() as u64).to_le_bytes());
        self.hasher.update(tail);
    }

    fn count(&mut self) -> Result<(), Refused> {
        self.entries += 1;
        if self.entries > MAX_ENTRIES {
            return Err(Refused::TooLarge);
        }
        Ok(())
    }

    fn folder(&mut self, dir: &Path, relative: &mut Vec<u8>, depth: usize) -> Result<(), Refused> {
        if depth > MAX_DEPTH {
            return Err(Refused::TooLarge);
        }
        let mut names: Vec<std::ffi::OsString> = std::fs::read_dir(dir)
            .map_err(|_| Refused::Unreadable)?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<io::Result<_>>()
            .map_err(|_| Refused::Unreadable)?;
        names.sort_by(|a, b| a.as_encoded_bytes().cmp(b.as_encoded_bytes()));
        for name in names {
            self.count()?;
            let path = dir.join(&name);
            let start = relative.len();
            if !relative.is_empty() {
                relative.push(b'/');
            }
            relative.extend_from_slice(name.as_encoded_bytes());
            self.entry(&path, relative, depth)?;
            relative.truncate(start);
        }
        Ok(())
    }

    fn entry(&mut self, path: &Path, relative: &mut Vec<u8>, depth: usize) -> Result<(), Refused> {
        let meta = std::fs::symlink_metadata(path).map_err(|_| Refused::Unreadable)?;
        let kind = meta.file_type();
        if kind.is_symlink() {
            let target = std::fs::read_link(path).map_err(|_| Refused::Unreadable)?;
            let resolved: PathBuf = path.canonicalize().map_err(|_| Refused::Escapes)?;
            if !resolved.starts_with(self.root) {
                return Err(Refused::Escapes);
            }
            let target = target.into_os_string();
            self.record(b'L', relative, target.as_encoded_bytes());
            return Ok(());
        }
        guard(&meta)?;
        if kind.is_dir() {
            self.record(b'D', relative, &[]);
            return self.folder(path, relative, depth + 1);
        }
        if !kind.is_file() {
            return Err(Refused::Escapes);
        }
        self.bytes = self.bytes.saturating_add(meta.len());
        if self.bytes > MAX_BYTES {
            return Err(Refused::TooLarge);
        }
        let mut file = std::fs::File::open(path).map_err(|_| Refused::Unreadable)?;
        let mut content = Sha256::new();
        io::copy(&mut file, &mut content).map_err(|_| Refused::Unreadable)?;
        let mut tail = vec![u8::from(executable(&meta))];
        tail.extend_from_slice(&content.finalize());
        self.record(b'F', relative, &tail);
        Ok(())
    }
}

/// Whether a regular file has any executable bit (Unix); always `false`
/// elsewhere.
fn executable(meta: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        false
    }
}

/// The interpreter line of `program` (the text after `#!` on its first
/// line, trimmed and with control characters replaced) when it is a script,
/// `None` for a native program or an unreadable file. At most
/// [`SHEBANG_MAX`] bytes are read: the kernel reads no more either.
pub(crate) fn shebang(program: &Path) -> Option<String> {
    use std::io::Read;
    let mut head = Vec::with_capacity(SHEBANG_MAX);
    std::fs::File::open(program)
        .ok()?
        .take(SHEBANG_MAX as u64)
        .read_to_end(&mut head)
        .ok()?;
    let line = head.strip_prefix(b"#!")?;
    let line = line.split(|byte| *byte == b'\n').next().unwrap_or_default();
    let text: String = String::from_utf8_lossy(line)
        .chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect();
    let text = text.trim();
    Some(if text.is_empty() {
        "#!".to_owned()
    } else {
        text.to_owned()
    })
}

/// Bytes of a program's first line read for [`shebang`].
pub const SHEBANG_MAX: usize = 256;
