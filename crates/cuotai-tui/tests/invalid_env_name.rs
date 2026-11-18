//! PRD 0047 S-06: a key pasted where a variable name belongs stops only its
//! own account and is never repeated by `--once` or `--json`; the other
//! account still prints its own line.
//!
//! Sandboxed: an empty environment with a temporary `HOME` and data folder,
//! the debug credential store in that folder, no provider query (every
//! account stops before one), status pages off.

#![cfg(unix)]

use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Sandbox {
    _root: tempfile::TempDir,
    dir: PathBuf,
}

impl Sandbox {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_path_buf();
        for sub in ["home", "data", "bin", "tmp"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let gsk = format!("gsk_FAKE0047{}", "a1".repeat(24));
        std::fs::write(
            dir.join("cuotai.toml"),
            format!(
                "[[accounts]]\nname = \"Pasted\"\nprovider = \"groq\"\ncredential_env = \"sk-FAKE-0047-pasted-value\"\n\n\
                 [[accounts]]\nname = \"Posix\"\nprovider = \"groq\"\ncredential_env = \"{gsk}\"\n\n\
                 [[accounts]]\nname = \"Admin\"\nprovider = \"mistral\"\n[accounts.params]\nadmin_key_env = \"sk-FAKE-0047-admin-value\"\n\n\
                 [[accounts]]\nname = \"Fine\"\nprovider = \"groq\"\ncredential_env = \"FAKE_GROQ_0047_UNSET\"\n"
            ),
        )
        .unwrap();
        // A browser opener that only logs, first on `PATH`.
        for opener in ["open", "xdg-open"] {
            use std::os::unix::fs::PermissionsExt;
            let path = dir.join("bin").join(opener);
            std::fs::write(
                &path,
                "#!/bin/sh\necho \"$@\" >> \"$(dirname \"$0\")/open.log\"\n",
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self { _root: root, dir }
    }

    fn run(&self, args: &[&str]) -> Output {
        let home = self.dir.join("home");
        let data = self.dir.join("data");
        let path = format!("{}:/usr/bin:/bin", self.dir.join("bin").display());
        Command::new(env!("CARGO_BIN_EXE_cuotai"))
            .env_clear()
            .args(["--config"])
            .arg(self.dir.join("cuotai.toml"))
            .args(["--no-status", "--dev-credentials-sqlite"])
            .arg(self.dir.join("dev.sqlite3"))
            .args(args)
            .env("HOME", &home)
            .env("TMPDIR", self.dir.join("tmp"))
            .env("XDG_DATA_HOME", home.join("data"))
            .env("XDG_CONFIG_HOME", home.join("config"))
            .env("CUOTAI_DATA_DIR", &data)
            .env("CUOTAI_FEED_DIR", data.join("feed"))
            .env("CUOTAI_USER_DIR", home.join("user"))
            .env("CUOTAI_METERED_DIR", home.join("metered"))
            .env("CUOTAI_LANG", "en-US")
            .env("BROWSER", "true")
            .env("PATH", path)
            .stdin(Stdio::null())
            .output()
            .expect("run cuotai")
    }
}

fn leaks(text: &str) -> bool {
    text.contains("FAKE-0047") || text.contains("gsk_FAKE")
}

#[test]
fn once_and_json_never_repeat_an_invalid_variable_name() {
    let sandbox = Sandbox::new();
    for args in [&["--once"][..], &["--json"][..]] {
        let output = sandbox.run(args);
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {stderr}");
        assert!(!leaks(&stdout), "{args:?}: a pasted name on stdout");
        assert!(!leaks(&stderr), "{args:?}: a pasted name on stderr");
        assert!(stdout.contains("Fine"), "{args:?}: the fine account prints");
        assert!(
            stdout.contains("FAKE_GROQ_0047_UNSET"),
            "{args:?}: the fine account keeps its own note"
        );
    }
    let json = sandbox.run(&["--json"]);
    let snapshot: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    let reports = snapshot["reports"].as_array().unwrap();
    assert_eq!(reports.len(), 4);
    for report in &reports[..3] {
        assert_eq!(report["status"], "needs_configuration");
        assert_eq!(report["notes_l10n"][0]["id"], "query.env_name_invalid");
    }
    assert_eq!(
        reports[3]["notes_l10n"][0]["id"],
        "query.missing_credential"
    );
    assert!(!sandbox.dir.join("bin/open.log").exists());
}
