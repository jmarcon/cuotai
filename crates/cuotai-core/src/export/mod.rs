//! Versioned export contracts for machines (PRD 0012): status bars, scripts,
//! the local server and widgets read these instead of the `Snapshot`, whose
//! `--json` shape stays as it is. Each version is a module; a field is
//! removed or changes meaning only in a new version.

pub mod v1;
