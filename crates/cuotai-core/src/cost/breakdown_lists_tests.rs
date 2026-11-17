//! PRD 0042 re-validation (K7): end dates across days (L-01), a labelled
//! session's time (L-03), the session's project name (M-06), the active
//! block first (R-03), the provider window of a single account (R-04) and
//! the code-help lines.

use super::*;
use crate::cost::Tokens;
use crate::model::{Metric, MetricKind};
use crate::Provider;
use chrono::{FixedOffset, TimeZone};

fn utc(d: u32, h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, d, h, m, 0).unwrap()
}

fn request(source: SourceId, when: DateTime<Utc>, session: &str) -> Request {
    Request {
        source,
        at: when,
        day: when.date_naive(),
        model: "claude-sonnet-4-5-20250929".into(),
        session: session.into(),
        project: Some("0123456789abcdef".into()),
        tokens: Tokens {
            input: 100,
            ..Tokens::default()
        },
    }
}

fn utc_zone() -> DayZone {
    DayZone::Fixed(FixedOffset::east_opt(0).unwrap())
}

#[test]
fn a_range_across_midnight_or_of_24_hours_shows_its_end_date() {
    let prices = Prices::embedded();
    let zone = utc_zone();
    let late = [request(SourceId::ClaudeCode, utc(14, 22, 30), "s")];
    let block = &blocks(&late, &|_| 5, utc(15, 9, 0), &prices)[0];
    assert_eq!(block.window_text(zone), "2026-10-14 22:00–2026-10-15 03:00");
    let day = &blocks(&late, &|_| 24, utc(16, 9, 0), &prices)[0];
    assert_eq!(day.window_text(zone), "2026-10-14 22:00–2026-10-15 22:00");
    let same = &blocks(&late, &|_| 1, utc(16, 9, 0), &prices)[0];
    assert_eq!(same.window_text(zone), "2026-10-14 22:00–23:00");
    // The local day decides: at UTC+3 the 5-hour block is one day.
    let east = DayZone::Fixed(FixedOffset::east_opt(-3 * 3600).unwrap());
    assert_eq!(block.window_text(east), "2026-10-14 19:00–2026-10-15 00:00");
    let early = DayZone::Fixed(FixedOffset::east_opt(3 * 3600).unwrap());
    assert_eq!(block.window_text(early), "2026-10-15 01:00–06:00");

    let rows = [
        request(SourceId::ClaudeCode, utc(14, 22, 30), "s"),
        request(SourceId::ClaudeCode, utc(15, 1, 30), "s"),
    ];
    let found = sessions(&rows, &BTreeMap::new(), &[], &prices, zone);
    assert_eq!(
        found[0].name.resolve("en-US"),
        "Session · 2026-10-14 22:30–2026-10-15 01:30 · 3h 0m"
    );
}

#[test]
fn a_labelled_session_keeps_its_time_and_names_its_project() {
    let prices = Prices::embedded();
    let mut orphan = request(SourceId::ClaudeCode, utc(20, 11, 0), "orphan");
    orphan.project = None;
    let rows = [
        request(SourceId::ClaudeCode, utc(20, 9, 2), "work"),
        request(SourceId::ClaudeCode, utc(20, 10, 40), "work"),
        orphan,
    ];
    let labels = BTreeMap::from([("work".to_owned(), "Refactor of the parser".to_owned())]);
    let found = sessions(&rows, &labels, &[], &prices, utc_zone());
    let work = found.iter().find(|s| s.session == "work").unwrap();
    assert_eq!(
        work.name.resolve("en-US"),
        "Refactor of the parser · 2026-10-20 09:02–10:40 · 1h 38m"
    );
    assert_eq!(work.project_name.resolve("en-US"), "Project 01234567");
    let orphan = found.iter().find(|s| s.session == "orphan").unwrap();
    assert_eq!(orphan.project_name.resolve("en-US"), "No project");
    let json = serde_json::to_value(work).unwrap();
    assert_eq!(
        json["project_name"]["fallback"], "Project 01234567",
        "{json}"
    );
}

#[test]
fn the_active_block_comes_first_whatever_another_source_did_later() {
    let prices = Prices::embedded();
    // Claude Code's active block started at 08:00; Codex CLI's closed
    // 1-hour block started later, at 10:00.
    let rows = [
        request(SourceId::ClaudeCode, utc(20, 8, 10), "a"),
        request(SourceId::CodexCli, utc(20, 10, 5), "b"),
    ];
    let hours = |s: SourceId| if s == SourceId::CodexCli { 1 } else { 5 };
    let found = blocks(&rows, &hours, utc(20, 11, 30), &prices);
    assert_eq!(found.len(), 2);
    assert!(found[0].active, "{found:?}");
    assert_eq!(found[0].source, SourceId::ClaudeCode);
    assert_eq!(found[1].source, SourceId::CodexCli);
}

fn report(account: &str, provider: Provider, window: bool) -> Report {
    Report {
        account: account.into(),
        account_id: None,
        provider,
        checked_at: utc(20, 11, 0),
        status: Status::Ready,
        metrics: vec![Metric {
            label: "5h".into(),
            used: Some(30.0),
            limit: Some(100.0),
            remaining: Some(70.0),
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: window.then_some(MetricWindow::Window5h),
            period: None,
            resets_at: Some(utc(20, 14, 0)),
            billing: Default::default(),
            scope: None,
            derived_from: None,
            unlimited: false,
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
fn the_provider_window_needs_exactly_one_account_with_one() {
    let one = [
        report("Work", Provider::Claude, true),
        report("Weekly only", Provider::Claude, false),
    ];
    let window = ProviderWindow::from_reports(&one, SourceId::ClaudeCode).unwrap();
    assert_eq!(window.used_pct, Some(30.0));
    let two = [
        report("Work", Provider::Claude, true),
        report("Personal", Provider::Claude, true),
    ];
    assert_eq!(
        ProviderWindow::from_reports(&two, SourceId::ClaudeCode),
        None,
        "two accounts: the logs belong to either"
    );
    let mut down = report("Personal", Provider::Claude, true);
    down.status = Status::NeedsConfiguration;
    let ready_one = [report("Work", Provider::Claude, true), down];
    assert!(ProviderWindow::from_reports(&ready_one, SourceId::ClaudeCode).is_some());
}

#[test]
fn the_code_help_says_it_is_not_the_tools_id() {
    let session = code_help_text(Breakdown::Session).unwrap().resolve("en-US");
    assert!(session.contains("--cost-label session"), "{session}");
    assert!(session.contains("not the tool's session id"), "{session}");
    let project = code_help_text(Breakdown::Project).unwrap().resolve("en-US");
    assert!(project.contains("--cost-label project"), "{project}");
    assert!(code_help_text(Breakdown::Block).is_none());
    assert_eq!(
        live_source_off_text(SourceId::GeminiCli).resolve("en-US"),
        "Gemini CLI is off; cuotai --cost-enable gemini_cli turns it on."
    );
}
