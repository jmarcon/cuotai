//! PRD 0044: channel validation, storage, payloads and delivery, against
//! local mock servers only.

use super::delivery::{Timing, HOURLY_CAP};
use super::log::{DeliveryLog, LogOutcome};
use super::payload::{self, prepare, render, Source};
use super::*;
use crate::alerts::{AlertEvent, AlertKind};
use crate::oauth::OAuthError;
use crate::{Account, MetricWindow, Provider};
use chrono::{TimeDelta, TimeZone};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Default)]
struct MemoryStore(Mutex<HashMap<String, OAuthToken>>);

impl TokenStore for MemoryStore {
    fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
        Ok(self.0.lock().unwrap().get(id).cloned())
    }
    fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
        self.0.lock().unwrap().insert(id.into(), token.clone());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), OAuthError> {
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}

fn memory() -> (Arc<MemoryStore>, Vault) {
    let store = Arc::new(MemoryStore::default());
    (Arc::clone(&store), Vault::new(store))
}

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 2, 14, 0, 0).unwrap()
}

const SLACK: &str = "https://hooks.slack.com/services/T000/B000/SECRETPATHTOKEN";
const DISCORD: &str = "https://discord.com/api/webhooks/123/SECRETPATHTOKEN";

// ---------------------------------------------------------------------------
// RF1.2: validation.

#[test]
fn every_rejection_has_its_message_and_never_echoes_the_url() {
    let long = format!("https://example.com/{}", "a".repeat(MAX_URL_BYTES));
    let table: &[(ChannelKind, &str, &str)] = &[
        (ChannelKind::Webhook, "", "channels.error.url_empty"),
        (ChannelKind::Webhook, &long, "channels.error.url_long"),
        (
            ChannelKind::Webhook,
            "not a url",
            "channels.error.url_invalid",
        ),
        (
            ChannelKind::Webhook,
            "ftp://example.com/x",
            "channels.error.url_invalid",
        ),
        (
            ChannelKind::Webhook,
            "https://user:pw@example.com/hook",
            "channels.error.url_userinfo",
        ),
        (
            ChannelKind::Webhook,
            "https://example.com/hook#frag",
            "channels.error.url_fragment",
        ),
        (
            ChannelKind::Webhook,
            "http://example.com/hook",
            "channels.error.url_http",
        ),
        (
            ChannelKind::Webhook,
            "http://127.0.0.1/hook",
            "channels.error.url_http",
        ),
        (
            ChannelKind::Ntfy,
            "http://ntfy.example.com/topic",
            "channels.error.url_http",
        ),
        (
            ChannelKind::Webhook,
            "https://api.openai.com/v1/x",
            "channels.error.url_provider",
        ),
        (
            ChannelKind::Slack,
            "https://example.com/services/x",
            "channels.error.url_slack",
        ),
        (
            ChannelKind::Slack,
            "https://hooks.slack.com/other/x",
            "channels.error.url_slack",
        ),
        (
            ChannelKind::Discord,
            "https://discord.com/channels/1",
            "channels.error.url_discord",
        ),
        (
            ChannelKind::Discord,
            "https://evil.com/api/webhooks/1/x",
            "channels.error.url_discord",
        ),
        (
            ChannelKind::Ntfy,
            "https://ntfy.sh/",
            "channels.error.url_ntfy",
        ),
        (
            ChannelKind::Ntfy,
            "https://ntfy.sh/a/b",
            "channels.error.url_ntfy",
        ),
        (
            ChannelKind::Ntfy,
            "https://ntfy.sh/bad.topic",
            "channels.error.url_ntfy",
        ),
        (
            ChannelKind::Ntfy,
            "https://ntfy.sh/topic?x=1",
            "channels.error.url_ntfy",
        ),
    ];
    for (kind, url, id) in table {
        let error = validate_url(*kind, url).expect_err(url);
        assert_eq!(error.id, *id, "{url}");
        assert!(!error.fallback.contains("pw@"), "{url}");
        assert!(!error.fallback.contains("SECRET"));
    }
    let long_topic = format!("https://ntfy.sh/{}", "t".repeat(65));
    assert!(validate_url(ChannelKind::Ntfy, &long_topic).is_err());
}

#[test]
fn accepted_urls_show_only_the_host() {
    let slack = validate_url(ChannelKind::Slack, SLACK).unwrap();
    assert_eq!(slack.host(), "hooks.slack.com");
    assert_eq!(slack.masked(), "https://hooks.slack.com/••••");
    assert!(!format!("{slack:?}").contains("SECRET"));
    assert!(validate_url(ChannelKind::Discord, DISCORD).is_ok());
    assert!(validate_url(
        ChannelKind::Discord,
        "https://discordapp.com/api/webhooks/1/x"
    )
    .is_ok());
    let ntfy = validate_url(ChannelKind::Ntfy, "https://ntfy.sh/my_topic-1").unwrap();
    assert!(ntfy.warning().is_none());
    for local in [
        "http://127.0.0.1:8080/alerts",
        "http://192.168.1.20/alerts",
        "http://10.0.0.5/alerts",
        "http://172.16.3.4/alerts",
        "http://localhost/alerts",
        "http://[::1]/alerts",
    ] {
        let valid = validate_url(ChannelKind::Ntfy, local).expect(local);
        assert_eq!(
            valid.warning().map(|w| w.id.as_ref()),
            Some("channels.warning.plain_http")
        );
    }
    assert!(validate_url(ChannelKind::Webhook, "https://relay.example.com/hook?key=1").is_ok());
}

