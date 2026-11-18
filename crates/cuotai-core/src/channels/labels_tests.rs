//! PRD 0044 re-validation (V-04, V-08, V-09, V-10): the log kinds, the
//! error categories, the inline kind name and the filters in words read in
//! the person's language.

use super::delivery::Outcome;
use super::log::{error_label, kind_label, Entry, EntryView, LogOutcome};
use super::*;

/// Every kind slug the dispatcher writes to the log: the notice kinds,
/// the Send test, the hourly summary and the failing notice.
fn written_kinds() -> Vec<&'static str> {
    let mut slugs: Vec<&str> = NoticeKind::ALL.iter().map(|kind| kind.slug()).collect();
    slugs.extend(["test", "rate_limit", "failing"]);
    slugs
}

#[test]
fn every_written_kind_and_error_has_a_catalog_label() {
    for slug in written_kinds() {
        let label = kind_label(slug);
        assert_ne!(label.id, "metric.verbatim", "{slug}");
        assert_ne!(label.resolve("de"), slug, "{slug}");
    }
    for category in ["network", "timeout", "vault", "invalid_url"] {
        let label = error_label(category);
        assert!(
            label.id.starts_with("channels.error_category."),
            "{category}"
        );
        assert_ne!(label.resolve("de"), label.fallback, "{category}");
    }
    assert_eq!(
        kind_label("threshold").resolve("de"),
        NoticeKind::Threshold.label().resolve("de")
    );
    // A slug of a newer version is shown as written.
    assert_eq!(kind_label("future_kind").resolve("de"), "future_kind");
    assert_eq!(error_label("odd").resolve("de"), "odd");
}

#[test]
fn the_log_view_flattens_the_entry_and_adds_the_labels() {
    let entry = Entry {
        at: "2026-11-18T10:00:00Z".parse().unwrap(),
        channel: "ch_1".into(),
        kind: "rate_limit".into(),
        outcome: LogOutcome::Failed,
        status: None,
        attempt: 3,
        error: Some("timeout".into()),
    };
    let json = serde_json::to_value(EntryView::from(entry)).unwrap();
    assert_eq!(json["kind"], "rate_limit");
    assert_eq!(json["outcome"], "failed");
    assert_eq!(json["error"], "timeout");
    assert_eq!(json["kind_label"]["id"], "channels.log_kind.rate_limit");
    assert_eq!(json["error_label"]["id"], "channels.error_category.timeout");
    let failed = Outcome::Failed {
        status: None,
        error: Some("network"),
        attempt: 3,
    };
    assert_eq!(
        failed.error_label().unwrap().id,
        "channels.error_category.network"
    );
    assert_eq!(Outcome::Delivered { status: 200 }.error_label(), None);
}

#[test]
fn webhook_is_lowercase_inside_a_sentence() {
    assert_eq!(
        ChannelKind::Webhook.inline_name().resolve("en-US"),
        "webhook"
    );
    assert_eq!(ChannelKind::Webhook.name(), "Webhook");
    for kind in [ChannelKind::Ntfy, ChannelKind::Slack, ChannelKind::Discord] {
        assert_eq!(kind.inline_name().resolve("de"), kind.name());
    }
}

#[test]
fn the_default_kinds_read_all_kinds_and_the_digest_is_still_named() {
    let words = |filters: &Filters| filter_words(filters, true).kinds;
    let default = words(&Filters::default());
    assert_eq!(default.len(), 1);
    assert_eq!(default[0].resolve("en-US"), "All kinds");
    let mut every = Filters::default();
    every.apply("kinds=all").unwrap();
    let every = words(&every);
    assert_eq!(every.len(), 2);
    assert_eq!(every[1], NoticeKind::Digest.label());
    let mut three = Filters::default();
    three.apply("kinds=threshold,exhausted,digest").unwrap();
    assert_eq!(words(&three).len(), 3);
}

#[test]
fn the_kinds_are_joined_the_language_way() {
    let mut three = Filters::default();
    three.apply("kinds=threshold,exhausted").unwrap();
    let words = filter_words(&three, true);
    let ja = words.resolve("ja");
    let first = NoticeKind::Threshold.label().resolve("ja");
    let second = NoticeKind::Exhausted.label().resolve("ja");
    assert!(ja.starts_with(&format!("{first}、{second} · ")), "{ja}");
    assert!(words
        .resolve("en-US")
        .starts_with("Thresholds, Exhaustion · "));
}
