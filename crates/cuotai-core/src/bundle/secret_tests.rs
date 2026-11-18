//! PRD 0046 re-validation M-04: the shapes the backup scan refuses, and the
//! ordinary settings it must keep accepting.

use super::*;

fn at() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

/// A settings tree like the one a person keeps: the TOML, hooks, desktop
/// and cost settings, a theme and a language pack. `hooks` is the text of
/// `hooks.toml`.
fn tree(hooks: &str) -> (tempfile::TempDir, Locations) {
    let dir = tempfile::tempdir().unwrap();
    let write = |path: &str, text: &str| {
        let path = dir.path().join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    };
    write(
        "config/cuotai.toml",
        "# my accounts\n[[accounts]]\nid = \"acct-one\"\nname = \"Groq\"\nprovider = \"groq\"\ncredential_env = \"GROQ_WORK\"\n\n[[accounts]]\nid = \"acct-two\"\nname = \"Claude\"\nprovider = \"claude\"\n\n[accounts.oauth]\ncredential_id = \"claude-work\"\n",
    );
    write("config/hooks.toml", hooks);
    write(
        "config/desktop.json",
        r#"{"motion":"off","config_file":"/Users/someone/c.toml","serve":{"enabled":true,"port":6740,"token":true}}"#,
    );
    write("data/cost/settings.json", r#"{"retention_days":60}"#);
    write(
        "data/cost/pricing.json",
        r#"{"currency":"USD","models":{}}"#,
    );
    write("user/themes/night.css", ":root{--accent:#336699}");
    write("user/locales/xx.json", "{}");
    let locations = Locations {
        config: dir.path().join("config/cuotai.toml"),
        desktop_settings: Some(dir.path().join("config/desktop.json")),
        user_dir: Some(dir.path().join("user")),
        data_root: Some(dir.path().join("data")),
    };
    (dir, locations)
}

#[test]
fn each_secret_shape_in_hooks_stops_the_backup() {
    for (case, leak) in [
        "https://hooks.slack.com/services/T0000000/B0000000/XXXXXXXXXXXXXXXXXXXXXXXX",
        "https://discord.com/api/webhooks/123456789012345678/abcdefghijklmnopqrstuvwxyz",
        "https://ptb.discordapp.com/api/webhooks/123456789012345678/abcdefgh",
        "Authorization: Bearer 0123456789abcdef0123456789abcdef",
        "authorization: bearer 0123456789abcdef0123456789abcdef",
        "xai-0123456789abcdefghijklmnopqrstuvwxyzABCD",
        "ya29.0123456789abcdefghijklmnopqrst",
        "1//0abcdefghijklmnopqrstuvwxyz",
        "ASIA0123456789ABCDEF",
        "AKIA0123456789ABCDEF",
        "sk_test_0123456789abcdefghij",
        "rk_test_0123456789abcdefghij",
        "glptt-0123456789abcdefghij",
        "xoxa-0123456789abcdefghij",
        "xoxr-0123456789abcdefghij",
        "xoxs-0123456789abcdefghij",
        "https://user:p4ssw0rd@host.example.com/x",
        "aws_secret_access_key = wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY",
        "AWS_SECRET_ACCESS_KEY=\"wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY\"",
        "-----BEGIN RSA PRIVATE KEY-----",
    ]
    .iter()
    .enumerate()
    {
        let (_dir, locations) = tree(&format!(
            "[[hooks]]\nname = \"n\"\ncommand = \"curl {leak}\"\n"
        ));
        let refused = collect(&locations, at(), "9.9.9").unwrap_err();
        assert_eq!(
            refused,
            BundleError::Secret {
                path: "config/hooks.toml".into()
            },
            "leak case {case}"
        );
    }
}

#[test]
fn ordinary_settings_never_look_secret() {
    let (_dir, locations) = tree("[[hooks]]\nname = \"n\"\n");
    assert!(collect(&locations, at(), "9.9.9").is_ok());

    let fingerprint = "0123456789abcdef".repeat(4);
    let colours = (0..40)
        .map(|n| format!("#{:06x}", n * 0x0a0b0c))
        .collect::<Vec<_>>()
        .join(", ");
    for (case, fine) in [
        "[[accounts]]\nname = \"sk-learning group\"\nprovider = \"alibaba\"\nsecret_env = \"ALIBABA_SECRET\"\ncredential_env = \"XAI_API_KEY\"\n".to_owned(),
        format!(
            "{{\"schema\":1,\"key\":\"{fingerprint}\",\"projects\":[{{\"project\":\"{fingerprint}\",\"label\":\"Client work\"}}]}}"
        ),
        "[[hooks]]\nname = \"n\"\ncommand = \"curl -d done https://ntfy.sh/topic\"\n".to_owned(),
        "# The server answers with a Bearer token that Cuotai never stores.\n".to_owned(),
        "# Send it as Authorization: Bearer $CUOTAI_SERVE_TOKEN\n".to_owned(),
        format!(":root {{ --palette: {colours}; }}"),
        format!(":root {{ --hex: #{fingerprint}; }}"),
        "region = \"ASIA_PACIFIC_SOUTHEAST\"\nname = \"ASIAPACIFIC\"\n".to_owned(),
        "url = \"https://example.com:8443/path\"\nwho = \"someone@example.com\"\n".to_owned(),
        "aws_secret_access_key = \"${AWS_SECRET}\"\n".to_owned(),
        "# -----BEGIN\n# PRIVATE KEY is never stored here\n".to_owned(),
        "https://hooks.slack.com/services/short".to_owned(),
    ]
    .iter()
    .enumerate()
    {
        assert!(!looks_secret(fine), "ordinary case {case} was refused");
    }
}