#[test]
fn labels_are_bounded() {
    assert_eq!(validate_label("  Phone ").unwrap(), "Phone");
    assert!(validate_label("").is_err());
    assert!(validate_label(&"x".repeat(65)).is_err());
    assert!(validate_label("a\u{7}b").is_err());
}

// ---------------------------------------------------------------------------
// RF1.3: filters.

#[test]
fn filters_default_to_everything_but_the_digest_and_round_trip() {
    let filters = Filters::default();
    assert!(!filters.kinds.contains(&NoticeKind::Digest));
    assert_eq!(filters.kinds.len(), NoticeKind::ALL.len() - 1);
    assert!(filters.names(false) && !filters.names(true));
    let json = serde_json::to_value(&filters).unwrap();
    assert_eq!(serde_json::from_value::<Filters>(json).unwrap(), filters);
    let read: Filters = serde_json::from_str("{}").unwrap();
    assert_eq!(read, filters);
    let mut custom = Filters::default();
    custom.apply("kinds=threshold,exhausted,floor").unwrap();
    custom.apply("accounts=acc_a, acc_b").unwrap();
    custom.apply("min_severity=warning").unwrap();
    custom.apply("names=on").unwrap();
    assert!(custom.admits(NoticeKind::Exhausted) && !custom.admits(NoticeKind::Reset));
    assert!(custom.admits_account("acc_a") && !custom.admits_account("acc_c"));
    assert!(custom.names(true));
    let back: Filters = serde_json::from_value(serde_json::to_value(&custom).unwrap()).unwrap();
    assert_eq!(back, custom);
    assert!(custom.apply("kinds=nope").is_err());
    assert!(custom.apply("colour=red").is_err());
    let words = filter_words(&custom, true).resolve("en-US");
    assert_eq!(
        words,
        "Thresholds, Exhaustion, Balance floors · of 2 accounts · warnings and critical only · names shown"
    );
    assert_eq!(
        filter_words(&Filters::default(), true).resolve("en-US"),
        "Resets, Thresholds, Exhaustion, Recovery, Projected exhaustion, Extra usage, Balance floors, Session expiry, Limit changes · of every account · names hidden"
    );
    assert_eq!(NoticeKind::Exhausted.severity(), Severity::Critical);
    assert_eq!(NoticeKind::Floor.severity(), Severity::Critical);
    assert_eq!(NoticeKind::Threshold.severity(), Severity::Warning);
    assert_eq!(NoticeKind::Reset.severity(), Severity::Info);
    assert_eq!(NoticeKind::Recovered.severity(), Severity::Info);
}

// ---------------------------------------------------------------------------
// RF1.1 / RF4: storage, vault and consent.

#[test]
fn the_url_lives_in_the_vault_never_in_the_preferences() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("desktop.json");
    std::fs::write(&settings, r#"{"theme":"dusk","notify":{"enabled":true}}"#).unwrap();
    let (store, vault) = memory();
    let draft = Draft {
        kind: ChannelKind::Slack,
        label: "Team".into(),
        filters: Filters::default(),
        sign: true,
    };
    let saved = add(&settings, &vault, &draft, SLACK, t0()).unwrap();
    assert!(saved.signing_secret.is_none(), "only webhooks sign");
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(!text.contains("SECRETPATHTOKEN") && !text.contains("/services/"));
    assert!(text.contains("hooks.slack.com") && text.contains("\"theme\": \"dusk\""));
    assert!(text.contains("\"notify\""));
    let channel = &read(&settings)[0];
    assert_eq!(channel.host, "hooks.slack.com");
    assert!(channel.enabled);
    assert_eq!(vault.url(channel).unwrap().expose().as_str(), SLACK);
    assert!(store
        .0
        .lock()
        .unwrap()
        .contains_key(&Vault::url_key(&channel.id)));
    // RF4.3: the same URL needs no new consent; another one does.
    assert!(!needs_consent(&vault, channel, None));
    assert!(!needs_consent(&vault, channel, Some(SLACK)));
    let other = "https://hooks.slack.com/services/T1/B1/OTHER";
    assert!(needs_consent(&vault, channel, Some(other)));
    let edited = edit(&settings, &vault, &channel.id, &draft, Some(other)).unwrap();
    assert_eq!(vault.url(&edited.channel).unwrap().expose().as_str(), other);
    // Disabling needs nothing else.
    assert!(!set_enabled(&settings, &channel.id, false).unwrap().enabled);
    // Removing deletes the vault entries.
    remove(&settings, &vault, &channel.id).unwrap();
    assert!(read(&settings).is_empty());
    assert!(store.0.lock().unwrap().is_empty());
    let text = std::fs::read_to_string(&settings).unwrap();
    assert!(!text.contains("\"channels\"") && text.contains("dusk"));
}

