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
//! PRD 0013 re-validation (S-01): a program or interpreter that another
//! user could change, replace or rename (itself, or any folder above it:
//! [`crate::private_fs::exposed_chain`]) is never pinned, so it can be
//! neither approved nor run. A script is never run by its path: the runner
//! copies it into a private folder, checks the copy's hash and starts the
//! pinned interpreter on the copy, so a swap after the check cannot run.
//!
//! The `#!` lines accepted are `#!/abs/interpreter [args]`,
//! `#!/usr/bin/env NAME [args]` and `#!/usr/bin/env -S NAME [args]`
//! (`--split-string`, and `--` before `NAME`, too). Any other `env` option,
//! a `NAME` holding `=` or `$`, `$` or `\` in a `-S` line, and an
//! interpreter that is itself a `#!` script are refused
//! ([`Unresolved::Shebang`]): what they would run cannot be pinned.
//!
//! Limits: arguments are not pinned (a script passed to `sh` as an argument
//! is data, not the program); a native program (and a script's
//! interpreter) is still started by its path, so a file swapped between the
//! last hash and the `exec` by someone who may write it (the user
//! themselves, since others are refused) is not detected. Relative and
//! empty `PATH` entries are skipped, since a hook runs in a private
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
    /// Its `#!` line takes a form whose program cannot be pinned.
    Shebang,
    /// It, its interpreter or a folder above either can be changed by
    /// another user.
    Exposed,
    /// PRD 0013 S-02: its path (or its interpreter's) holds a control or
    /// text-direction character that could disguise it in the listing.
    Disguised,
}

/// Pin `program` (a rule's `command[0]`: absolute, or a bare name looked up
/// on `search_path`).
pub fn resolve(program: &str, search_path: Option<&OsStr>) -> Result<Pin, Unresolved> {
    let found = locate(program, search_path).ok_or(Unresolved::Program)?;
    let (program, sha256, head) = hashed(&found).ok_or(Unresolved::Program)?;
    disguised(&program)?;
    if crate::private_fs::exposed_chain(&program) {
        return Err(Unresolved::Exposed);
    }
    let interpreter = match shebang(&head) {
        None => None,
        Some(line) => Some(interpreter(&line?, search_path)?),
    };
    Ok(Pin {
        program,
        sha256,
        interpreter,
    })
}

/// PRD 0013 S-02: a path holding a control or text-direction character is
/// refused, never shown stripped (which would misstate what runs).
fn disguised(path: &Path) -> Result<(), Unresolved> {
    if path
        .to_string_lossy()
        .chars()
        .any(crate::text::is_disguising)
    {
        return Err(Unresolved::Disguised);
    }
    Ok(())
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
pub(crate) fn hashed(path: &Path) -> Option<(PathBuf, String, Vec<u8>)> {
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

/// What a `#!` line runs: the interpreter, how to find it, the `argv[0]`
/// it is started with and the arguments before the script.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(unix), allow(dead_code))]
pub(crate) struct Shebang {
    /// An absolute path, or for `env` the name looked up on `PATH`.
    target: String,
    pub(crate) arg0: OsString,
    pub(crate) args: Vec<OsString>,
}

/// The `#!` line of a file starting with `head`: `None` without one,
/// `Some(Err)` for a form that cannot be pinned. Windows never runs a `#!`
/// line (it starts only the program itself), so there it is always `None`.
#[cfg(not(unix))]
pub(crate) fn shebang(_head: &[u8]) -> Option<Result<Shebang, Unresolved>> {
    None
}

/// The `#!` line of a file starting with `head`: `None` without one,
/// `Some(Err)` for a form that cannot be pinned.
#[cfg(unix)]
pub(crate) fn shebang(head: &[u8]) -> Option<Result<Shebang, Unresolved>> {
    let rest = head.strip_prefix(b"#!")?;
    let line = rest.split(|byte| *byte == b'\n').next().unwrap_or_default();
    Some(parse_shebang(&String::from_utf8_lossy(line)).ok_or(Unresolved::Shebang))
}

#[cfg_attr(not(unix), allow(dead_code))]
fn parse_shebang(line: &str) -> Option<Shebang> {
    let line = line.trim();
    let first = line.split_whitespace().next()?;
    if !Path::new(first).is_absolute() {
        return None;
    }
    if Path::new(first).file_name() != Some(OsStr::new("env")) {
        // Linux passes the rest of the line as one argument; the BSDs and
        // macOS split it on blanks.
        let tail = line[first.len()..].trim();
        let args = if tail.is_empty() {
            Vec::new()
        } else if cfg!(any(target_os = "linux", target_os = "android")) {
            vec![OsString::from(tail)]
        } else {
            tail.split_whitespace().map(OsString::from).collect()
        };
        return Some(Shebang {
            target: first.to_owned(),
            arg0: OsString::from(first),
            args,
        });
    }
    let mut words = line.split_whitespace().skip(1).peekable();
    let split = matches!(words.peek(), Some(&("-S" | "--split-string")));
    if split {
        words.next();
    }
    if words.peek() == Some(&"--") {
        words.next();
    }
    let name = words.next()?;
    if name.starts_with('-') || name.contains(['=', '$']) {
        return None;
    }
    let args: Vec<&str> = words.collect();
    if split && args.iter().any(|word| word.contains(['$', '\\'])) {
        return None;
    }
    Some(Shebang {
        target: name.to_owned(),
        arg0: OsString::from(name),
        args: args.into_iter().map(OsString::from).collect(),
    })
}

/// The interpreter `line` runs, pinned: its absolute path, or the name
/// found on `search_path`; refused when it is itself a `#!` script or
/// another user can change it.
fn interpreter(line: &Shebang, search_path: Option<&OsStr>) -> Result<Interpreter, Unresolved> {
    let target = locate(&line.target, search_path).ok_or(Unresolved::Interpreter)?;
    let (program, sha256, head) = hashed(&target).ok_or(Unresolved::Interpreter)?;
    if head.starts_with(b"#!") {
        return Err(Unresolved::Shebang);
    }
    disguised(&program)?;
    if crate::private_fs::exposed_chain(&program) {
        return Err(Unresolved::Exposed);
    }
    Ok(Interpreter { program, sha256 })
}

/// Unix paths: Windows never reads a `#!` line, and `/bin/sh` is not
/// absolute there.
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn only_the_env_forms_that_can_be_pinned_are_accepted() {
        let target = |line: &str| parse_shebang(line).map(|s| (s.target, s.args.len()));
        for (line, expected) in [
            ("/bin/sh", Some(("/bin/sh", 0))),
            ("/usr/bin/env python3", Some(("python3", 0))),
            ("/usr/bin/env -S python3 -u", Some(("python3", 1))),
            (
                "/usr/bin/env --split-string python3 -u -B",
                Some(("python3", 2)),
            ),
            ("/usr/bin/env -- python3", Some(("python3", 0))),
            ("/usr/bin/env -S -- python3", Some(("python3", 0))),
            ("/usr/bin/env -P /tmp/evil python3", None),
            ("/usr/bin/env -i python3", None),
            ("/usr/bin/env -u HOME python3", None),
            ("/usr/bin/env -C /tmp python3", None),
            ("/usr/bin/env PATH=/tmp python3", None),
            ("/usr/bin/env -S $SHELL", None),
            ("/usr/bin/env -S python3 ${X}", None),
            ("/usr/bin/env -S python3 \\_x", None),
            ("/usr/bin/env", None),
            ("python3", None),
        ] {
            let expected = expected.map(|(name, args)| (name.to_owned(), args));
            assert_eq!(target(line), expected, "{line}");
        }
    }
}
