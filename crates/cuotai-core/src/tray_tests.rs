//! PRD 0025 RF1 acceptance: table tests per style.

use super::*;
use chrono::TimeZone;

fn now() -> DateTime<Utc> {
    Utc.timestamp_opt(1_800_000_000, 0).unwrap()
}

fn later(minutes: i64) -> DateTime<Utc> {
    now() + chrono::TimeDelta::minutes(minutes)
}

fn hhmm(t: DateTime<Utc>) -> String {
    t.format("%H:%M").to_string()
}

fn acc(key: &str, provider: &str, name: &str, remaining: Option<f64>) -> AccountView {
    AccountView {
        key: key.into(),
        provider: provider.to_lowercase(),
        provider_label: provider.into(),
        name: name.into(),
        remaining,
        ..AccountView::default()
    }
}

fn run(accounts: &[AccountView], style: TrayStyle, selection: &TraySelection) -> TraySummary {
    summary(accounts, style, selection, now(), hhmm)
}

fn auto(accounts: &[AccountView], style: TrayStyle) -> TraySummary {
    run(accounts, style, &TraySelection::Auto)
}

#[test]
fn tightest_is_the_default_and_picks_the_lowest_fresh_share() {
    assert_eq!(TrayStyle::default(), TrayStyle::TightestPercent);
    let accounts = [
        acc("a", "Claude", "Work", Some(0.22)),
        acc("b", "Gemini", "Home", Some(0.64)),
    ];
    let s = auto(&accounts, TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("22% · Work"));
    assert_eq!(s.items.len(), 2);
    assert_eq!(s.items[0].level, Some(Level::Bad));
    assert_eq!(s.items[1].level, Some(Level::Warn));
    assert_eq!(s.badge, Badge::None);
}

#[test]
fn stale_readings_are_marked_and_left_out_of_tightest() {
    let mut old = acc("a", "Claude", "Old", Some(0.05));
    old.stale = true;
    let accounts = [old.clone(), acc("b", "Gemini", "Home", Some(0.64))];
    let s = auto(&accounts, TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("64% · Home"));
    // Every reading stale: the stale one counts, with its mark.
    let s = auto(&[old], TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("5%· · Old"));
    assert_eq!(
        s.accessible_label.resolve("en-US"),
        "Old 5% left (old reading)"
    );
}

#[test]
fn unmeasured_shows_a_dash_never_zero() {
    let accounts = [acc("a", "Claude", "Work", None)];
    let s = auto(&accounts, TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("—"));
    let s = auto(&accounts, TrayStyle::LogosPercent);
    assert_eq!(s.title.as_deref(), Some("C —"));
    assert_eq!(s.items[0].level, None);
    assert_eq!(s.accessible_label.resolve("en-US"), "Work: no data");
}

#[test]
fn exhausted_shows_the_return_time() {
    let mut out = acc("a", "Claude", "Work", Some(0.0));
    out.exhausted = true;
    out.resets_at = Some(later(90));
    let s = auto(
        &[out.clone(), acc("b", "Gemini", "G", Some(0.5))],
        TrayStyle::TightestPercent,
    );
    let at = hhmm(later(90));
    assert_eq!(s.title, Some(format!("0% → {at} · Work")));
    assert_eq!(s.items[0].level, Some(Level::Bad));
    assert!(s
        .accessible_label
        .resolve("en-US")
        .starts_with(&format!("Work exhausted, returns {at}")));
    // A return time already past is dropped.
    out.resets_at = Some(later(-5));
    assert_eq!(
        auto(&[out], TrayStyle::TightestPercent).title.as_deref(),
        Some("0% · Work")
    );
}

#[test]
fn outlook_chip_replaces_the_figure_when_shorter() {
    let mut a = acc("a", "Claude", "Personal account", Some(0.4));
    a.runs_out_at = Some(later(30));
    let s = auto(&[a], TrayStyle::TightestPercent);
    assert_eq!(s.title, Some(format!("↑ ≈{}", hhmm(later(30)))));
}

#[test]
fn logos_percent_lists_each_item_with_its_initial() {
    let accounts = [
        acc("a", "Claude", "W", Some(0.22)),
        acc("b", "Gemini", "H", Some(0.64)),
    ];
    let s = auto(&accounts, TrayStyle::LogosPercent);
    assert_eq!(s.title.as_deref(), Some("C 22% · G 64%"));
}

#[test]
fn width_budget_drops_items_with_a_count() {
    let accounts: Vec<AccountView> = (0..6)
        .map(|i| {
            acc(
                &format!("k{i}"),
                "Claude",
                "x",
                Some(0.5 + i as f64 / 100.0),
            )
        })
        .collect();
    let s = auto(&accounts, TrayStyle::LogosPercent);
    let title = s.title.unwrap();
    assert!(title.chars().count() <= TITLE_BUDGET, "{title}");
    assert!(title.ends_with("+3"), "{title}");
    assert_eq!(s.hidden, 3);
    // A long account name never pushes the figure out.
    let long = [acc(
        "a",
        "Claude",
        "A very long account name here",
        Some(0.3),
    )];
    let title = auto(&long, TrayStyle::TightestPercent).title.unwrap();
    assert!(title.chars().count() <= TITLE_BUDGET);
    assert!(title.starts_with("30%"));
}

