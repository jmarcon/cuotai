use super::*;
use crate::config_file::ConfigIdentity;
use crate::model::{Billing, Metric, MetricKind, MetricWindow, Provider};
use crate::resets::Ledger;

fn at(day: u32, hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
}

fn daily(used: f64, limit: f64, day: u32) -> Metric {
    Metric {
        label: "Diário".into(),
        used: Some(used),
        limit: Some(limit),
        remaining: None,
        unit: "%".into(),
        kind: MetricKind::Quota,
        window: Some(MetricWindow::Daily),
        period: None,
        resets_at: Some(at(day + 1, 0)),
        billing: Billing::Included,
        scope: None,
        derived_from: None,
    }
}

fn account() -> Account {
    Account {
        name: "Work".into(),
        provider: Provider::Claude,
        ..Default::default()
    }
}

/// A fixture week (Mon 2026-10-19 … Sun 2026-10-25): use climbs every
/// morning, Thursday hits the limit, the limit moves on Friday.
fn fixture(dir: &std::path::Path) -> HistoryStore {
    let store = HistoryStore::new(
        dir.join("history.sqlite3"),
        &ConfigIdentity::Explicit("digest".into()),
    );
    let work = account();
    for (day, peak, limit) in [
        (19, 40.0, 100.0),
        (20, 55.0, 100.0),
        (21, 30.0, 100.0),
        (22, 100.0, 100.0),
        (23, 60.0, 150.0),
        (24, 10.0, 150.0),
    ] {
        for (hour, share) in [(9, 0.25), (10, 0.5), (11, 1.0)] {
            store
                .record(&work, at(day, hour), &[daily(peak * share, limit, day)])
                .unwrap();
        }
    }
    store
}

#[test]
fn the_weekly_digest_summarizes_a_fixture_week() {
    let dir = tempfile::tempdir().unwrap();
    let store = fixture(dir.path());
    let now = at(26, 9); // Monday 09:00 UTC
    let digest = weekly(
        &store,
        &[account()],
        &[None],
        DigestWeek::Last,
        now,
        &Utc,
        DaysOff::default(),
    )
    .unwrap();
    assert!(digest.recorded);
    assert_eq!(
        digest.first_day,
        NaiveDate::from_ymd_opt(2026, 10, 19).unwrap()
    );
    assert_eq!(
        digest.last_day,
        NaiveDate::from_ymd_opt(2026, 10, 25).unwrap()
    );
    let work = &digest.accounts[0];
    assert_eq!(work.days_at_limit, 1);
    assert_eq!(
        work.heaviest.unwrap().day,
        NaiveDate::from_ymd_opt(2026, 10, 22).unwrap()
    );
    assert_eq!(
        work.lightest.unwrap().day,
        NaiveDate::from_ymd_opt(2026, 10, 24).unwrap()
    );
    assert_eq!(work.busiest_hours, Some((10, 13)));
    assert_eq!(work.limit_changes.len(), 1);
    assert_eq!(
        (work.limit_changes[0].from, work.limit_changes[0].to),
        (100.0, 150.0)
    );

    let lines: Vec<String> = digest
        .short_lines("en")
        .iter()
        .map(|l| l.resolve("en"))
        .collect();
    assert!(lines.len() <= SHORT_LINES);
    assert_eq!(
        digest.title().resolve("en"),
        "Your week in Cuotai: 2026-10-19 – 2026-10-25"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("Heaviest day: Thursday 2026-10-22 (100%)")),
        "{lines:?}"
    );
    assert!(
        lines.contains(&"Busiest hours: 10:00–13:00".to_owned()),
        "{lines:?}"
    );
    assert!(lines.contains(&"1 limit change".to_owned()), "{lines:?}");
    assert_eq!(
        lines.last().unwrap(),
        "From the readings recorded on this computer."
    );
    assert!(
        lines.iter().all(|l| !l.contains("Work")),
        "private: no account name"
    );
    let markdown = digest.markdown("en");
    assert!(markdown.starts_with("# Your week in Cuotai"));
    assert!(markdown.contains("## Work · Claude"));
    assert!(markdown.contains("limit changed 100 → 150"));
    // Every catalog words every line.
    for locale in crate::text::catalog_locales() {
        for line in digest.short_lines(locale) {
            assert_ne!(
                line.resolve(locale),
                line.fallback,
                "{} in {locale}",
                line.id
            );
        }
    }
}

#[test]
fn an_empty_history_is_an_empty_digest() {
    let dir = tempfile::tempdir().unwrap();
    let store = HistoryStore::new(
        dir.path().join("h.sqlite3"),
        &ConfigIdentity::Explicit("x".into()),
    );
    let digest = weekly(
        &store,
        &[account()],
        &[None],
        DigestWeek::Current,
        at(26, 9),
        &Utc,
        DaysOff::default(),
    )
    .unwrap();
    assert!(!digest.recorded);
    assert_eq!(digest.short_lines("en")[0].id, "digest.line.empty");
}

#[test]
fn the_slot_is_due_for_48_hours_and_claimed_once_across_processes() {
    let prefs = DigestPrefs {
        enabled: true,
        weekday: 0,
        hour: 9,
    };
    assert_eq!(
        due_slot(&prefs, at(26, 8), &Utc).map(|s| s.day()),
        None,
        "before Monday 09:00 last week's slot is too old"
    );
    assert_eq!(due_slot(&prefs, at(26, 9), &Utc), Some(at(26, 9)));
    assert_eq!(
        due_slot(&prefs, at(28, 9), &Utc),
        Some(at(26, 9)),
        "48 h catch-up"
    );
    assert_eq!(due_slot(&prefs, at(28, 10), &Utc), None, "then dropped");
    assert_eq!(
        due_slot(&DigestPrefs::default(), at(26, 9), &Utc),
        None,
        "off by default"
    );

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("notified.json");
    let (mut desktop, mut terminal) = (Ledger::default(), Ledger::default());
    assert!(desktop.claim_weekly_digest(Some(&path), at(26, 9), at(26, 9)));
    assert!(!terminal.claim_weekly_digest(Some(&path), at(26, 9), at(26, 10)));
    assert!(!desktop.claim_weekly_digest(Some(&path), at(26, 9), at(27, 9)));
    // The next week's slot is new.
    assert!(terminal.claim_weekly_digest(
        Some(&path),
        at(26, 9) + TimeDelta::days(7),
        at(26, 9) + TimeDelta::days(7)
    ));
}
