//! PRD 0051 RF3.6: the policy of every local-state contract, the one source
//! of the table in `docs/local-state.md` (generated, drift-tested).
//!
//! Adding a store means adding its row here; the tests fail when a named
//! contract or a compatibility fixture has no row.

/// One state file and how Cuotai versions, backs up, repairs, downgrades and
/// erases it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateContract {
    pub file: &'static str,
    pub location: &'static str,
    pub owner: &'static str,
    pub version: &'static str,
    pub holds: &'static str,
    pub permissions: &'static str,
    pub backup: &'static str,
    pub damaged: &'static str,
    pub newer: &'static str,
    pub erase: &'static str,
}

/// Every contract, in the order of the table.
pub const CONTRACTS: [StateContract; 8] = [
    StateContract {
        file: "cuotai.toml",
        location: "`--config`, `CUOTAI_CONFIG`, `./cuotai.toml` or `<config>/config.toml`",
        owner: "core `config_file`",
        version: "none (tolerant reader)",
        holds: "accounts, variable **names**, vault references",
        permissions: "`0600` when Cuotai creates it",
        backup: "edit history is yours; `cuotai --backup`",
        damaged: "the file is rejected with its line, column and key, never ignored",
        newer: "an unknown provider or account key sets that account aside; unknown top-level keys are ignored; a save keeps both byte for byte",
        erase: "delete the file",
    },
    StateContract {
        file: "desktop.json",
        location: "`<config>/desktop.json`",
        owner: "desktop, through core `private_fs::update_locked`",
        version: "`schema` (absent = 1)",
        holds: "preferences and window state",
        permissions: "`0600` in a `0700` folder",
        backup: "`desktop.json.bak[.N]`, never overwritten, before a damaged file is replaced",
        damaged: "kept as `.bak`, the app starts with the defaults and says so",
        newer: "unknown members are kept; a later `schema` major is read but never rewritten",
        erase: "delete the file",
    },
    StateContract {
        file: "history.sqlite3",
        location: "`<data>/history/`",
        owner: "core `history`",
        version: "`user_version` (migrations) and `state_meta` major 1",
        holds: "readings: numbers, metric labels, digests",
        permissions: "`0600` in `0700`",
        backup: "`history.sqlite3.bak` (consistent copy) before each migration",
        damaged: "the history is reported unavailable, nothing is deleted",
        newer: "same major: readable and writable; later major or no `state_meta`: refused untouched",
        erase: "Settings › Data & privacy, `cuotai --history-erase`, or delete the folder",
    },
    StateContract {
        file: "cost.sqlite3",
        location: "`<data>/cost/`",
        owner: "core `cost`",
        version: "as the history",
        holds: "per-request token counts, model names, digests",
        permissions: "`0600` in `0700`",
        backup: "`cost.sqlite3.bak` before each migration",
        damaged: "the index is reported unavailable",
        newer: "same major: readable and writable; later major: refused untouched (`state.error.newer_major`)",
        erase: "`cuotai --cost-erase`",
    },
    StateContract {
        file: "models.sqlite3",
        location: "`<data>/models/`",
        owner: "core `models`",
        version: "`user_version` 1 and `state_meta` major 1",
        holds: "model lists per account digest",
        permissions: "`0600` in `0700`",
        backup: "none (refetched)",
        damaged: "reported as a storage error",
        newer: "same major: usable; later major: refused untouched (`state.error.newer_major`)",
        erase: "delete the folder",
    },
    StateContract {
        file: "notified.json",
        location: "`<data>/notified.json`",
        owner: "core `resets`",
        version: "`schema` (absent = 1) and `schema_revision` (absent = 0)",
        holds: "digests and dates of sent notices",
        permissions: "`0600`",
        backup: "none",
        damaged: "set aside as `notified.json.corrupt-<UTC time>`, never overwritten (`cuotai --doctor` reports it), and read as empty",
        newer: "same major: unknown members are kept; later major: read as empty and never rewritten",
        erase: "delete the file",
    },
    StateContract {
        file: "hooks.toml",
        location: "beside `cuotai.toml`",
        owner: "core `hooks`",
        version: "`schema` (absent = 1) and `schema_revision` (absent = 0)",
        holds: "your hook rules (commands)",
        permissions: "as you created it",
        backup: "`cuotai --backup`",
        damaged: "the hooks are refused with the line",
        newer: "same major: unknown keys are ignored and the editor keeps them; later major: refused untouched",
        erase: "delete the file",
    },
    StateContract {
        file: "widget.json",
        location: "the widget folder or `<data>`",
        owner: "core `widget_snapshot`",
        version: "`schema = 1` and `schema_revision` (absent = 0)",
        holds: "the published readings",
        permissions: "`0600`",
        backup: "`widget.json.bak` before the first replacement of another schema or revision",
        damaged: "rewritten on the next publish",
        newer: "replaced after the backup",
        erase: "turn widgets off and delete it",
    },
];

/// The seven contracts PRD 0051 RF3.1 names (the TOML is the eighth row).
pub const VERSIONED: [&str; 7] = [
    "desktop.json",
    "notified.json",
    "history.sqlite3",
    "models.sqlite3",
    "cost.sqlite3",
    "hooks.toml",
    "widget.json",
];

/// The Markdown table of [`CONTRACTS`].
pub fn matrix() -> String {
    let mut out = String::from(
        "| File | Location | Owner | Version | Holds | Permissions | Backup | Damaged file | Written by a newer Cuotai | Erase |\n|---|---|---|---|---|---|---|---|---|---|\n",
    );
    for c in CONTRACTS {
        out.push_str(&format!(
            "| `{}` | {} | {} | {} | {} | {} | {} | {} | {} | {} |\n",
            c.file,
            c.location,
            c.owner,
            c.version,
            c.holds,
            c.permissions,
            c.backup,
            c.damaged,
            c.newer,
            c.erase
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "docs/local-state.md";
    const START: &str =
        "<!-- state-matrix:start (generated from state_contracts::CONTRACTS; do not edit by hand) -->";
    const END: &str = "<!-- state-matrix:end -->";

    #[test]
    fn every_named_contract_has_a_policy() {
        for file in VERSIONED {
            assert!(
                CONTRACTS.iter().any(|c| c.file == file),
                "{file} has no state policy"
            );
        }
        let mut files: Vec<_> = CONTRACTS.iter().map(|c| c.file).collect();
        files.dedup();
        assert_eq!(files.len(), CONTRACTS.len(), "one row per file");
    }

    #[test]
    fn local_state_doc_matrix_matches_the_contracts() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(DOC);
        let doc = std::fs::read_to_string(&path)
            .expect("local-state doc")
            .replace("\r\n", "\n");
        let (head, rest) = doc.split_once(START).expect("start marker");
        let (current, tail) = rest.split_once(END).expect("end marker");
        let expected = format!("\n{}", matrix());
        if std::env::var_os("CUOTAI_REGENERATE_DOCS").is_some() {
            std::fs::write(&path, format!("{head}{START}{expected}{END}{tail}"))
                .expect("write local-state doc");
            return;
        }
        assert_eq!(
            current, expected,
            "{DOC} is out of date with state_contracts::CONTRACTS. Regenerate it with \
             `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core local_state_doc_matrix` and commit the result."
        );
    }
}
