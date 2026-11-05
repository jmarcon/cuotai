//! PRD 0042 RF3 acceptance: block alignment (gap, straddle, DST), the
//! active block, the burn-rate gating, `block_hours = 1` and the provider
//! window attachment.

use super::*;
use crate::cost::Tokens;
use crate::model::{Metric, MetricKind};
use crate::Provider;
use chrono::{FixedOffset, NaiveDate, TimeZone};

fn at(h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, 25, h, m, 0).unwrap()
}

fn request(when: DateTime<Utc>, tokens: u64) -> Request {
    Request {
        source: SourceId::ClaudeCode,
        at: when,
        day: when.date_naive(),
        model: "claude-sonnet-4-5-20250929".into(),
        session: "s".into(),
        project: Some("p".into()),
        tokens: Tokens {
            input: tokens,
            ..Tokens::default()
        },
    }
}

fn five(_: SourceId) -> u8 {
    5
}

#[test]
fn a_block_starts_at_the_hour_of_the_first_request_and_a_gap_closes_it() {
    let prices = Prices::embedded();
    let requests = [
        request(at(8, 20), 1),
        request(at(12, 59), 1),
        // 13:00 is the end of the first block: a new one starts.
        request(at(13, 0), 1),
        // A gap of hours: the next request opens a third block.
        request(at(21, 45), 1),
    ];
    let found = blocks(&requests, &five, at(22, 0), &prices);
    let starts: Vec<_> = found.iter().rev().map(|b| b.start).collect();
    assert_eq!(starts, [at(8, 0), at(13, 0), at(21, 0)]);
    assert_eq!(found[2].estimate.requests, 2);
    assert!(found[0].active && !found[1].active && !found[2].active);
    assert_eq!(found[0].end, at(21, 0) + Duration::hours(5));
}

#[test]
fn a_block_straddling_midnight_and_a_clock_change_lasts_its_real_hours() {
    let prices = Prices::embedded();
    // 2026-10-25 is the end of summer time in Europe; blocks are in real
    // (UTC) hours, so the change does not stretch them.
    let late = Utc.with_ymd_and_hms(2026, 10, 24, 23, 30, 0).unwrap();
    let requests = [
        request(late, 1),
        request(at(3, 59), 1),
        request(at(4, 0), 1),
    ];
    let found = blocks(&requests, &five, at(4, 1), &prices);
    assert_eq!(found.len(), 2);
    assert_eq!(
        found[1].start,
        Utc.with_ymd_and_hms(2026, 10, 24, 23, 0, 0).unwrap()
    );
    assert_eq!(found[1].end - found[1].start, Duration::hours(5));
    assert_eq!(found[1].estimate.requests, 2);
    // Local day starts honour a fixed offset.
    let zone = DayZone::Fixed(FixedOffset::east_opt(3600).unwrap());
    let day = NaiveDate::from_ymd_opt(2026, 10, 25).unwrap();
    assert_eq!(
        zone.start_of(day),
        Utc.with_ymd_and_hms(2026, 10, 24, 23, 0, 0).unwrap()
    );
}

#[test]
fn one_hour_blocks_follow_the_setting() {
    let prices = Prices::embedded();
    let requests = [
        request(at(8, 10), 1),
        request(at(9, 5), 1),
        request(at(9, 50), 1),
    ];
    let found = blocks(&requests, &|_| 1, at(10, 0), &prices);
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|b| b.end - b.start == Duration::hours(1)));
    assert!(found.iter().all(|b| !b.active), "10:00 ends the 9:00 block");
}

#[test]
fn the_burn_rate_needs_three_requests_and_three_minutes() {
    let prices = Prices::embedded();
    let zone = DayZone::Fixed(FixedOffset::east_opt(0).unwrap());
    let few = [
        request(at(10, 0), 10),
        request(at(11, 55), 600),
        request(at(11, 58), 600),
    ];
    let block = &blocks(&few, &five, at(12, 0), &prices)[0];
    let summary = live(block, &few, at(12, 0), &prices, zone);
    assert_eq!(
        summary.burn_rate_tokens_per_min, None,
        "two recent requests"
    );
    assert!(summary.line.is_none() && summary.projection.is_none());

    // A block two minutes old: too short, whatever the count.
    let young = [
        request(at(12, 0), 1),
        request(at(12, 1), 1),
        request(at(12, 1), 1),
    ];
    let block = &blocks(&young, &five, at(12, 2), &prices)[0];
    assert_eq!(
        live(block, &young, at(12, 2), &prices, zone).burn_rate_tokens_per_min,
        None
    );

    // Three requests in a block 10% old: a rate, no projection yet.
    let early = [
        request(at(12, 5), 100),
        request(at(12, 21), 100),
        request(at(12, 24), 100),
        request(at(12, 28), 100),
    ];
    let block = &blocks(&early, &five, at(12, 30), &prices)[0];
    let summary = live(block, &early, at(12, 30), &prices, zone);
    assert_eq!(summary.burn_rate_tokens_per_min, Some(30.0));
    assert_eq!(summary.projected_block_cost, None, "under 20% elapsed");
    assert_eq!(summary.left_minutes, 270);
    let line = summary.line.unwrap().resolve("en-US");
    assert!(
        line.starts_with("This block ≈ ") && line.ends_with(" · 30 tok/min"),
        "{line}"
    );

    // Later in the block the projection appears, never below the cost so far.
    let later = [
        request(at(10, 5), 100),
        request(at(11, 52), 100),
        request(at(11, 55), 100),
        request(at(11, 59), 100),
    ];
    let block = &blocks(&later, &five, at(12, 0), &prices)[0];
    let summary = live(block, &later, at(12, 0), &prices, zone);
    let projected = summary.projected_block_cost.unwrap();
    assert!(projected >= block.estimate.total.unwrap());
    assert!(summary
        .projection
        .unwrap()
        .resolve("en-US")
        .ends_with("by 15:00"));
}

