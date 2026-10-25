//! PRD 0031 RF1.2: the status-bar recipes in `examples/integrations` run
//! against fixture `/v1/limits` bodies (a fake `curl` answers with the
//! fixture, a fake `sketchybar` prints its arguments) and print exactly the
//! output the pages under `docs/integrations` show. Needs `sh`, `jq`, `cut`.
//! The fixtures are checked against the JSON Schema in `cuotai-core`.

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

/// A directory with fake `curl` (prints `$FIXTURE`, or fails when unset) and
/// `sketchybar` (prints its arguments) at the front of `PATH`.
struct Bin(PathBuf);

impl Bin {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("cuotai-integrations-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        for (program, body) in [
            (
                "curl",
                "#!/bin/sh\n[ -n \"$FIXTURE\" ] || exit 22\ncat \"$FIXTURE\"\n",
            ),
            ("sketchybar", "#!/bin/sh\necho \"$@\"\n"),
        ] {
            let path = dir.join(program);
            fs::write(&path, body).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        Self(dir)
    }

    fn run(&self, script: &str, fixture: Option<&str>) -> String {
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
            .env_remove("CUOTAI_TOKEN_FILE")
            .env_remove("NAME");
        if let Some(fixture) = fixture {
            command.env("FIXTURE", examples().join("fixtures").join(fixture));
        }
        let output = command.output().expect("run the recipe");
        assert!(output.status.success(), "{script}: {output:?}");
        String::from_utf8(output.stdout)
            .unwrap()
            .trim_end()
            .to_owned()
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