#[test]
fn a_signed_webhook_keeps_its_secret_in_the_vault_and_shows_it_once() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("desktop.json");
    let (_, vault) = memory();
    let draft = Draft {
        kind: ChannelKind::Webhook,
        label: "Relay".into(),
        filters: Filters::default(),
        sign: true,
    };
    let saved = add(
        &settings,
        &vault,
        &draft,
        "https://relay.example.com/h",
        t0(),
    )
    .unwrap();
    let secret = saved.signing_secret.clone().unwrap();
    assert_eq!(secret.len(), 64);
    assert!(!format!("{saved:?}").contains(&secret));
    assert!(!std::fs::read_to_string(&settings)
        .unwrap()
        .contains(&secret));
    assert_eq!(
        vault.secret(&saved.channel).unwrap().as_deref(),
        Some(secret.as_str())
    );
}

#[test]
fn at_most_five_channels_and_a_later_schema_is_never_rewritten() {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("desktop.json");
    let (_, vault) = memory();
    let draft = |n: usize| Draft {
        kind: ChannelKind::Ntfy,
        label: format!("Phone {n}"),
        filters: Filters::default(),
        sign: false,
    };
    for n in 0..MAX_CHANNELS {
        add(&settings, &vault, &draft(n), "https://ntfy.sh/topic", t0()).unwrap();
    }
    let error = add(&settings, &vault, &draft(9), "https://ntfy.sh/topic", t0()).unwrap_err();
    assert_eq!(error.id, "channels.error.too_many");
    let later = dir.path().join("later.json");
    std::fs::write(&later, r#"{"schema":2}"#).unwrap();
    assert!(add(&later, &vault, &draft(0), "https://ntfy.sh/topic", t0()).is_err());
    assert_eq!(std::fs::read_to_string(&later).unwrap(), r#"{"schema":2}"#);
}

/// A hand-edited `desktop.json` entry whose id, label or host Cuotai
/// would not have written is skipped, so it never reaches a vault key,
/// an HTML attribute or the terminal.
#[test]
fn hand_edited_channels_with_a_bad_id_label_or_host_are_skipped() {
    let entry = |id: &str, label: &str, host: &str| {
        serde_json::json!({
            "id": id, "kind": "webhook", "label": label, "host": host,
            "created_at": "2026-11-04T12:00:00Z",
        })
    };
    let document = serde_json::json!({ "channels": [
        entry("0123456789abcdef", "Ops & alerts", "hooks.example.com"),
        entry("0123456789abcdef", "IPv6", "[::1]:8080"),
        entry("0123456789ABCDEF", "Upper", "hooks.example.com"),
        entry("0123456789abcde", "Short", "hooks.example.com"),
        entry("\" onclick=\"x\"0123", "Quote", "hooks.example.com"),
        entry("0123456789abcdef", "", "hooks.example.com"),
        entry("0123456789abcdef", "Bell\u{7}", "hooks.example.com"),
        entry("0123456789abcdef", "Spaced", "evil.example.com/<b>"),
        entry("0123456789abcdef", "Escape", "a\u{1b}[2J.example.com"),
        entry("0123456789abcdef", "Empty host", ""),
    ]});
    let labels: Vec<String> = from_document(&document)
        .into_iter()
        .map(|channel| channel.label)
        .collect();
    assert_eq!(labels, ["Ops & alerts", "IPv6"]);
}

#[test]
fn the_consent_shows_host_filters_example_and_the_sentence() {
    let draft = Draft {
        kind: ChannelKind::Discord,
        label: "Ops".into(),
        filters: Filters::default(),
        sign: false,
    };
    let consent = consent(&draft, DISCORD, true, "en-US", t0()).unwrap();
    assert_eq!(consent.host, "discord.com");
    assert!(!consent.masked.contains("SECRET"));
    assert_eq!(
        consent.sentence.fallback,
        "Cuotai will send these texts to this address when a notification is due. Nothing else leaves this computer."
    );
    assert!(consent.example.contains("\"content\""));
    assert!(!consent.example.contains("Work"), "names hidden by default");
    let named = Draft {
        filters: Filters {
            include_names: Some(true),
            ..Filters::default()
        },
        ..draft
    };
    let shown = super::consent(&named, DISCORD, true, "en-US", t0()).unwrap();
    assert!(shown.example.contains("Work"));
    let ntfy = super::payload::example(ChannelKind::Ntfy, &Filters::default(), true, "en-US", t0());
    assert!(ntfy.starts_with("Title: Claude"), "{ntfy}");
    assert!(ntfy.contains("Priority: 4") && ntfy.contains("Tags: warning"));
    let hook = super::payload::example(
        ChannelKind::Webhook,
        &Filters::default(),
        true,
        "en-US",
        t0(),
    );
    assert!(hook.contains("\"sample\": true"));
}

// ---------------------------------------------------------------------------
// RF2: payloads.

fn account(name: &str) -> Account {
    Account {
        name: name.into(),
        provider: Provider::Claude,
        ..Account::default()
    }
}

fn alert(account: &Account, kind: AlertKind, threshold: u8) -> AlertEvent {
    AlertEvent {
        account_key: account.state_key(),
        label: "Session (5h)".into(),
        window: Some(MetricWindow::Window5h),
        kind,
        threshold,
        consumed: f64::from(threshold) / 100.0,
        resets_at: Some(t0() + TimeDelta::hours(2)),
        projection: None,
        overage: None,
        limit_change: None,
        row: None,
    }
}

fn utc() -> chrono::FixedOffset {
    chrono::FixedOffset::east_opt(0).unwrap()
}

fn outgoing(source: &Source, names: bool) -> Outgoing {
    let filters = Filters {
        kinds: NoticeKind::ALL.into_iter().collect(),
        include_names: Some(names),
        ..Filters::default()
    };
    prepare(source, &filters, true, "en-US", t0(), &utc()).unwrap()
}

fn json(kind: ChannelKind, out: &Outgoing) -> serde_json::Value {
    serde_json::from_slice(&render(kind, out, None).body).unwrap()
}

fn digest() -> crate::digest::Digest {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::history::HistoryStore::new(
        dir.path().join("h.sqlite3"),
        &crate::config_file::ConfigIdentity::Explicit("channels".into()),
    );
    crate::digest::weekly(
        &store,
        &[account("Work")],
        &[None],
        crate::digest::DigestWeek::Last,
        t0(),
        &utc(),
        Default::default(),
    )
    .unwrap()
}

#[test]
fn golden_payloads_for_a_threshold_an_exhaustion_and_a_digest() {
    let work = account("Work");
    let threshold = Source::Alerts {
        account: work.clone(),
        events: vec![alert(&work, AlertKind::Threshold, 90)],
    };
    let exhausted = Source::Alerts {
        account: work.clone(),
        events: vec![alert(&work, AlertKind::Exhausted, 100)],
    };
    let out = outgoing(&threshold, false);
    let hook = json(ChannelKind::Webhook, &out);
    assert_eq!(hook["schema"], 1);
    assert_eq!(hook["kind"], "threshold");
    assert_eq!(hook["severity"], "warning");
    assert_eq!(hook["title"], out.title);
    assert_eq!(hook["provider"], "claude");
    assert_eq!(hook["account"]["id"], work.state_key());
    assert!(hook["account"].get("name").is_none(), "names hidden");
    assert_eq!(hook["metric"], serde_json::json!({"window": "window_5h"}));
    assert_eq!(hook["current"]["percent"], 90.0);
    assert_eq!(hook["resets_at"], "2026-11-02T16:00:00Z");
    assert!(hook.get("link").is_none() && hook.get("sample").is_none());
    let named = json(ChannelKind::Webhook, &outgoing(&threshold, true));
    assert_eq!(named["account"]["name"], "Work");
    assert_eq!(named["metric"]["label"], "Session (5h)");

    let critical = outgoing(&exhausted, false);
    assert_eq!(critical.kind, "exhausted");
    assert_eq!(critical.severity, Severity::Critical);
    let ntfy = render(ChannelKind::Ntfy, &critical, None);
    let header = |name: &str| {
        ntfy.headers
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.clone())
            .unwrap()
    };
    assert_eq!(header("priority"), "5");
    assert_eq!(header("tags"), "rotating_light");
    assert_eq!(header("title"), critical.title);
    assert_eq!(String::from_utf8(ntfy.body.clone()).unwrap(), critical.body);

    let slack = json(ChannelKind::Slack, &critical);
    assert_eq!(
        slack["text"],
        format!("{}\n{}", critical.title, critical.body)
    );
    assert_eq!(slack["blocks"][0]["type"], "header");
    assert_eq!(slack["blocks"][1]["text"]["type"], "mrkdwn");
    assert!(slack.get("attachments").is_none());
    let discord = json(ChannelKind::Discord, &critical);
    assert_eq!(
        discord["content"],
        format!("**{}**\n{}", critical.title, critical.body)
    );
    assert_eq!(discord["allowed_mentions"]["parse"], serde_json::json!([]));

    // RF6.1: the digest, Markdown for webhook/Slack/Discord, plain for ntfy.
    let weekly = Source::WeeklyDigest(Box::new(digest()));
    let default = Filters::default();
    assert!(
        prepare(&weekly, &default, true, "en-US", t0(), &utc()).is_none(),
        "off by default per channel"
    );
    let out = outgoing(&weekly, true);
    assert_eq!((out.kind, out.severity), ("digest", Severity::Info));
    let hook = json(ChannelKind::Webhook, &out);
    assert!(hook["body"].as_str().unwrap().starts_with("# "), "Markdown");
    let ntfy = render(ChannelKind::Ntfy, &out, None);
    assert!(
        !String::from_utf8(ntfy.body).unwrap().starts_with('#'),
        "plain"
    );
    assert_eq!(
        json(ChannelKind::Discord, &out)["content"]
            .as_str()
            .unwrap()
            .split('\n')
            .nth(1),
        Some(out.markdown.as_deref().unwrap().lines().next().unwrap())
    );
}

#[test]
fn filters_keep_kinds_accounts_and_severity() {
    let work = account("Work");
    let source = Source::Alerts {
        account: work.clone(),
        events: vec![
            alert(&work, AlertKind::Threshold, 90),
            alert(&work, AlertKind::Exhausted, 100),
        ],
    };
    let critical_only = Filters {
        min_severity: Severity::Critical,
        ..Filters::default()
    };
    let out = prepare(&source, &critical_only, true, "en-US", t0(), &utc()).unwrap();
    assert_eq!(out.kind, "exhausted");
    let others = Filters {
        accounts: ["acc_someone_else".to_owned()].into(),
        ..Filters::default()
    };
    assert!(prepare(&source, &others, true, "en-US", t0(), &utc()).is_none());
    let resets_only = Filters {
        kinds: [NoticeKind::Reset].into(),
        ..Filters::default()
    };
    assert!(prepare(&source, &resets_only, true, "en-US", t0(), &utc()).is_none());
}

/// RF2.2: PRD 0032's redaction over every payload: no credential variable,
/// URL or e-mail, and no account name unless the channel shows names.
#[test]
fn every_payload_is_redacted() {
    let mut secret = account("boss@example.com");
    secret.credential_env = Some("SECRET_ADMIN_KEY".into());
    let sources = [
        Source::Alerts {
            account: secret.clone(),
            events: vec![alert(&secret, AlertKind::Threshold, 75)],
        },
        Source::Alerts {
            account: secret.clone(),
            events: vec![alert(&secret, AlertKind::BalanceBelowFloor, 0)],
        },
        Source::QuietDigest {
            groups: vec![(
                secret.clone(),
                vec![alert(&secret, AlertKind::Threshold, 90)],
            )],
        },
        Source::Test,
        Source::Summary { suppressed: 4 },
    ];
    for source in &sources {
        for names in [false, true] {
            let out = outgoing(source, names);
            for kind in ChannelKind::ALL {
                let request = render(kind, &out, None);
                let body = String::from_utf8_lossy(&request.body).into_owned();
                let headers = format!("{:?}", request.headers);
                for text in [&body, &headers] {
                    assert!(!text.contains("boss@example.com"), "{kind:?} {text}");
                    assert!(!text.contains("SECRET_ADMIN_KEY"));
                    assert!(!text.contains("http"), "{kind:?} {text}");
                    if !names {
                        assert!(!text.contains("boss"), "{kind:?} {text}");
                    }
                }
            }
        }
    }
}

#[test]
fn the_signature_is_the_hmac_of_the_exact_body() {
    let work = account("Work");
    let out = outgoing(
        &Source::Alerts {
            account: work.clone(),
            events: vec![alert(&work, AlertKind::Threshold, 90)],
        },
        false,
    );
    let request = render(ChannelKind::Webhook, &out, Some("key"));
    let signature = &request
        .headers
        .iter()
        .find(|(name, _)| *name == "x-cuotai-signature")
        .unwrap()
        .1;
    assert_eq!(*signature, payload::signature("key", &request.body));
    // A known HMAC-SHA256 vector (RFC 4231 test case 2).
    assert_eq!(
        payload::signature("Jefe", b"what do ya want for nothing?"),
        "sha256=5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
    assert!(render(ChannelKind::Webhook, &out, None)
        .headers
        .iter()
        .all(|(name, _)| *name != "x-cuotai-signature"));
}

#[test]
fn bodies_are_capped_with_an_ellipsis() {
    let mut out = outgoing(&Source::Test, false);
    out.body = "é".repeat(5000);
    let hook = json(ChannelKind::Webhook, &out);
    let body = hook["body"].as_str().unwrap();
    assert!(body.len() <= payload::WEBHOOK_BODY_BYTES && body.ends_with('…'));
    let ntfy = render(ChannelKind::Ntfy, &out, None);
    assert!(ntfy.body.len() <= payload::NTFY_BODY_BYTES);
    let slack = json(ChannelKind::Slack, &out);
    assert!(
        slack["blocks"][1]["text"]["text"].as_str().unwrap().len() <= payload::SLACK_BODY_BYTES
    );
    let discord = json(ChannelKind::Discord, &out);
    let content = discord["content"].as_str().unwrap();
    assert_eq!(content.chars().count(), payload::DISCORD_CHARS);
    assert!(content.ends_with('…'));
    // A non-ASCII ntfy title is an RFC 2047 encoded word.
    out.title = "Cuotai テスト".into();
    let ntfy = render(ChannelKind::Ntfy, &out, None);
    let title = &ntfy.headers.iter().find(|(n, _)| *n == "title").unwrap().1;
    assert!(title.starts_with("=?UTF-8?B?") && title.is_ascii());
}

#[test]
fn the_test_is_marked_as_a_sample() {
    let out = outgoing(&Source::Test, false);
    assert_eq!(out.title, "Cuotai test");
    assert_eq!(json(ChannelKind::Webhook, &out)["sample"], true);
}

// ---------------------------------------------------------------------------
// RF3: delivery against local mock servers.

struct Fixture {
    _dir: tempfile::TempDir,
    settings: std::path::PathBuf,
    log: DeliveryLog,
    vault: Vault,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("desktop.json");
    let log = DeliveryLog::at(dir.path().join("channels").join("log.jsonl"));
    let (_, vault) = memory();
    Fixture {
        _dir: dir,
        settings,
        log,
        vault,
    }
}

impl Fixture {
    /// A channel posting to `url` (a mock server), stored as `add` would.
    fn channel(&self, kind: ChannelKind, url: &str, filters: Filters) -> Channel {
        let channel = Channel {
            id: new_id(),
            kind,
            label: "Mock".into(),
            host: "127.0.0.1".into(),
            filters,
            enabled: true,
            created_at: t0(),
            signed: false,
        };
        self.vault.put(&Vault::url_key(&channel.id), url).unwrap();
        let stored = channel.clone();
        mutate(&self.settings, move |channels| {
            channels.push(stored);
            Ok(())
        })
        .unwrap();
        channel
    }

    fn dispatcher(&self, started: DateTime<Utc>, timing: Timing) -> Dispatcher {
        Dispatcher::with(
            self.settings.clone(),
            self.vault.clone(),
            Some(self.log.clone()),
            started,
            timing,
        )
        .unwrap()
        .loose()
    }

    /// Wait until the log holds `count` final outcomes.
    async fn settled(&self, count: usize) -> Vec<log::Entry> {
        for _ in 0..2000 {
            let entries = self.log.read();
            if entries.len() >= count {
                return entries;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!(
            "the log never reached {count} entries: {:?}",
            self.log.read()
        );
    }
}

fn fast() -> Timing {
    Timing {
        timeout: Duration::from_secs(2),
        backoff: [Duration::from_millis(5), Duration::from_millis(5)],
        startup_grace: TimeDelta::zero(),
        ..Timing::default()
    }
}

fn threshold_source() -> Source {
    let work = account("Work");
    Source::Alerts {
        account: work.clone(),
        events: vec![alert(&work, AlertKind::Threshold, 90)],
    }
}

fn exhausted_source() -> Source {
    let work = account("Work");
    Source::Alerts {
        account: work.clone(),
        events: vec![alert(&work, AlertKind::Exhausted, 100)],
    }
}

#[tokio::test]
async fn each_kind_posts_its_payload_to_the_mock() {
    let server = MockServer::start().await;
    for (route, content) in [
        ("/hook", "application/json"),
        ("/topic", "text/plain; charset=utf-8"),
        ("/slack", "application/json"),
        ("/discord", "application/json"),
    ] {
        Mock::given(method("POST"))
            .and(path(route))
            .and(header("content-type", content))
            .and(header("user-agent", crate::network::USER_AGENT))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
    }
    let fx = fixture();
    let url = |route: &str| format!("{}{route}", server.uri());
    fx.channel(ChannelKind::Webhook, &url("/hook"), Filters::default());
    fx.channel(ChannelKind::Ntfy, &url("/topic"), Filters::default());
    fx.channel(ChannelKind::Slack, &url("/slack"), Filters::default());
    fx.channel(ChannelKind::Discord, &url("/discord"), Filters::default());
    let dispatcher = fx.dispatcher(t0() - TimeDelta::hours(1), fast());
    let sent = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&sent);
    dispatcher.set_on_sent(Arc::new(move |sent| seen.lock().unwrap().push(sent)));
    assert_eq!(
        dispatcher.dispatch(&threshold_source(), true, "en-US", t0()),
        4
    );
    let entries = fx.settled(4).await;
    assert!(entries
        .iter()
        .all(|e| e.outcome == LogOutcome::Delivered && e.status == Some(200)));
    assert_eq!(sent.lock().unwrap().len(), 4);
    assert!(sent.lock().unwrap().iter().all(|s| s.kind == "threshold"));
    // RF3.3: the log never holds the body, the URL or the label.
    let text = std::fs::read_to_string(fx.log.path()).unwrap();
    assert!(!text.contains("127.0.0.1") && !text.contains("/hook") && !text.contains("Mock"));
    assert!(!text.contains("Claude") && !text.contains("Work"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(fx.log.path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[tokio::test]
async fn the_retry_matrix() {
    let server = MockServer::start().await;
    // 5xx twice then 2xx: delivered on the third attempt.
    Mock::given(path("/flaky"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(path("/flaky"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    // 5xx three times: failed after three attempts.
    Mock::given(path("/down"))
        .respond_with(ResponseTemplate::new(500))
        .expect(3)
        .mount(&server)
        .await;
    // 4xx: failed at once, never retried.
    Mock::given(path("/gone"))
        .respond_with(ResponseTemplate::new(403))
        .expect(1)
        .mount(&server)
        .await;
    // A redirect is never followed.
    Mock::given(path("/moved"))
        .respond_with(ResponseTemplate::new(302).insert_header("location", "https://example.com/"))
        .expect(1)
        .mount(&server)
        .await;
    let fx = fixture();
    let url = |route: &str| format!("{}{route}", server.uri());
    let flaky = fx.channel(ChannelKind::Webhook, &url("/flaky"), Filters::default());
    let down = fx.channel(ChannelKind::Webhook, &url("/down"), Filters::default());
    let gone = fx.channel(ChannelKind::Webhook, &url("/gone"), Filters::default());
    let moved = fx.channel(ChannelKind::Webhook, &url("/moved"), Filters::default());
    // Nothing listens here: a network error, retried, then failed.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        fx.channel(
            ChannelKind::Webhook,
            &format!("http://127.0.0.1:{port}/x"),
            Filters::default(),
        )
    };
    let dispatcher = fx.dispatcher(t0() - TimeDelta::hours(1), fast());
    assert_eq!(
        dispatcher.dispatch(&threshold_source(), true, "en-US", t0()),
        5
    );
    let entries = fx.settled(5).await;
    let of = |channel: &Channel| {
        entries
            .iter()
            .find(|e| e.channel == channel.id)
            .unwrap()
            .clone()
    };
    assert_eq!(
        (of(&flaky).outcome, of(&flaky).attempt),
        (LogOutcome::Delivered, 3)
    );
    assert_eq!(
        (of(&down).outcome, of(&down).status, of(&down).attempt),
        (LogOutcome::Failed, Some(500), 3)
    );
    assert_eq!(
        (of(&gone).outcome, of(&gone).status, of(&gone).attempt),
        (LogOutcome::Failed, Some(403), 1)
    );
    assert_eq!((of(&moved).status, of(&moved).attempt), (Some(302), 1));
    assert_eq!(
        (of(&closed).outcome, of(&closed).attempt),
        (LogOutcome::Failed, 3)
    );
    // Windows retries a refused connection until the timeout; elsewhere
    // it is refused at once. Either way it is a retried transport error.
    assert!(
        matches!(of(&closed).error.as_deref(), Some("network" | "timeout")),
        "{:?}",
        of(&closed).error
    );
    // RF5.1: the list shows the last outcome and the failure.
    let views = delivery::views(&fx.settings, Some(&fx.log), true, t0());
    let view = views.iter().find(|v| v.channel.id == gone.id).unwrap();
    assert_eq!(view.failing_status, Some(403));
    assert!(!view.failing_long);
}

#[tokio::test]
async fn the_hourly_cap_folds_the_rest_into_one_summary() {
    let server = MockServer::start().await;
    Mock::given(path("/hook"))
        .respond_with(ResponseTemplate::new(200))
        .expect(HOURLY_CAP as u64 + 1)
        .mount(&server)
        .await;
    let fx = fixture();
    fx.channel(
        ChannelKind::Webhook,
        &format!("{}/hook", server.uri()),
        Filters::default(),
    );
    let dispatcher = fx.dispatcher(t0() - TimeDelta::hours(1), fast());
    for minute in 0..HOURLY_CAP + 5 {
        let at = t0() + TimeDelta::minutes(minute as i64);
        dispatcher.dispatch(&threshold_source(), true, "en-US", at);
    }
    let entries = fx.settled(HOURLY_CAP + 5).await;
    let limited = entries
        .iter()
        .filter(|e| e.outcome == LogOutcome::RateLimited)
        .count();
    assert_eq!(limited, 5);
    // Within the hour nothing more goes; at its end one summary does.
    assert!(dispatcher
        .tick("en-US", t0() + TimeDelta::minutes(59))
        .is_empty());
    dispatcher.tick("en-US", t0() + TimeDelta::minutes(61));
    let entries = fx.settled(HOURLY_CAP + 6).await;
    let summary = entries.last().unwrap();
    assert_eq!(
        (summary.kind.as_str(), summary.outcome),
        ("rate_limit", LogOutcome::Delivered)
    );
    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests.last().unwrap().body).unwrap();
    assert_eq!(
        body["body"],
        "5 more notifications were not sent (rate limit)."
    );
}

#[tokio::test]
async fn the_first_minute_delivers_only_critical_notifications() {
    let server = MockServer::start().await;
    Mock::given(path("/hook"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let fx = fixture();
    fx.channel(
        ChannelKind::Webhook,
        &format!("{}/hook", server.uri()),
        Filters::default(),
    );
    let timing = Timing {
        startup_grace: TimeDelta::minutes(1),
        ..fast()
    };
    let dispatcher = fx.dispatcher(t0(), timing);
    let soon = t0() + TimeDelta::seconds(10);
    assert_eq!(
        dispatcher.dispatch(&threshold_source(), true, "en-US", soon),
        0
    );
    assert_eq!(
        dispatcher.dispatch(&exhausted_source(), true, "en-US", soon),
        1
    );
    let entries = fx.settled(2).await;
    assert_eq!(entries[0].outcome, LogOutcome::Held);
    assert_eq!(entries[1].kind, "exhausted");
}

/// RF3.1: quiet hours hold a channel delivery exactly as a system notice;
/// an exhaustion breaks through.
#[tokio::test]
async fn quiet_hours_hold_and_exhaustion_breaks_through() {
    use crate::quiet::{decide, Decision, QuietWindow, Subject};
    let server = MockServer::start().await;
    Mock::given(path("/hook"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let fx = fixture();
    fx.channel(
        ChannelKind::Webhook,
        &format!("{}/hook", server.uri()),
        Filters::default(),
    );
    let dispatcher = fx.dispatcher(t0() - TimeDelta::hours(1), fast());
    let window = Some(QuietWindow {
        ends_at: t0() + TimeDelta::hours(6),
    });
    let work = account("Work");
    for event in [
        alert(&work, AlertKind::Threshold, 90),
        alert(&work, AlertKind::Exhausted, 100),
    ] {
        if decide(Subject::Alert(&event), window) == Decision::Deliver {
            let source = Source::Alerts {
                account: work.clone(),
                events: vec![event],
            };
            dispatcher.dispatch(&source, true, "en-US", t0());
        }
    }
    let entries = fx.settled(1).await;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].kind, "exhausted");
}

/// RF3.1: the desktop and `--notify` share `notified.json`, so one crossing
/// reaches a channel once.
#[tokio::test]
async fn two_processes_deliver_one_crossing_once() {
    use crate::resets::{ResetEvent, ResetMetric, SharedLedger};
    let server = MockServer::start().await;
    Mock::given(path("/topic"))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    let fx = fixture();
    fx.channel(
        ChannelKind::Ntfy,
        &format!("{}/topic", server.uri()),
        Filters::default(),
    );
    let ledger = fx._dir.path().join("notified.json");
    let work = account("Work");
    let event = ResetEvent {
        account_index: 0,
        account_key: work.state_key(),
        metrics: vec![ResetMetric {
            label: "Session (5h)".into(),
            window: MetricWindow::Window5h,
            resets_at: t0(),
        }],
        resets_at: t0(),
        notify_at: t0(),
    };
    let desktop = fx.dispatcher(t0() - TimeDelta::hours(1), fast());
    let tui = fx.dispatcher(t0() - TimeDelta::hours(1), fast());
    for dispatcher in [&desktop, &tui] {
        let mut shared = SharedLedger::open(Some(ledger.clone()));
        shared.sync();
        for claimed in shared.claim_resets(vec![event.clone()], Vec::new(), t0()) {
            let source = Source::Reset {
                account: work.clone(),
                event: claimed,
            };
            dispatcher.dispatch(&source, true, "en-US", t0());
        }
    }
    let entries = fx.settled(1).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(fx.log.read().len(), 1, "{entries:?}");
}

#[tokio::test]
async fn a_channel_failing_for_a_day_gets_one_notice_and_stays_enabled() {
    let fx = fixture();
    let channel = fx.channel(
        ChannelKind::Webhook,
        "http://127.0.0.1:9/x",
        Filters::default(),
    );
    fx.log.append(&log::Entry {
        at: t0(),
        channel: channel.id.clone(),
        kind: "threshold".into(),
        outcome: LogOutcome::Failed,
        status: Some(403),
        attempt: 1,
        error: None,
    });
    let dispatcher = fx.dispatcher(t0(), fast());
    assert!(dispatcher
        .tick("en-US", t0() + TimeDelta::hours(23))
        .is_empty());
    let notices = dispatcher.tick("en-US", t0() + TimeDelta::hours(25));
    assert_eq!(notices.len(), 1);
    assert_eq!(
        notices[0].text().resolve("en-US"),
        "Channel Mock is failing (403)"
    );
    assert!(
        dispatcher
            .tick("en-US", t0() + TimeDelta::hours(26))
            .is_empty(),
        "once"
    );
    assert!(
        read(&fx.settings)[0].enabled,
        "never disabled automatically"
    );
    let views = delivery::views(
        &fx.settings,
        Some(&fx.log),
        true,
        t0() + TimeDelta::hours(25),
    );
    assert!(views[0].failing_long);
}

#[tokio::test]
async fn send_test_reports_the_status() {
    let server = MockServer::start().await;
    Mock::given(path("/hook"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;
    let fx = fixture();
    let channel = fx.channel(
        ChannelKind::Webhook,
        &format!("{}/hook", server.uri()),
        Filters::default(),
    );
    // The test passes the startup grace and the cap.
    let dispatcher = fx.dispatcher(
        t0(),
        Timing {
            startup_grace: TimeDelta::hours(1),
            ..fast()
        },
    );
    let outcome = dispatcher.test(&channel.id, "en-US", t0()).await.unwrap();
    assert_eq!(outcome, Outcome::Delivered { status: 201 });
    let requests = server.received_requests().await.unwrap();
    let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
    assert_eq!(
        (body["sample"].as_bool(), body["kind"].as_str()),
        (Some(true), Some("test"))
    );
    assert!(dispatcher.test("nope", "en-US", t0()).await.is_err());
}

#[test]
fn the_log_keeps_seven_days() {
    let dir = tempfile::tempdir().unwrap();
    let log = DeliveryLog::at(dir.path().join("log.jsonl"));
    for days in [9, 3, 0] {
        log.append(&log::Entry {
            at: t0() - TimeDelta::days(days),
            channel: "c".into(),
            kind: "threshold".into(),
            outcome: LogOutcome::Delivered,
            status: Some(200),
            attempt: 1,
            error: None,
        });
    }
    log.prune(t0());
    assert_eq!(log.read().len(), 2);
}
