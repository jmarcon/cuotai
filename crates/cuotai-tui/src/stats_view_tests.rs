use super::*;
use chrono::{FixedOffset, TimeZone};
use cuotai_core::{
    config_file::ConfigIdentity,
    events::{Event, EventKind, MetricRef, Values},
    Metric, MetricKind, MetricWindow, Status,
};
use ratatui::{backend::TestBackend, Terminal};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

const CONFIG: &str = r#"
[[accounts]]
name = "alpha"
provider = "groq"
credential_env = "GROQ_KEY_ALPHA"
[[accounts]]
name = "beta"
provider = "groq"
credential_env = "GROQ_KEY_BETA"
"#;

/// A private directory removed on drop.
struct Dir(PathBuf);

impl Dir {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "cuotai-stats-view-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn store(&self, name: &str) -> HistoryStore {
        HistoryStore::new(
            self.0.join("history.sqlite3"),
            &ConfigIdentity::Explicit(name.into()),
        )
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn at(day: u32, hour: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, day, hour, 0, 0).unwrap()
}

fn quota(used: f64, resets: DateTime<Utc>) -> Metric {
    Metric {
        label: "5h".into(),
        used: Some(used),
        limit: Some(100.0),
        remaining: Some(100.0 - used),
        unit: "%".into(),
        kind: MetricKind::Quota,
        window: Some(MetricWindow::Window5h),
        period: None,
        resets_at: Some(resets),
        billing: Default::default(),
        scope: None,
        derived_from: None,
    }
}

fn report(config: &Config, used: f64, resets: DateTime<Utc>) -> Report {
    Report {
        as_of: Default::default(),
        overage_hint: Default::default(),
        sub_reports: Vec::new(),
        diagnostic: Default::default(),
        account_id: Some(config.accounts[0].id()),
        account: "alpha".into(),
        provider: config.accounts[0].provider,
        checked_at: at(20, 12),
        status: Status::Ready,
        metrics: vec![quota(used, resets)],
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
        breakdown: Vec::new(),
    }
}

struct Fixture {
    _dir: Dir,
    store: HistoryStore,
    config: Config,
}

/// Three days of readings of `alpha`, the middle one at the limit, a gap on
/// the 18th; one exhaustion in the journal.
fn fixture() -> Fixture {
    let dir = Dir::new();
    let store = dir.store("stats-view");
    let config = Config::from_toml(CONFIG).unwrap();
    let alpha = &config.accounts[0];
    for (day, used) in [(17, 40.0), (19, 100.0), (20, 60.0)] {
        let resets = at(day, 0) + TimeDelta::days(1);
        store
            .record(alpha, at(day, 10), &[quota(used, resets)])
            .unwrap();
    }
    let event = Event {
        parent: Default::default(),
        schema: cuotai_core::events::SCHEMA,
        event: EventKind::QuotaExhausted,
        occurred_at: at(19, 10),
        account_key: alpha.key(),
        account_id: alpha.state_key(),
        account_name: alpha.name.clone(),
        provider: Some(alpha.provider),
        accounts: Vec::new(),
        metric: Some(MetricRef::new("5h", None)),
        current: Values::default(),
        previous: None,
        sample: false,
        row: None,
    };
    store.journal_record(&[event]).unwrap();
    Fixture {
        _dir: dir,
        store,
        config,
    }
}

fn summary(fixture: &Fixture, range: StatsRange) -> StatsSummary {
    stats::summarize(
        &fixture.store,
        &fixture.config.accounts,
        range,
        at(20, 12),
        &FixedOffset::east_opt(0).unwrap(),
    )
    .unwrap()
}

#[test]
fn stats_text_has_the_figures_gaps_and_the_base() {
    let fixture = fixture();
    let text = stats_text(&summary(&fixture, StatsRange::Days7), "pt-BR");
    assert!(text.contains("últimos 7 dias"), "{text}");
    assert!(text.contains("Limites atingidos: 1"), "{text}");
    assert!(
        text.contains("Pico médio do limite mais apertado: 67% (3 dias com dados)"),
        "{text}"
    );
    assert!(text.contains("Gasto extra: —"), "{text}");
    // Today is labelled, the 100% day is full, a day without data is a gap
    // with a word, never an empty bar.
    // V-03: the core's day text (the weekday within six days).
    assert!(text.contains("ter. hoje"), "{text}");
    assert!(
        text.contains(&format!("{} 100%", "█".repeat(BAR_CELLS))),
        "{text}"
    );
    let gap = text.lines().find(|l| l.starts_with("dom.")).unwrap();
    // Beyond six days the date, in each language's order.
    let thirty = summary(&fixture, StatsRange::Days30);
    assert!(stats_text(&thirty, "de").contains("\n13.10. "));
    assert!(stats_text(&thirty, "en-US").contains("\n10/13 "));
    assert!(stats_text(&thirty, "pt-BR").contains("\n13/10 "));
    assert!(gap.ends_with("sem dados"), "{gap}");
    let kept = summary(&fixture, StatsRange::Days7)
        .history_kept()
        .resolve("pt-BR");
    assert!(text.contains(&kept), "{text}");
    // beta has no data: a dash, not a zero.
    let beta = text.lines().find(|l| l.starts_with("beta")).unwrap();
    assert!(beta.contains("média    —"), "{beta}");
    // PRD 0034 RF2: the same figures in English.
    let english = stats_text(&summary(&fixture, StatsRange::Days7), "en-US");
    assert!(english.contains("Statistics · last 7 days"), "{english}");
    assert!(english.contains("Limits reached: 1"), "{english}");
    assert!(
        english.contains("History kept 90 days on this device"),
        "{english}"
    );
    assert!(
        cuotai_core::text::portuguese_markers(&english).is_empty(),
        "{english}"
    );
}

