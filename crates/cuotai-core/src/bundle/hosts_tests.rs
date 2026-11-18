//! PRD 0046 M-03 (owner decision of 2026-11-18): a restore names each
//! account that would send a key to another address or run another
//! program, and writes the accounts file only once they are confirmed.

use super::*;

/// The accounts file this computer already has: one LiteLLM account on a
/// loopback proxy, which the backup moves to a remote one.
const LOCAL: &str = "[[accounts]]\nid = \"moved\"\nprovider = \"litellm\"\ncredential_env = \"LITELLM_KEY\"\n\n[accounts.params]\nbase_url = \"http://127.0.0.1:4000\"\n";

/// The backup's accounts file.
const INCOMING: &str = "[[accounts]]\nid = \"moved\"\nprovider = \"litellm\"\ncredential_env = \"LITELLM_KEY\"\n\n[accounts.params]\nbase_url = \"https://other.example.com\"\n\n\
[[accounts]]\nid = \"proxy\"\nprovider = \"litellm\"\ncredential_env = \"PROXY_KEY\"\n\n[accounts.params]\nbase_url = \"https://user@proxy.example.com:8443/v1?x=1\"\n\n\
[[accounts]]\nid = \"local\"\nprovider = \"litellm\"\ncredential_env = \"LOCAL_KEY\"\n\n[accounts.params]\nbase_url = \"http://127.0.0.1:4000\"\n\n\
[[accounts]]\nid = \"router\"\nprovider = \"nine_router\"\n\n[accounts.params]\nbase_url = \"http://10.0.0.5:20128\"\n\n\
[[accounts]]\nid = \"bedrock\"\nprovider = \"aws_bedrock\"\n\n[accounts.params]\nprofile = \"prod\"\ncli_path = \"/opt/tools/aws\"\n";

fn locations(root: &Path) -> Locations {
    Locations {
        config: root.join("config/cuotai.toml"),
        desktop_settings: Some(root.join("config/desktop.json")),
        user_dir: Some(root.join("user")),
        data_root: Some(root.join("data")),
    }
}

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn created() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

/// A backup of a tree holding `config` (and a hook and a theme).
fn backup_of(config: &str) -> Bundle {
    let source = tempfile::tempdir().unwrap();
    write(source.path(), "config/cuotai.toml", config);
    write(
        source.path(),
        "config/hooks.toml",
        "[[hooks]]\nname = \"n\"\n",
    );
    write(source.path(), "user/themes/night.css", ":root{}");
    let bundle = collect(&locations(source.path()), created(), "9.9.9").unwrap();
    Bundle::from_zip(&bundle.to_zip()).unwrap()
}

/// Every file under `root`, with its bytes.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                files.insert(path.clone(), std::fs::read(&path).unwrap());
            }
        }
    }
    files
}

#[test]
fn a_restore_names_where_restored_keys_go() {
    let bundle = backup_of(INCOMING);
    let here = tempfile::tempdir().unwrap();
    write(here.path(), "config/cuotai.toml", LOCAL);
    let none = |_: &str| None;
    let plan = plan(&bundle, &locations(here.path()), None, &none).unwrap();
    let hosts: Vec<_> = plan
        .hosts
        .iter()
        .map(|h| (h.account, h.kind, h.host.as_str()))
        .collect();
    assert_eq!(
        hosts,
        [
            (0, "base_url", "https://other.example.com"),
            (1, "base_url", "https://proxy.example.com:8443"),
            (4, "cli_path", "/opt/tools/aws"),
        ],
        "the loopback and credential-less accounts give no entry"
    );
    for host in &plan.hosts {
        assert!(
            !host.host.contains("pw") && !host.host.contains("/v1") && !host.host.contains('?'),
            "no user, path or query"
        );
    }
    assert_eq!(plan.hosts[1].text.id, "bundle.host.url");
    assert_eq!(
        plan.hosts[1].text.resolve("en-US"),
        "Account 2 (litellm) sends its key to https://proxy.example.com:8443."
    );
    assert_eq!(plan.hosts[2].text.id, "bundle.host.program");
    assert_eq!(plan.hosts[2].provider, "aws_bedrock");
    let json = serde_json::to_value(&plan).unwrap();
    assert_eq!(json["hosts"][0]["host"], "https://other.example.com");

    // Once restored, a new plan keeps the file and lists nothing.
    plan.apply(true, |_| true).unwrap();
    let again = super::plan(&bundle, &locations(here.path()), None, &none).unwrap();
    assert!(again.hosts.is_empty());
    assert!(serde_json::to_value(&again).unwrap().get("hosts").is_none());
}

#[test]
fn apply_refuses_unconfirmed_hosts_before_writing() {
    let bundle = backup_of(INCOMING);
    let none = |_: &str| None;

    // A fresh computer: the accounts file is an addition.
    let fresh = tempfile::tempdir().unwrap();
    let plan_fresh = plan(&bundle, &locations(fresh.path()), None, &none).unwrap();
    assert!(!plan_fresh.hosts.is_empty());
    let before = snapshot(fresh.path());
    assert_eq!(plan_fresh.apply(false, |_| true), Err(BundleError::Hosts));
    assert_eq!(
        snapshot(fresh.path()),
        before,
        "nothing written, hooks and themes included"
    );
    assert_eq!(BundleError::Hosts.exit_code(), 1);
    assert_eq!(BundleError::Hosts.l10n().id, "bundle.error.hosts");

    // A replacement the person declines writes the rest without asking.
    let here = tempfile::tempdir().unwrap();
    write(here.path(), "config/cuotai.toml", LOCAL);
    let declined = plan(&bundle, &locations(here.path()), None, &none).unwrap();
    assert!(!declined.hosts.is_empty());
    declined
        .apply(false, |item| item.path != "config/cuotai.toml")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(here.path().join("config/cuotai.toml")).unwrap(),
        LOCAL
    );
    assert!(here.path().join("user/themes/night.css").exists());

    // Confirmed, it writes.
    assert!(plan_fresh.apply(true, |_| true).unwrap() >= 3);
    assert_eq!(
        std::fs::read_to_string(fresh.path().join("config/cuotai.toml")).unwrap(),
        INCOMING
    );

    // Without the accounts file (`--only prefs`, `--only user`) there is
    // nothing to confirm.
    let other = tempfile::tempdir().unwrap();
    let user_only = plan(&bundle, &locations(other.path()), Some(Part::User), &none).unwrap();
    assert!(user_only.hosts.is_empty());
    assert_eq!(user_only.apply(false, |_| true).unwrap(), 1);
}
