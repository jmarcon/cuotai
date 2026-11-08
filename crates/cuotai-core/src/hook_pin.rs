//! PRD 0050 RF2.4: the program a hook rule runs, pinned at consent.
//!
//! A bare `command[0]` is looked up on the exact `PATH` the hooks run with
//! (captured when the rules are read, [`super::Hooks::search_path`]), the
//! file found is canonicalized (symbolic links resolved) and its bytes
//! hashed with SHA-256. A script starting with `#!` also pins its
//! interpreter: the absolute interpreter path, or for `#!/usr/bin/env name`
//! the `name` found on the same `PATH`. The pins enter the consent digest
//! ([`super::Hooks::digest`]) and the listing, and every spawn resolves and
//! hashes again first: any difference (another file earlier on `PATH`, new
//! bytes, a new interpreter) runs nothing and suspends the rule until the
//! hooks are confirmed again.
//!
//! Limits: arguments are not pinned (a script passed to `sh` as an argument
//! is data, not the program); a file swapped between the last hash and the
//! `exec` itself is not detected (the window is the spawn call). Relative
//! and empty `PATH` entries are skipped, since a hook runs in a private
//! working directory. On Windows a bare name also tries each `PATHEXT`
//! extension, as the shell does.

use std::ffi::{OsStr, OsString};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The pinned program of one rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pin {
    /// The canonical absolute path that runs.
    pub program: PathBuf,
    /// SHA-256 (lowercase hex) of its bytes.
    pub sha256: String,
    /// The interpreter its `#!` line names, pinned the same way.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interpreter: Option<Interpreter>,
}

/// A script's pinned interpreter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Interpreter {
    pub program: PathBuf,
    pub sha256: String,
}

/// Why a program could not be pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unresolved {
    /// Not found on `PATH`, or not a regular executable file.
    Program,
    /// Its `#!` interpreter could not be found or read.
    Interpreter,
}

/// Pin `program` (a rule's `command[0]`: absolute, or a bare name looked up
/// on `search_path`).
pub fn resolve(program: &str, search_path: Option<&OsStr>) -> Result<Pin, Unresolved> {
    let found = locate(program, search_path).ok_or(Unresolved::Program)?;
    let (program, sha256, head) = hashed(&found).ok_or(Unresolved::Program)?;
    let interpreter = match shebang(&head) {
        None => None,
        Some(line) => Some(interpreter(&line, search_path).ok_or(Unresolved::Interpreter)?),
    };
    Ok(Pin {
        program,
        sha256,
        interpreter,
    })
}

/// The file `program` names: itself when absolute, else the first
/// executable regular file of that name on `search_path`.
fn locate(program: &str, search_path: Option<&OsStr>) -> Option<PathBuf> {
    let path = Path::new(program);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    if program.is_empty() || program.contains(['/', '\\']) {
        return None;
    }
    let search_path = search_path?;
    std::env::split_paths(search_path)
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| candidates(&dir, program))
        .find(|candidate| executable(candidate))
}

#[cfg(not(windows))]
fn candidates(dir: &Path, program: &str) -> Vec<PathBuf> {
    vec![dir.join(program)]
}

/// The name as written, then with each `PATHEXT` extension when it has
/// none.
#[cfg(windows)]
fn candidates(dir: &Path, program: &str) -> Vec<PathBuf> {
    let mut all = vec![dir.join(program)];
    if Path::new(program).extension().is_none() {
        let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
        all.extend(
            exts.split(';')
                .filter(|ext| !ext.is_empty())
                .map(|ext| dir.join(format!("{program}{ext}"))),
        );
    }
    all
}

#[cfg(unix)]
fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_file())
}

/// The canonical path of `path`, the SHA-256 of its bytes and their first
/// [`HEAD`] bytes; `None` unless it is a readable regular file.
fn hashed(path: &Path) -> Option<(PathBuf, String, Vec<u8>)> {
    use sha2::{Digest, Sha256};
    let canonical = std::fs::canonicalize(path).ok()?;
    let mut file = std::fs::File::open(&canonical).ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut hasher = Sha256::new();
    let mut head = Vec::new();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        match file.read(&mut buffer) {
            Ok(0) => break,
            Ok(read) => {
                if head.len() < HEAD {
                    let take = read.min(HEAD - head.len());
                    head.extend_from_slice(&buffer[..take]);
                }
                hasher.update(&buffer[..read]);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return None,
        }
    }
    Some((canonical, format!("{:x}", hasher.finalize()), head))
}

/// Bytes read for the `#!` line.
const HEAD: usize = 256;

/// The words of a `#!` line, or `None` for a file without one. Windows
/// never runs a `#!` line (it starts only the program itself), so there it
/// pins no interpreter.
#[cfg(not(unix))]
fn shebang(_head: &[u8]) -> Option<Vec<OsString>> {
    None
}

/// The words of a `#!` line, or `None` for a file without one.
#[cfg(unix)]
fn shebang(head: &[u8]) -> Option<Vec<OsString>> {
    let rest = head.strip_prefix(b"#!")?;
    let line = rest.split(|byte| *byte == b'\n').next().unwrap_or_default();
    let line = String::from_utf8_lossy(line);
    Some(line.split_whitespace().map(OsString::from).collect())
}

/// The interpreter a `#!` line runs: its absolute first word, or for
/// `env` the first non-option word found on `search_path`.
fn interpreter(words: &[OsString], search_path: Option<&OsStr>) -> Option<Interpreter> {
    let first = Path::new(words.first()?);
    if !first.is_absolute() {
        return None;
    }
    let target = if first.file_name() == Some(OsStr::new("env")) {
        let name = words[1..]
            .iter()
            .map(|word| word.to_string_lossy())
            .find(|word| !word.starts_with('-'))?;
        locate(&name, search_path)?
    } else {
        first.to_path_buf()
    };
    let (program, sha256, _) = hashed(&target)?;
    Some(Interpreter { program, sha256 })
}