#[test]
fn auto_caps_meters_and_logos() {
    let accounts: Vec<AccountView> = (0..8)
        .map(|i| acc(&format!("k{i}"), "Claude", "x", Some(0.9)))
        .collect();
    let s = auto(&accounts, TrayStyle::Meters);
    assert_eq!(s.items.len(), AUTO_METERS);
    assert_eq!(s.hidden, 4);
    assert_eq!(s.title, None);
    assert_eq!(
        auto(&accounts, TrayStyle::LogosPercent).items.len(),
        AUTO_LOGOS
    );
    assert_eq!(auto(&accounts, TrayStyle::IconOnly).title, None);
}

#[test]
fn custom_selection_keeps_its_own_order() {
    let accounts = [
        acc("a", "Claude", "A", Some(0.9)),
        acc("b", "Gemini", "B", Some(0.8)),
        acc("c", "Cursor", "C", Some(0.7)),
    ];
    let sel = TraySelection::Custom(vec!["c".into(), "gone".into(), "a".into()]);
    let s = run(&accounts, TrayStyle::LogosPercent, &sel);
    let keys: Vec<&str> = s.items.iter().map(|i| i.key.as_str()).collect();
    assert_eq!(keys, ["c", "a"]);
    assert_eq!(s.title.as_deref(), Some("C 70% · C 90%"));
}

#[test]
fn pruning_drops_removed_ids_and_keeps_auto() {
    let sel = TraySelection::Custom(vec!["a".into(), "x".into(), "a".into(), "b".into()]);
    let existing = vec!["a".to_owned(), "b".to_owned()];
    assert_eq!(
        sel.pruned(&existing),
        TraySelection::Custom(vec!["a".into(), "b".into()])
    );
    assert_eq!(TraySelection::Auto.pruned(&existing), TraySelection::Auto);
}

#[test]
fn alerts_outages_and_extra_usage_reach_title_badge_and_label() {
    let mut a = acc("a", "Claude", "Work", Some(0.2));
    a.alert = true;
    a.on_extra = true;
    let s = auto(std::slice::from_ref(&a), TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("! 20% · Work"));
    assert_eq!(s.badge, Badge::Alert);
    assert!(s.items[0].on_extra);
    assert_eq!(
        s.accessible_label.resolve("en-US"),
        "Work 20% left, alert active, using extra usage"
    );
    a.incident = true;
    let s = auto(&[a], TrayStyle::IconOnly);
    assert_eq!(s.badge, Badge::Outage);
    assert!(s
        .accessible_label
        .resolve("en-US")
        .ends_with("; provider incident"));
}

#[test]
fn empty_selection_has_no_title() {
    let s = auto(&[], TrayStyle::TightestPercent);
    assert_eq!(s.title, None);
    assert!(s.items.is_empty());
    assert_eq!(
        s.accessible_label.resolve("en-US"),
        "No accounts in the menu bar"
    );
    assert_eq!(
        s.tooltip.resolve("en-US"),
        "Cuotai — No accounts in the menu bar"
    );
}

#[test]
fn rounding_never_lies() {
    assert_eq!(percent(0.004), "<1%");
    assert_eq!(percent(0.0), "0%");
    assert_eq!(percent(0.998), "99%");
    assert_eq!(percent(1.0), "100%");
}

#[test]
fn labels_render_in_the_four_languages() {
    let mut a = acc("a", "Claude", "Work", Some(0.22));
    a.resets_at = Some(later(60));
    let b = acc("b", "Gemini", "Home", Some(0.64));
    let s = auto(&[a, b], TrayStyle::TightestPercent);
    let at = hhmm(later(60));
    assert_eq!(
        s.accessible_label.resolve("en-US"),
        format!("Work 22% left, resets {at}; Home 64% left")
    );
    for locale in ["pt-BR", "es", "it"] {
        let text = s.accessible_label.resolve(locale);
        assert!(
            text.contains("22%") && text.contains("Home"),
            "{locale}: {text}"
        );
        assert!(!text.contains('{'), "{locale}: {text}");
    }
    // The title holds figures and names only, so the budget is the same in every language.
    assert!(s.title.unwrap().chars().count() <= TITLE_BUDGET);
}

#[test]
fn style_slugs_round_trip() {
    for style in TrayStyle::ALL {
        assert_eq!(TrayStyle::parse(style.slug()), Some(style));
    }
    assert_eq!(TrayStyle::parse("nope"), None);
}

/// PRD 0023 via PRD 0025: an account paying for extra usage reads as
/// `on_extra` from its report alone.
#[test]
fn a_report_with_active_extra_usage_is_on_extra() {
    use crate::{Account, Billing, Metric, MetricKind, Provider, Report, Status};
    let account = Account {
        name: "Work".into(),
        provider: Provider::Claude,
        ..Account::default()
    };
    let extra = Metric {
        label: "Extra".into(),
        used: Some(5.0),
        limit: Some(50.0),
        remaining: Some(45.0),
        unit: "USD".into(),
        kind: MetricKind::Spend,
        window: None,
        period: None,
        resets_at: None,
        billing: Billing::Overage,
        scope: None,
    };
    let report = Report {
        account_id: None,
        account: "Work".into(),
        provider: Provider::Claude,
        checked_at: chrono::Utc::now(),
        status: Status::Ready,
        metrics: vec![extra],
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
        diagnostic: None,
        sub_reports: Vec::new(),
        as_of: None,
        overage_hint: None,
    };
    let view =
        AccountView::from_report(&account, Some(&report), crate::freshness::Freshness::Fresh);
    assert!(view.on_extra);
    let none = AccountView::from_report(&account, None, crate::freshness::Freshness::Fresh);
    assert!(!none.on_extra);
}
