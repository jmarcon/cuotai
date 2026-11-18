//! PRD 0046 re-validation M-01: the restore plan carries no terminal
//! escape from a crafted backup.

use super::*;
use cuotai_core::bundle::{Entry, Manifest};
use std::collections::BTreeMap;

#[test]
fn plan_text_prints_no_escape_from_the_backup() {
    let dir = tempfile::tempdir().unwrap();
    let config =
        b"[[accounts]]\nname = \"a\"\nprovider = \"groq\"\ncredential_env = \"A\\u001b[1AB\"\n"
            .to_vec();
    let bundle = Bundle {
        manifest: Manifest {
            schema: 1,
            created_at: "2026-01-01T00:00:00Z".into(),
            cuotai_version: "9.9.9".into(),
            os: "macos".into(),
            files: vec![Entry {
                path: "config/cuotai.toml".into(),
                sha256: String::new(),
                kind: "config".into(),
            }],
            excluded: Vec::new(),
        },
        files: BTreeMap::from([("config/cuotai.toml".to_owned(), config)]),
    };
    let locations = Locations {
        config: dir.path().join("cuotai.toml"),
        desktop_settings: None,
        user_dir: None,
        data_root: None,
    };
    let none = |_: &str| None;
    let mut plan = bundle::plan(&bundle, &locations, None, &none).unwrap();
    // Defence in depth: whatever reaches the plan is filtered field by field.
    plan.created_at = "2026\u{1b}[2J-01-01".into();
    plan.cuotai_version = "1.0\u{1b}]0;x\u{7}".into();
    plan.items[0].target = "~/a\u{202e}b.toml".into();
    plan.unplaced.push("user/themes/x\u{1b}[1A.css".into());
    let text = plan_text(&plan, Path::new("b.zip"), "en-US");
    assert!(!text.contains('\u{1b}') && !text.contains('\u{7}') && !text.contains('\u{202e}'));
    assert!(text.contains("Fix the variable name of account 1 (groq); it is not valid."));
    assert!(text.lines().count() > 3, "line breaks are kept");
}
