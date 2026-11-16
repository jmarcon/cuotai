//! PRD 0031 RF1.2: the status-bar recipes in `examples/integrations` run
//! against fixture `/v1/limits` bodies (a fake `curl` answers with the
//! fixture, a fake `sketchybar` prints its arguments) and print exactly the
//! output the pages under `docs/integrations` show. Needs `sh`, `jq`, `cut`.
//! The fixtures are checked against the JSON Schema in `cuotai-core`.
//!
//! The fake `curl` also enforces how the token travels (PRD 0031 S-01,
//! S-02): never in its arguments, only as a config on stdin (`-K -`), and
//! always with `-q`, `--noproxy '*'`, `--proto =http` and `--max-redirs 0`.

#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/integrations")
}

fn has_jq() -> bool {
    Command::new("jq")
        .arg("--version")
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The fake `curl`: it records each call in `$CALLS`, rejects the token or
/// `Bearer` in its arguments, a missing safety flag, and (with
/// `$EXPECT_TOKEN`) a stdin that is not exactly the header config; each
/// rejection prints `fake curl:` on stderr. Otherwise it prints `$FIXTURE`,
/// or fails like an unreachable server when that is unset.
const FAKE_CURL: &str = r#"#!/bin/sh
echo call >> "$CALLS"
for arg in "$@"; do
  case $arg in *Bearer*) echo "fake curl: Bearer in the arguments" >&2; exit 90 ;; esac
  if [ -n "$EXPECT_TOKEN" ]; then
    case $arg in *"$EXPECT_TOKEN"*) echo "fake curl: token in the arguments" >&2; exit 91 ;; esac
  fi
done
[ "$1" = -q ] || { echo "fake curl: -q is not first" >&2; exit 92; }
args=" $* "
for flag in " -K - " " --noproxy * " " --proto =http " " --max-redirs 0 " " --max-time " " --max-filesize "; do
  case $args in *"$flag"*) ;; *) echo "fake curl: missing$flag" >&2; exit 92 ;; esac
done
config=$(cat)
if [ -n "$EXPECT_TOKEN" ] && [ "$config" != "header = "Authorization: Bearer $EXPECT_TOKEN"" ]; then
  echo "fake curl: the token is not on stdin" >&2; exit 93
fi
[ -n "$FIXTURE" ] || exit 22
cat "$FIXTURE"
"#;

/// A directory with fake `curl` (see [`FAKE_CURL`]) and `sketchybar`
/// (prints its arguments) at the front of `PATH`.
struct Bin(PathBuf);

impl Bin {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("cuotai-integrations-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (program, body) in [
            ("curl", FAKE_CURL),
            ("sketchybar", "#!/bin/sh\necho \"$@\"\n"),
        ] {
            let path = dir.join(program);
            fs::write(&path, body).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self(dir)
    }

    fn run(&self, script: &str, fixture: Option<&str>) -> String {
        self.run_with(script, fixture, &[])
    }

