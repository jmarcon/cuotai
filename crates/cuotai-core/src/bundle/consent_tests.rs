//! PRD 0046 RF3.1 (re-validation G-0046-1): a restore keeps this
//! computer's consent and exposure switches, and the plan lists every one
//! the backup had on.

use super::*;

/// A `desktop.json` with every path of [`DESKTOP_CONSENT_PATHS`] on, and
/// sub-choices that travel.
const ALL_ON: &str = r#"{
    "motion": "off",
    "serve": {"enabled": true, "port": 6740, "token": true, "allow_browsers": true,
              "allowed_origins": ["https://dash.example.com"], "stats": true,
              "access_log": "all"},
    "updater": {"automatic": true, "consent_answered": true,
                "last_check": "2026-11-01T00:00:00Z"},
    "detect": {"enabled": true, "dismissed": ["codex"]},
    "widgets": {"enabled": true, "include_identity": true, "max_accounts": 3},
    "status_queries": true,
    "notify": {"enabled": true, "private": false, "lead_minutes": 15,
               "digest": {"enabled": true, "weekday": 4, "hour": 8}},
    "alerts": {"enabled": true, "thresholds": [50]},
    "hooks": true
}"#;

const TABLE_ORDER: [&str; 14] = [
    "serve",
    "serve_browsers",
    "serve_stats",
    "serve_access_log",
    "updates",
    "detect",
    "widgets",
    "widget_identity",
    "status_queries",
    "notify",
    "notify_names",
    "digest",
    "alerts",
    "hooks",
];

fn merged(local: Option<&str>) -> (serde_json::Value, Vec<&'static str>) {
    let (bytes, held) = merge_desktop(local.map(str::as_bytes), ALL_ON.as_bytes()).unwrap();
    (serde_json::from_slice(&bytes).unwrap(), held)
}

#[test]
fn a_restore_keeps_every_consent_switch_of_this_computer() {
    // (a) A fresh computer: no listed path arrives, the choices travel.
    let (desktop, held) = merged(None);
    let object = desktop.as_object().unwrap();
    for (path, _) in DESKTOP_CONSENT_PATHS {
        assert!(json_path(object, path).is_none(), "{path:?} arrived");
    }
    assert_eq!(desktop["motion"], "off");
    assert_eq!(desktop["serve"]["port"], 6740);
    assert_eq!(desktop["serve"]["token"], true);
    assert_eq!(desktop["alerts"]["thresholds"], serde_json::json!([50]));
    assert_eq!(desktop["notify"]["lead_minutes"], 15);
    assert_eq!(desktop["notify"]["digest"]["weekday"], 4);
    assert_eq!(desktop["detect"]["dismissed"], serde_json::json!(["codex"]));
    assert_eq!(desktop["widgets"]["max_accounts"], 3);
    assert_eq!(held, TABLE_ORDER);
    // The desktop crate's `absent_consent_paths_read_as_off` reads this
    // document; keep the two in step.
    assert_eq!(
        desktop,
        serde_json::json!({
            "motion": "off",
            "serve": {"port": 6740, "token": true},
            "updater": {},
            "detect": {"dismissed": ["codex"]},
            "widgets": {"max_accounts": 3},
            "notify": {"lead_minutes": 15, "digest": {"weekday": 4, "hour": 8}},
            "alerts": {"thresholds": [50]}
        })
    );

    // (b) This computer's own server, browsers and origins stay.
    let local = r#"{"serve": {"enabled": true, "port": 6738, "token": true,
        "allow_browsers": true, "allowed_origins": ["https://mine.example.com"]}}"#;
    let (desktop, held) = merged(Some(local));
    assert_eq!(desktop["serve"]["enabled"], true);
    assert_eq!(desktop["serve"]["allow_browsers"], true);
    assert_eq!(
        desktop["serve"]["allowed_origins"],
        serde_json::json!(["https://mine.example.com"]),
        "a foreign origin is never kept"
    );
    assert_eq!(desktop["serve"]["port"], 6740, "the port travels");
    assert!(desktop["serve"].get("stats").is_none());
    assert!(!held.contains(&"serve") && !held.contains(&"serve_browsers"));
    assert_eq!(held, TABLE_ORDER[2..]);

    // (c) This computer's answer to the update question stays.
    let local = r#"{"updater": {"automatic": false, "consent_answered": true}}"#;
    let (desktop, held) = merged(Some(local));
    assert_eq!(
        desktop["updater"],
        serde_json::json!({"automatic": false, "consent_answered": true})
    );
    assert!(held.contains(&"updates"));
}

#[test]
fn a_backup_with_everything_off_holds_nothing() {
    let off = br#"{"serve":{"enabled":false,"access_log":"off"},"notify":{"private":true},"updater":{"automatic":false,"consent_answered":true},"hooks":false}"#;
    let (_, held) = merge_desktop(None, off).unwrap();
    assert!(held.is_empty(), "{held:?}");
}

#[test]
fn the_plan_lists_each_held_switch_as_a_setting_step() {
    let source = tempfile::tempdir().unwrap();
    let locations = |root: &Path| Locations {
        config: root.join("config/cuotai.toml"),
        desktop_settings: Some(root.join("config/desktop.json")),
        user_dir: Some(root.join("user")),
        data_root: Some(root.join("data")),
    };
    std::fs::create_dir_all(source.path().join("config")).unwrap();
    std::fs::write(source.path().join("config/cuotai.toml"), "").unwrap();
    std::fs::write(source.path().join("config/desktop.json"), ALL_ON).unwrap();
    let at = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);
    let bundle = collect(&locations(source.path()), at, "9.9.9").unwrap();
    let fresh = tempfile::tempdir().unwrap();
    let none = |_: &str| None;
    let plan = plan(&bundle, &locations(fresh.path()), None, &none).unwrap();
    let steps: Vec<_> = plan
        .next_steps
        .iter()
        .map(|step| (step.kind, step.account, step.provider.as_str()))
        .collect();
    let expected: Vec<_> = TABLE_ORDER.iter().map(|id| ("setting", 0, *id)).collect();
    assert_eq!(steps, expected);
    assert_eq!(plan.needs.len(), plan.next_steps.len());
    assert_eq!(
        plan.needs[1].resolve("en-US"),
        "Allow web browsers was on in the backup and stays off on this computer; turn it on in Settings if you want it."
    );
    assert_eq!(plan.needs[1].id, "bundle.need.setting");
    let json = serde_json::to_value(&plan).unwrap();
    assert_eq!(json["next_steps"][0]["kind"], "setting");

    // Only the accounts: no desktop settings, no setting step.
    let plan = super::plan(&bundle, &locations(fresh.path()), Some(Part::Config), &none).unwrap();
    assert!(plan.next_steps.is_empty());
}