#[test]
fn not_enough_history_says_so_instead_of_showing_zeros() {
    let dir = Dir::new();
    let store = dir.store("empty");
    let config = Config::from_toml(CONFIG).unwrap();
    let summary = stats::summarize(
        &store,
        &config.accounts,
        StatsRange::Days30,
        at(20, 12),
        &FixedOffset::east_opt(0).unwrap(),
    )
    .unwrap();
    let text = stats_text(&summary, "pt-BR");
    assert!(text.contains("— Histórico insuficiente"), "{text}");
    assert!(
        text.contains("Pico médio do limite mais apertado: —"),
        "{text}"
    );
    // R-01: the hits are unknown too, never 0.
    assert!(text.contains("Limites atingidos: —"), "{text}");
    assert!(!dir.0.join("history.sqlite3").exists());
}

#[test]
fn timeline_lists_returns_in_order_then_later_and_unknown() {
    let config = Config::from_toml(CONFIG).unwrap();
    let now = at(20, 12);
    let reports = vec![
        Some(report(&config, 100.0, now + TimeDelta::minutes(95))),
        None,
    ];
    let timeline = timeline_of(&config, &reports, TimelineRange::Hours24, now);
    let text = timeline_text(&timeline, "pt-BR");
    assert!(text.contains("próximas 24 h"), "{text}");
    assert!(text.contains("em 1h 35m · alpha · 5h · esgotada"), "{text}");
    // Beyond the range: listed under "Depois".
    let later = vec![Some(report(&config, 10.0, now + TimeDelta::days(3))), None];
    let text = timeline_text(
        &timeline_of(&config, &later, TimelineRange::Hours24, now),
        "pt-BR",
    );
    assert!(text.contains("Nenhum retorno neste período."), "{text}");
    assert!(text.contains("Depois"), "{text}");
    assert!(text.contains("90% restante"), "{text}");
    // No known return.
    let mut unknown = report(&config, 10.0, now);
    unknown.metrics[0].resets_at = None;
    let text = timeline_text(
        &timeline_of(&config, &[Some(unknown), None], TimelineRange::Days7, now),
        "pt-BR",
    );
    assert!(text.contains("Retorno desconhecido"), "{text}");
}

#[test]
fn events_text_and_json_follow_the_journal() {
    let fixture = fixture();
    let entries = fixture
        .store
        .journal_read(.., &fixture.config.accounts, 20)
        .unwrap();
    let now = Utc::now();
    let text = events_text(&entries, now, "en-US");
    assert!(text.contains("alpha · 5h: quota exhausted"), "{text}");
    let json = events_json(&entries, "en-US");
    assert_eq!(json["events"][0]["event"], "quota_exhausted");
    assert_eq!(json["events"][0]["account"], "alpha");
    // V-10/S-03: the projection, the amount and the skipped count travel.
    assert!(json["events"][0]
        .as_object()
        .unwrap()
        .contains_key("projected_at"));
    assert!(json["events"][0].as_object().unwrap().contains_key("spent"));
    assert!(json["events"][0]
        .as_object()
        .unwrap()
        .contains_key("currency"));
    assert_eq!(json["skipped"], 0);
    assert_eq!(
        events_text(&JournalRead::default(), now, "en-US")
            .lines()
            .count(),
        2
    );
    // An account left the configuration.
    let entries = fixture.store.journal_read(.., &[], 20).unwrap();
    assert!(events_text(&entries, now, "en-US").contains("Removed account"));
}