    /// Runs `script` with `env` added; fails on any `fake curl:` complaint.
    fn run_with(&self, script: &str, fixture: Option<&str>, env: &[(&str, &str)]) -> String {
        let path = format!(
            "{}:{}",
            self.0.display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new("sh");
        command
            .arg(examples().join(script))
            .env("PATH", path)
            .env("SKETCHYBAR", self.0.join("sketchybar"))
            .env("CALLS", self.calls())
            .env_remove("CUOTAI_TOKEN_FILE")
            .env_remove("CUOTAI_TOKEN")
            .env_remove("CUOTAI_URL")
            .env_remove("EXPECT_TOKEN")
            .env_remove("NAME");
        if let Some(fixture) = fixture {
            command.env("FIXTURE", examples().join("fixtures").join(fixture));
        }
        command.envs(env.iter().copied());
        let output = command.output().expect("run the recipe");
        assert!(output.status.success(), "{script}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!stderr.contains("fake curl:"), "{script}: {stderr}");
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end()
            .to_owned()
    }
}

impl Bin {
    fn calls(&self) -> PathBuf {
        self.0.join("calls")
    }

    fn call_count(&self) -> usize {
        fs::read_to_string(self.calls()).map_or(0, |calls| calls.lines().count())
    }
}

impl Drop for Bin {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn every_recipe_prints_what_its_page_shows() {
    if !has_jq() {
        // The CI runners have jq; do not let a missing one pass there.
        assert!(
            std::env::var_os("CI").is_none(),
            "jq is required to run the integration recipes"
        );
        eprintln!("jq not found; skipping the integration recipes");
        return;
    }
    let bin = Bin::new("recipes");
    let cases = [
        (
            "waybar.sh",
            "limits-busy.json",
            r#"{"text":"Cuotai 78% Personal","class":"warning","tooltip":"Resets at 2026-10-24T09:00:00Z"}"#,
        ),
        (
            "waybar.sh",
            "limits-blocked.json",
            r#"{"text":"Cuotai 100% Personal","class":"critical","tooltip":"Resets at 2026-10-20T14:00:00Z"}"#,
        ),
        (
            "sketchybar.sh",
            "limits-busy.json",
            "--set cuotai label=78% Personal label.color=0xffffb86c",
        ),
        (
            "tmux.sh",
            "limits-busy.json",
            "#[fg=yellow]78% Personal#[default]",
        ),
        (
            "tmux.sh",
            "limits-blocked.json",
            "#[fg=red]100% Personal#[default]",
        ),
        (
            "i3status.sh",
            "limits-busy.json",
            r##"{"name":"cuotai","full_text":"Cuotai 78% Personal","color":"#ffb86c"}"##,
        ),
    ];
    for (script, fixture, expected) in cases {
        assert_eq!(
            bin.run(script, Some(fixture)),
            expected,
            "{script} {fixture}"
        );
    }
}

#[test]
fn an_unreachable_server_shows_offline_and_exits_zero() {
    if !has_jq() {
        return;
    }
    let bin = Bin::new("offline");
    assert_eq!(
        bin.run("waybar.sh", None),
        r#"{"text":"Cuotai offline","tooltip":"cuotai --serve is not reachable","class":"offline"}"#
    );
    assert_eq!(
        bin.run("tmux.sh", None),
        "#[fg=colour244]cuotai offline#[default]"
    );
    assert_eq!(
        bin.run("sketchybar.sh", None),
        "--set cuotai label=offline label.color=0xff888888"
    );
}

#[test]
fn separators_in_an_account_name_do_not_shift_the_fields() {
    if !has_jq() {
        return;
    }
    let bin = Bin::new("separators");
    let body = fs::read_to_string(examples().join("fixtures/limits-busy.json"))
        .unwrap()
        .replace(
            "\"name\": \"Personal\"",
            "\"name\": \"Work\\tTeam\\nNight\"",
        );
    let fixture = bin.0.join("limits-tabbed.json");
    fs::write(&fixture, body).unwrap();
    assert_eq!(
        bin.run("waybar.sh", fixture.to_str()),
        r#"{"text":"Cuotai 78% Work Team Night","class":"warning","tooltip":"Resets at 2026-10-24T09:00:00Z"}"#
    );
}

#[test]
fn the_token_reaches_curl_only_on_stdin() {
    if !has_jq() {
        return;
    }
    let bin = Bin::new("token");
    // A fake token: the recipes read it from the file, as `cuotai --serve` keeps it.
    let token = "cuotai_FAKE0test0token0not0real0123456789";
    let token_file = bin.0.join("token");
    fs::write(&token_file, format!("{token}\n")).unwrap();
    let env = [
        ("CUOTAI_TOKEN_FILE", token_file.to_str().unwrap()),
        ("EXPECT_TOKEN", token),
        // A proxy in the environment must change nothing: curl gets --noproxy '*'.
        ("http_proxy", "http://127.0.0.1:9"),
        ("ALL_PROXY", "http://127.0.0.1:9"),
    ];
    for (script, expected) in [
        ("tmux.sh", "#[fg=yellow]78% Personal#[default]"),
        (
            "waybar.sh",
            r#"{"text":"Cuotai 78% Personal","class":"warning","tooltip":"Resets at 2026-10-24T09:00:00Z"}"#,
        ),
        (
            "sketchybar.sh",
            "--set cuotai label=78% Personal label.color=0xffffb86c",
        ),
        (
            "i3status.sh",
            r##"{"name":"cuotai","full_text":"Cuotai 78% Personal","color":"#ffb86c"}"##,
        ),
    ] {
        assert_eq!(
            bin.run_with(script, Some("limits-busy.json"), &env),
            expected,
            "{script}"
        );
    }
    assert_eq!(bin.call_count(), 4);
}

#[test]
fn only_a_loopback_http_address_is_read() {
    if !has_jq() {
        return;
    }
    let bin = Bin::new("loopback");
    let offline = "#[fg=colour244]cuotai offline#[default]";
    for url in [
        "https://127.0.0.1:6738",
        "http://example.com:6738",
        "http://127.0.0.1:6738@example.com",
        "http://127.0.0.1:6738/elsewhere",
        "http://127.0.0.1",
    ] {
        let env = [("CUOTAI_URL", url)];
        assert_eq!(
            bin.run_with("tmux.sh", Some("limits-busy.json"), &env),
            offline,
            "{url}"
        );
    }
    assert_eq!(bin.call_count(), 0, "curl ran for a refused address");
    for url in [
        "http://localhost:6738",
        "http://[::1]:6738/",
        "http://127.0.0.1:7000",
    ] {
        let env = [("CUOTAI_URL", url)];
        assert_eq!(
            bin.run_with("tmux.sh", Some("limits-busy.json"), &env),
            "#[fg=yellow]78% Personal#[default]",
            "{url}"
        );
    }
    // A token with characters outside RFC 6750 could break out of the config.
    let bad = bin.0.join("bad-token");
    fs::write(&bad, "cuotai_x\"\noutput = \"/tmp/x\"\n").unwrap();
    let env = [("CUOTAI_TOKEN_FILE", bad.to_str().unwrap())];
    assert_eq!(
        bin.run_with("tmux.sh", Some("limits-busy.json"), &env),
        offline
    );
    assert_eq!(bin.call_count(), 3);
}
