//! PRD 0044 RF4.1 and RF5.1 (re-validation V-06, V-11): the consent shows
//! the address's port and an example of a kind the channel receives.

use super::*;
use chrono::TimeZone;

fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 5, 4, 12, 0, 0).unwrap()
}

#[test]
fn a_non_default_port_is_part_of_the_shown_host() {
    let local = validate_url(ChannelKind::Ntfy, "http://127.0.0.1:8080/t").unwrap();
    assert_eq!(local.host(), "127.0.0.1:8080");
    assert_eq!(local.masked(), "http://127.0.0.1:8080/••••");
    let v6 = validate_url(ChannelKind::Ntfy, "http://[::1]:8080/t").unwrap();
    assert_eq!(v6.host(), "[::1]:8080");
    for plain in ["https://ntfy.sh/t", "https://ntfy.sh:443/t"] {
        let valid = validate_url(ChannelKind::Ntfy, plain).unwrap();
        assert_eq!(valid.host(), "ntfy.sh", "{plain}");
        assert_eq!(valid.masked(), "https://ntfy.sh/••••");
    }
    let hook = validate_url(ChannelKind::Webhook, "https://relay.example.com:8443/h").unwrap();
    assert_eq!(hook.host(), "relay.example.com:8443");
    // The stored host still passes the hand-edit check of `desktop.json`.
    let channel = Channel {
        id: "0123456789abcdef".into(),
        kind: ChannelKind::Ntfy,
        label: "Phone".into(),
        host: local.host().to_owned(),
        filters: Filters::default(),
        enabled: true,
        created_at: now(),
        signed: false,
    };
    assert!(well_formed(&channel));
}

fn example_kind(filters: Filters) -> String {
    let text = payload::example(ChannelKind::Webhook, &filters, true, "en-US", now());
    let value: serde_json::Value = serde_json::from_str(&text).expect(&text);
    assert_eq!(value["sample"], true, "{text}");
    value["kind"].as_str().unwrap().to_owned()
}

#[test]
fn the_example_is_of_the_first_kind_the_draft_admits() {
    let kinds = |kinds: &[NoticeKind]| Filters {
        kinds: kinds.iter().copied().collect(),
        ..Filters::default()
    };
    assert_eq!(example_kind(Filters::default()), "threshold");
    assert_eq!(example_kind(kinds(&[NoticeKind::Digest])), "digest");
    assert_eq!(
        example_kind(kinds(&[NoticeKind::Reset, NoticeKind::Digest])),
        "reset"
    );
    assert_eq!(
        example_kind(Filters {
            min_severity: Severity::Critical,
            ..Filters::default()
        }),
        "exhausted"
    );
    assert_eq!(example_kind(kinds(&[NoticeKind::Floor])), "threshold");
    let digest = payload::example(
        ChannelKind::Ntfy,
        &kinds(&[NoticeKind::Digest]),
        true,
        "en-US",
        now(),
    );
    assert!(
        digest.contains("Tags: calendar") && digest.contains("1 limit hit"),
        "{digest}"
    );
}

#[test]
fn the_consent_of_a_digest_channel_shows_a_digest() {
    let draft = Draft {
        kind: ChannelKind::Webhook,
        label: "Weekly".into(),
        filters: Filters {
            kinds: [NoticeKind::Digest].into(),
            ..Filters::default()
        },
        sign: false,
    };
    let consent = consent(
        &draft,
        "https://relay.example.com:8443/hook",
        true,
        "en-US",
        now(),
    )
    .unwrap();
    assert_eq!(consent.host, "relay.example.com:8443");
    assert_eq!(consent.masked, "https://relay.example.com:8443/••••");
    assert!(
        consent.example.contains("\"kind\": \"digest\""),
        "{}",
        consent.example
    );
}