/// V-09/S-03: rows group under the core's day headings, and unreadable
/// entries are counted in the reader's plural.
#[test]
fn events_group_under_day_headings_and_count_unreadable_entries() {
    let fixture = fixture();
    let mut read = fixture
        .store
        .journal_read(.., &fixture.config.accounts, 20)
        .unwrap();
    let entry = read.entries[0].clone();
    let now = entry.occurred_at;
    let mut yesterday = entry.clone();
    yesterday.occurred_at -= TimeDelta::days(1);
    let mut old = entry.clone();
    old.occurred_at -= TimeDelta::days(9);
    read.entries = vec![entry, yesterday, old];
    read.skipped = 2;
    let text = events_text(&read, now, "en-US");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[1], "Today", "{text}");
    assert!(lines[2].starts_with("  "), "{text}");
    assert_eq!(lines[3], "Yesterday", "{text}");
    assert!(
        lines.iter().any(|l| l.contains('/') && !l.contains('·')),
        "a date heading beyond six days: {text}"
    );
    assert!(
        text.ends_with("2 journal entries could not be read.\n"),
        "{text}"
    );
    let ru = events_text(&read, now, "ru");
    assert!(ru.contains("Вчера"), "{ru}");
    assert!(
        ru.contains("Не удалось прочитать 2 записи журнала."),
        "{ru}"
    );
    assert_eq!(events_json(&read, "en-US")["skipped"], 2);
}

/// V-13/R-02: plural forms and the language's money format.
#[test]
fn account_lines_take_plurals_and_amounts_the_language_format() {
    let fixture = fixture();
    let mut summary = summary(&fixture, StatsRange::Days7);
    summary.unknown_limits = 1;
    summary.extra_spent = Some(vec![cuotai_core::stats::ExtraSpent {
        unit: "USD".into(),
        amount: 6.25,
    }]);
    let english = stats_text(&summary, "en-US");
    assert!(english.contains("· 1 limit reached ·"), "{english}");
    assert!(
        english.contains("1 limit without a known ceiling was left out"),
        "{english}"
    );
    assert!(english.contains("Extra spending: $6.25"), "{english}");
    assert!(!english.contains("(s)"), "{english}");
    let german = stats_text(&summary, "de");
    assert!(german.contains("6,25"), "{german}");
    assert!(!german.contains("6.25 USD"), "{german}");
    summary.unknown_limits = 3;
    assert!(stats_text(&summary, "pt-BR").contains("3 limites sem teto conhecido ficaram"));
}

#[test]
fn range_arguments_are_validated() {
    assert_eq!(stats_range(None), Ok(StatsRange::Days7));
    assert_eq!(stats_range(Some(30)), Ok(StatsRange::Days30));
    assert!(stats_range(Some(14)).is_err());
    assert_eq!(timeline_range(Some("7d")), Ok(TimelineRange::Days7));
    assert!(timeline_range(Some("48h")).is_err());
    assert_eq!(event_days(None), Ok(7));
    assert_eq!(event_days(Some(365)), Ok(365));
    assert!(event_days(Some(0)).is_err() && event_days(Some(366)).is_err());
}

fn render(view: &mut StatsView, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| view.draw(frame, "pt-BR")).unwrap();
    let buffer = terminal.backend().buffer().clone();
    buffer
        .content()
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|c| c.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_screen_scrolls_toggles_ranges_and_closes() {
    let fixture = fixture();
    let mut view = StatsView::open(fixture.store.clone(), &fixture.config, vec![None, None]);
    let first = render(&mut view, 100, 30);
    assert!(first.contains("ESTATÍSTICAS"), "{first}");
    assert!(first.contains("Limites atingidos"), "{first}");
    assert!(
        first.contains("d 30 dias") && first.contains("t 7 dias"),
        "{first}"
    );

    // `d` switches to 30 days, `t` the timeline range.
    assert_eq!(view.key(KeyCode::Char('d')), Reply::Stay);
    let thirty = render(&mut view, 100, 80);
    assert!(thirty.contains("últimos 30 dias"), "{thirty}");
    assert!(thirty.contains("d 7 dias"), "{thirty}");
    view.key(KeyCode::Char('t'));
    assert!(render(&mut view, 100, 80).contains("próximas 7 dias"));

    // A short window scrolls to the end and clamps; Home returns.
    view.key(KeyCode::End);
    let end = render(&mut view, 100, 12);
    assert!(end.contains("Eventos recentes"), "{end}");
    view.key(KeyCode::Home);
    assert!(render(&mut view, 100, 12).contains("Estatísticas"));

    for code in [KeyCode::Esc, KeyCode::Char('q'), KeyCode::Char('S')] {
        assert_eq!(view.key(code), Reply::Close);
    }
    assert_eq!(
        view.mouse(MouseEventKind::Down(MouseButton::Right), 0, 0),
        Reply::Close
    );
    // The footer actions are clickable.
    render(&mut view, 100, 30);
    let back = view.footer[3].0;
    assert_eq!(
        view.mouse(MouseEventKind::Down(MouseButton::Left), back.x, back.y),
        Reply::Close
    );
    // A narrow terminal does not panic.
    render(&mut view, 20, 6);
}