#[test]
fn an_unpriced_block_has_no_projection_and_never_shows_zero() {
    let prices = Prices::embedded();
    let zone = DayZone::Fixed(FixedOffset::east_opt(0).unwrap());
    let mut rows = vec![
        request(at(10, 5), 100),
        request(at(11, 52), 100),
        request(at(11, 55), 100),
        request(at(11, 59), 100),
    ];
    for row in &mut rows {
        row.model = "model-without-a-price".into();
    }
    let block = &blocks(&rows, &five, at(12, 0), &prices)[0];
    assert_eq!(block.estimate.total, None);
    let summary = live(block, &rows, at(12, 0), &prices, zone);
    assert_eq!(summary.projected_block_cost, None);
    let line = summary.line.unwrap().resolve("en-US");
    assert!(
        line.contains("unknown cost") && !line.contains("$0"),
        "{line}"
    );
}

fn report(provider: Provider, used: f64, resets_at: Option<DateTime<Utc>>) -> Report {
    Report {
        account: "Work".into(),
        account_id: None,
        provider,
        checked_at: at(11, 0),
        status: Status::Ready,
        metrics: vec![Metric {
            label: "5h".into(),
            used: Some(used),
            limit: Some(100.0),
            remaining: Some(100.0 - used),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Window5h),
            period: None,
            resets_at,
            billing: Default::default(),
            scope: None,
            derived_from: None,
        }],
        as_of: None,
        overage_hint: None,
        sub_reports: Vec::new(),
        diagnostic: None,
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
        breakdown: Vec::new(),
    }
}

#[test]
fn the_provider_window_is_attached_beside_the_active_block_only() {
    let prices = Prices::embedded();
    let requests = [request(at(3, 0), 1), request(at(10, 0), 1)];
    let mut found = blocks(&requests, &five, at(11, 0), &prices);
    let reports = [
        report(Provider::ChatGpt, 90.0, Some(at(14, 0))),
        report(Provider::Claude, 42.0, Some(at(14, 30))),
    ];
    attach_windows(&mut found, &reports);
    let window = found[0].provider_window.unwrap();
    assert_eq!(
        (window.resets_at, window.used_pct),
        (at(14, 30), Some(42.0))
    );
    assert!(found[1].provider_window.is_none(), "closed blocks get none");
    // The local block keeps its own end: shown beside, never merged.
    assert_eq!(found[0].end, at(15, 0));
    let zone = DayZone::Fixed(FixedOffset::east_opt(0).unwrap());
    assert_eq!(
        provider_window_text(&window, zone).resolve("en-US"),
        "Provider window: 42% used, resets at 14:30"
    );
    // No account of the source: nothing attached.
    let mut alone = blocks(&requests, &five, at(11, 0), &prices);
    attach_windows(&mut alone, &[report(Provider::Claude, 1.0, None)]);
    assert!(alone[0].provider_window.is_none());
}

#[test]
fn projects_group_missing_folders_under_no_project() {
    let prices = Prices::embedded();
    let mut orphan = request(at(9, 0), 5);
    orphan.project = None;
    let rows = [request(at(8, 0), 1), orphan];
    let found = projects(&rows, &[], &prices);
    assert_eq!(found.len(), 2);
    let none = found.iter().find(|p| p.project.is_none()).unwrap();
    assert_eq!(none.name.resolve("en-US"), "No project");
    assert_eq!(Breakdown::from_slug("block"), Some(Breakdown::Block));
    assert_eq!(Breakdown::from_slug("week"), None);
    assert_eq!(duration_text(158).resolve("en-US"), "2h 38m");
}
