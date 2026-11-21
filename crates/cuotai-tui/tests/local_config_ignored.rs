//! Security review 2026-11-21 (H-1): a `cuotai.toml` in the working
//! directory is never read by the default rule. A planted file that would
//! send a variable's value to a local listener sends nothing; `--once` and
//! `--check-config` say on standard error that the file is not read, the
//! machine modes stay silent, and `--config ./cuotai.toml` still reads it.
//!
//! Sandboxed: an empty environment with a temporary `HOME`, the debug
//! credential store in that folder, status pages and update checks off, the
//! only account of the OS-path file stops before any query.

#![cfg(unix)]

use std::io::Read;
use std::net::TcpListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const NOTICE: &str = "is not read; pass --config ./cuotai.toml";
const FAKE: &str = "fake-h1-value-0000";

struct Sandbox {
    _root: tempfile::TempDir,
    dir: PathBuf,
    listener: TcpListener,
}

impl Sandbox {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().to_path_buf();
        for sub in ["home", "data", "bin", "tmp", "work"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        // The planted file: a LiteLLM account that would send FAKE to the
        // listener above.
        std::fs::write(
            dir.join("work").join("cuotai.toml"),
            format!(
                "[[accounts]]\nname = \"Planted\"\nprovider = \"litellm\"\ncredential_env = \"CUOTAI_H1_FAKE\"\n\
                 [accounts.params]\nbase_url = \"http://127.0.0.1:{port}\"\n"
            ),
        )
        .unwrap();
        // The file the default rule reads (macOS and XDG locations).
        let home = dir.join("home");
        for config_dir in [
            home.join("Library").join("Application Support"),
            home.join("config"),
        ] {
            let cuotai = config_dir.join("cuotai");
            std::fs::create_dir_all(&cuotai).unwrap();
            std::fs::write(
                cuotai.join("config.toml"),
                "[[accounts]]\nname = \"Default\"\nprovider = \"groq\"\ncredential_env = \"CUOTAI_H1_UNSET\"\n",
            )
            .unwrap();
        }
        Self {
            _root: root,
            dir,
            listener,
        }
    }

    fn run(&self, args: &[&str]) -> Output {
        let home = self.dir.join("home");
        let data = self.dir.join("data");
        let path = format!("{}:/usr/bin:/bin", self.dir.join("bin").display());
        Command::new(env!("CARGO_BIN_EXE_cuotai"))
            .env_clear()
            .current_dir(self.dir.join("work"))
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
            .env("CUOTAI_UPDATE_CHECK", "0")
            .env("CUOTAI_LANG", "en-US")
            .env("CUOTAI_H1_FAKE", FAKE)
            .env("BROWSER", "true")
            .env("PATH", path)
            .stdin(Stdio::null())
            .output()
            .expect("run cuotai")
    }

    /// Whatever reached the listener (empty when nothing connected).
    fn received(&self) -> String {
        let mut text = String::new();
        while let Ok((mut stream, _)) = self.listener.accept() {
            stream.set_nonblocking(false).unwrap();
            stream
                .set_read_timeout(Some(Duration::from_millis(500)))
                .unwrap();
            let mut buf = [0u8; 4096];
            if let Ok(n) = stream.read(&mut buf) {
                text.push_str(&String::from_utf8_lossy(&buf[..n]));
            }
            text.push_str("<connection>");
        }
        text
    }
}

#[test]
fn a_cuotai_toml_in_the_working_directory_is_never_read_by_default() {
    let sandbox = Sandbox::new();

    let once = sandbox.run(&["--once"]);
    let stdout = String::from_utf8_lossy(&once.stdout);
    let stderr = String::from_utf8_lossy(&once.stderr);
    assert!(stdout.contains("Default"), "{stdout}\n{stderr}");
    assert!(!stdout.contains("Planted"), "{stdout}");
    assert!(stderr.contains(NOTICE), "{stderr}");
    assert!(!stdout.contains(NOTICE), "{stdout}");

    for machine in [&["--json"][..], &["--json-v1"], &["--status-line"]] {
        let out = sandbox.run(machine);
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!stdout.contains("Planted"), "{machine:?}: {stdout}");
        assert!(!stdout.contains(NOTICE), "{machine:?}: {stdout}");
        assert!(!stderr.contains(NOTICE), "{machine:?}: {stderr}");
    }

    let check = sandbox.run(&["--check-config"]);
    let stdout = String::from_utf8_lossy(&check.stdout);
    assert!(stdout.contains("config.toml"), "{stdout}");
    assert!(String::from_utf8_lossy(&check.stderr).contains(NOTICE));
    let check_json = sandbox.run(&["--check-config", "--json"]);
    assert!(!String::from_utf8_lossy(&check_json.stderr).contains(NOTICE));

    // Nothing of the planted file ran: no connection, no value sent.
    let received = sandbox.received();
    assert!(received.is_empty(), "{received}");

    // Named explicitly, the file is read (and the notice is gone).
    let named = sandbox.run(&["--config", "./cuotai.toml", "--check-config"]);
    let stdout = String::from_utf8_lossy(&named.stdout);
    assert!(stdout.contains("./cuotai.toml"), "{stdout}");
    assert!(!String::from_utf8_lossy(&named.stderr).contains(NOTICE));
}
