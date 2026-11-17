//! PRD 0025 RF1 acceptance: table tests per style.

use super::*;
use chrono::TimeZone;

fn now() -> DateTime<Utc> {
    Utc.timestamp_opt(1_800_000_000, 0).unwrap()
}

fn later(minutes: i64) -> DateTime<Utc> {
    now() + chrono::TimeDelta::minutes(minutes)
}

/// A time today as the title writes it in pt-BR (24 hours, UTC here).
fn hhmm(t: DateTime<Utc>) -> String {
    t.format("%H:%M").to_string()
}

/// A time today as the en-US label reads it ("9:30 AM").
fn ampm(t: DateTime<Utc>) -> String {
    t.format("%-I:%M\u{a0}%p").to_string()
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
    summary(accounts, style, selection, now(), &Utc, "pt-BR")
}

/// Every account selected, in order: the title names the account.
fn custom(accounts: &[AccountView], style: TrayStyle) -> TraySummary {
    let keys = accounts.iter().map(|a| a.key.clone()).collect();
    run(accounts, style, &TraySelection::Custom(keys))
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
    // Automatic names the provider, never the account (V-07).
    assert_eq!(s.title.as_deref(), Some("22% · Claude"));
    assert_eq!(
        custom(&accounts, TrayStyle::TightestPercent)
            .title
            .as_deref(),
        Some("22% · Work")
    );
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
    let s = custom(&accounts, TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("64% · Home"));
    // Every reading stale: the stale one counts, with its mark.
    let s = custom(&[old], TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("5%· · Old"));
    assert_eq!(
        s.accessible_label.resolve("en-US"),
        "Claude Old 5% left (old reading)"
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
    assert_eq!(s.accessible_label.resolve("en-US"), "Claude Work: no data");
}

#[test]
fn exhausted_shows_the_return_time() {
    let mut out = acc("a", "Claude", "Work", Some(0.0));
    out.exhausted = true;
    out.resets_at = Some(later(90));
    let s = custom(
        &[out.clone(), acc("b", "Gemini", "G", Some(0.5))],
        TrayStyle::TightestPercent,
    );
    let at = hhmm(later(90));
    assert_eq!(s.title, Some(format!("0% → {at} · Work")));
    assert_eq!(s.items[0].level, Some(Level::Bad));
    assert!(s.accessible_label.resolve("en-US").starts_with(&format!(
        "Claude Work exhausted, returns {}",
        ampm(later(90))
    )));
    // A return time already past is dropped.
    out.resets_at = Some(later(-5));
    assert_eq!(
        custom(&[out], TrayStyle::TightestPercent).title.as_deref(),
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
    let title = custom(&long, TrayStyle::TightestPercent).title.unwrap();
    assert!(title.chars().count() <= TITLE_BUDGET);
    assert!(title.starts_with("30%"));
    assert_eq!(title, "30% · A very long…", "cut at a word, marked");
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
    let logos = auto(&accounts, TrayStyle::LogosPercent);
    assert_eq!(logos.items.len(), AUTO_LOGOS);
    // Re-validation V-09: the "+N" counts the accounts beyond the cap too.
    let title = logos.title.unwrap();
    let shown = title.matches('%').count();
    assert!(
        title.ends_with(&format!(" +{}", accounts.len() - shown)),
        "{title}"
    );
    assert_eq!(logos.hidden, accounts.len() - shown);
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
    let s = custom(std::slice::from_ref(&a), TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("! 20% · Work"));
    assert_eq!(s.badge, Badge::Alert);
    assert!(s.items[0].on_extra);
    assert_eq!(
        s.accessible_label.resolve("en-US"),
        "Claude Work 20% left, alert active, using extra usage"
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
    assert_eq!(percent(0.004, "en-US"), "<1%");
    assert_eq!(percent(0.0, "en-US"), "0%");
    assert_eq!(percent(0.998, "en-US"), "99%");
    assert_eq!(percent(1.0, "en-US"), "100%");
}

/// V-17: the figure places its percent sign as the language does.
#[test]
fn the_percent_sign_follows_the_language() {
    assert_eq!(percent(0.64, "de"), "64\u{a0}%");
    assert_eq!(percent(0.64, "tr"), "%64");
    assert_eq!(percent(0.004, "fr"), "<1\u{a0}%");
    assert_eq!(
        percent_l10n(0.64).resolve("de"),
        "64\u{a0}%",
        "the accessible label resolves in the reader's language"
    );
}

#[test]
fn labels_render_in_the_four_languages() {
    let mut a = acc("a", "Claude", "Work", Some(0.22));
    a.resets_at = Some(later(60));
    let b = acc("b", "Gemini", "Home", Some(0.64));
    let s = auto(&[a, b], TrayStyle::TightestPercent);
    let at = ampm(later(60));
    assert_eq!(
        s.accessible_label.resolve("en-US"),
        format!("Claude Work 22% left, resets {at}; Gemini Home 64% left")
    );
    for locale in crate::text::catalog_locales() {
        let text = s.accessible_label.resolve(locale);
        assert!(
            text.contains(&percent(0.22, locale)) && text.contains("Home"),
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
        derived_from: None,
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
        breakdown: Vec::new(),
    };
    let view =
        AccountView::from_report(&account, Some(&report), crate::freshness::Freshness::Fresh);
    assert!(view.on_extra);
    let none = AccountView::from_report(&account, None, crate::freshness::Freshness::Fresh);
    assert!(!none.on_extra);
}

#[test]
fn level_thresholds_are_70_and_30_and_match_the_desktop_ui() {
    assert_eq!(Level::of(0.7), Level::Good);
    assert_eq!(Level::of(0.6999), Level::Warn);
    assert_eq!(Level::of(0.3), Level::Warn);
    assert_eq!(Level::of(0.2999), Level::Bad);
    let lib = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../apps/desktop/ui/js/lib.js"
    ))
    .unwrap();
    let line = lib
        .lines()
        .find(|l| l.starts_with("export const level ="))
        .expect("lib.js level()");
    let expected = format!(
        "p >= {} ? \"good\" : p >= {} ? \"warn\"",
        (Level::GOOD_AT * 100.0).round(),
        (Level::WARN_AT * 100.0).round()
    );
    assert!(line.contains(&expected), "{line}");
}

/// PRD 0043 RF4.2: the hourly strip has no title, one item (the tightest
/// account) and a label that says what the bars are.
#[test]
fn the_hourly_strip_shows_the_tightest_account_without_a_title() {
    let accounts = [
        acc("a", "Claude", "Work", Some(0.8)),
        acc("b", "Codex", "Home", Some(0.2)),
        acc("c", "Gemini", "Lab", None),
    ];
    let s = auto(&accounts, TrayStyle::HourlyStrip);
    assert_eq!(s.title, None);
    assert_eq!(s.items.len(), 1);
    assert_eq!(s.items[0].key, "b");
    assert_eq!(s.hidden, 2);
    assert_eq!(
        s.accessible_label.resolve("en-US"),
        "Codex Home 20% left; the last 24 hours as hourly bars"
    );
    assert_eq!(
        strip_account(&accounts, &TraySelection::Auto).as_deref(),
        Some("b")
    );
    let only_a = TraySelection::Custom(vec!["a".into()]);
    assert_eq!(strip_account(&accounts, &only_a).as_deref(), Some("a"));
    let empty = auto(&[], TrayStyle::HourlyStrip);
    assert!(empty.items.is_empty());
    assert_eq!(
        empty.accessible_label.resolve("en-US"),
        "No accounts in the menu bar"
    );
}

#[test]
fn strip_bars_keep_gaps_and_resets() {
    use crate::insights::{Rhythm, RhythmBucket, RhythmRange};
    let bucket = |consumption, reset| RhythmBucket {
        start: now(),
        hour: Some(0),
        day: None,
        consumption,
        samples: usize::from(consumption.is_some()),
        reset,
        day_off: false,
        label: crate::text::Localized::fixed("x", "x"),
    };
    let rhythm = Rhythm {
        range: RhythmRange::Last24h,
        buckets: vec![
            bucket(Some(2.5), false),
            bucket(None, false),
            bucket(Some(0.0), true),
        ],
        metrics: Vec::new(),
    };
    let bars = strip_bars(&rhythm);
    assert_eq!(bars.len(), 3);
    assert_eq!(bars[0].consumption, Some(2.5));
    assert_eq!(bars[1].consumption, None, "a gap stays a gap");
    assert!(bars[2].reset);
}

/// A time `days` days after `now()`, at 21:31 UTC.
fn on_day(days: i64) -> DateTime<Utc> {
    let day = now().date_naive() + chrono::TimeDelta::days(days);
    Utc.from_utc_datetime(&day.and_hms_opt(21, 31, 0).unwrap())
}

fn exhausted_until(name: &str, at: DateTime<Utc>) -> AccountView {
    let mut a = acc("a", "Claude", name, Some(0.0));
    a.exhausted = true;
    a.resets_at = Some(at);
    a
}

fn summary_in(accounts: &[AccountView], locale: &str) -> TraySummary {
    let keys = accounts.iter().map(|a| a.key.clone()).collect();
    summary(
        accounts,
        TrayStyle::TightestPercent,
        &TraySelection::Custom(keys),
        now(),
        &Utc,
        locale,
    )
}

/// PRD 0025 re-validation V-02: the core owns the clock. A return today is
/// the time in the locale's hour cycle; within six days the short weekday;
/// further away the date. With an alert and a long name the title stays
/// within the budget and the time is never cut.
#[test]
fn return_times_read_today_weekday_or_date_and_are_never_cut() {
    // now() is Friday 2027-01-15, 08:00 UTC.
    let cases = [
        (0, "en-US", "0% → 9:31\u{a0}PM", "returns 9:31\u{a0}PM"),
        (3, "en-US", "0% → Mon", "returns Monday 9:31\u{a0}PM"),
        (10, "en-US", "0% → 01/25", "returns 01/25 9:31\u{a0}PM"),
        (0, "pt-BR", "0% → 21:31", "volta 21:31"),
        (3, "pt-BR", "0% → seg", "volta segunda-feira 21:31"),
        (10, "pt-BR", "0% → 25/01", "volta 25/01 21:31"),
    ];
    for (days, locale, figure, label) in cases {
        for alert in [false, true] {
            for name in ["Work", "Engineering-Production"] {
                let mut a = exhausted_until(name, on_day(days));
                a.alert = alert;
                let s = summary_in(&[a], locale);
                let title = s.title.unwrap();
                assert!(
                    title.chars().count() <= TITLE_BUDGET,
                    "{days} {locale} {alert} {name}: {title}"
                );
                let head = if alert {
                    format!("! {figure}")
                } else {
                    figure.to_owned()
                };
                assert!(title.starts_with(&head), "{days} {locale}: {title}");
                let text = s.accessible_label.resolve(locale);
                assert!(text.contains(label), "{days} {locale}: {text}");
            }
        }
    }
}

/// V-02: the projected-exhaustion chip writes its time with the core's
/// short clock too.
#[test]
fn the_outlook_chip_uses_the_core_clock() {
    let mut a = acc("a", "Claude", "Personal account", Some(0.4));
    a.runs_out_at = Some(on_day(2));
    let s = summary_in(&[a], "en-US");
    assert_eq!(s.title.as_deref(), Some("↑ ≈Sun"));
}

/// V-01/S-02: an unnamed account reads as its provider in the title, the
/// items and the label, with no dangling separator; two unnamed accounts
/// of different providers, and one name under two providers, stay apart in
/// the label.
#[test]
fn unnamed_and_same_named_accounts_are_told_apart() {
    let unnamed = acc("a", "Claude", "", Some(0.22));
    let s = custom(std::slice::from_ref(&unnamed), TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("22% · Claude"));
    assert_eq!(s.items[0].name, "Claude");
    assert_eq!(s.accessible_label.resolve("en-US"), "Claude 22% left");
    let two = [unnamed.clone(), acc("b", "Gemini", " ", Some(0.64))];
    assert_eq!(
        auto(&two, TrayStyle::TightestPercent)
            .accessible_label
            .resolve("en-US"),
        "Claude 22% left; Gemini 64% left"
    );
    let same = [
        acc("a", "Claude", "Personal", Some(0.22)),
        acc("b", "Gemini", "Personal", Some(0.64)),
    ];
    assert_eq!(
        auto(&same, TrayStyle::TightestPercent)
            .accessible_label
            .resolve("en-US"),
        "Claude Personal 22% left; Gemini Personal 64% left"
    );
    let mut stale = unnamed;
    stale.stale = true;
    let s = custom(&[stale], TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("22%· · Claude"));
    assert_eq!(
        s.accessible_label.resolve("en-US"),
        "Claude 22% left (old reading)"
    );
}

/// V-07: an e-mail-shaped name never reaches the title, under any
/// selection; it stays in the label.
#[test]
fn an_email_shaped_name_shows_as_the_provider() {
    let a = acc("a", "Claude", "someone@example.com", Some(0.31));
    let s = custom(std::slice::from_ref(&a), TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("31% · Claude"));
    assert!(s
        .accessible_label
        .resolve("en-US")
        .starts_with("Claude someone@example.com"));
}

/// V-07: a cut name ends with "…" inside the budget; the figure stays.
#[test]
fn a_long_name_is_cut_with_an_ellipsis() {
    let mut a = acc("a", "Claude", "Engineering-Production", Some(0.12));
    let s = custom(std::slice::from_ref(&a), TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("12% · Engineering-Produ…"));
    a.alert = true;
    let s = custom(&[a], TrayStyle::TightestPercent);
    assert_eq!(s.title.as_deref(), Some("! 12% · Engineering-Pro…"));
    assert_eq!(cut("abc", 3), "abc");
    assert_eq!(cut("abcd", 3), "ab…");
    assert_eq!(cut("日本語の名前", 4), "日本語…");
}

/// S-03: the label names at most six accounts, then "+N more", and cuts a
/// long name.
#[test]
fn the_label_is_bounded() {
    let accounts: Vec<AccountView> = (0..9)
        .map(|i| {
            acc(
                &format!("k{i}"),
                "Claude",
                &format!("Account {i}"),
                Some(0.5),
            )
        })
        .collect();
    let s = auto(&accounts, TrayStyle::IconOnly);
    let text = s.accessible_label.resolve("en-US");
    assert!(text.ends_with("; +3 more"), "{text}");
    assert!(
        text.contains("Account 5") && !text.contains("Account 6"),
        "{text}"
    );
    for locale in crate::text::catalog_locales() {
        let text = s.accessible_label.resolve(locale);
        assert!(
            text.contains('3') && !text.contains('{'),
            "{locale}: {text}"
        );
    }
    let long = acc("a", "Claude", &"x".repeat(5000), Some(0.5));
    let text = auto(&[long], TrayStyle::IconOnly)
        .accessible_label
        .resolve("en-US");
    assert!(text.chars().count() < 60, "{text}");
}

/// V-01: `from_report` takes the display name, so an unnamed account
/// reads as its provider.
#[test]
fn from_report_uses_the_display_name() {
    let account = crate::Account {
        provider: crate::Provider::Claude,
        ..crate::Account::default()
    };
    let view = AccountView::from_report(&account, None, crate::freshness::Freshness::Fresh);
    assert_eq!(view.name, "Claude");
}

// PRD 0036 RF5.3/RF7.2 (V-05): the items of a summary are units.
mod units {
    use super::*;
    use crate::model::MetricKind;
    use crate::{Metric, Provider};

    fn quota(used: f64) -> Metric {
        Metric {
            billing: Default::default(),
            scope: None,
            label: "weekly (7d)".into(),
            used: Some(used),
            limit: Some(100.0),
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: None,
            period: None,
            resets_at: Some(later(90)),
            derived_from: None,
        }
    }

    fn sub(id: &str, used: Option<f64>) -> SubReport {
        SubReport {
            id: id.into(),
            upstream: "claude".into(),
            provider: Some(Provider::Claude),
            label: format!("conn-{id}"),
            status: Status::Ready,
            metrics: used.map(quota).into_iter().collect(),
            notes_l10n: Vec::new(),
            checked_at: now(),
            as_of: None,
            enabled_upstream: Some(true),
            duplicate_of: None,
            plan: None,
            via: None,
            identity: None,
        }
    }

    fn report(name: &str, provider: Provider, status: Status, subs: Vec<SubReport>) -> Report {
        Report {
            overage_hint: None,
            as_of: None,
            account_id: None,
            diagnostic: None,
            account: name.into(),
            provider,
            checked_at: now(),
            status,
            metrics: if provider.is_aggregator() {
                Vec::new()
            } else {
                vec![quota(30.0)]
            },
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: subs,
            breakdown: Vec::new(),
        }
    }

    fn account(name: &str, provider: Provider) -> Account {
        Account {
            name: name.into(),
            provider,
            ..Account::default()
        }
    }

    /// A direct Claude account and a 9router instance with `subs`.
    fn setup(status: Status, subs: Vec<SubReport>, show_both: &str) -> (Vec<Account>, Vec<Report>) {
        let mut router = account("home", Provider::NineRouter);
        if !show_both.is_empty() {
            router.params.insert("show_both".into(), show_both.into());
        }
        (
            vec![account("work", Provider::Claude), router],
            vec![
                report("work", Provider::Claude, Status::Ready, Vec::new()),
                report("home", Provider::NineRouter, status, subs),
            ],
        )
    }

    fn names(views: &[AccountView]) -> Vec<String> {
        views.iter().map(|v| v.name.clone()).collect()
    }

    #[test]
    fn active_connections_count_and_off_or_duplicate_ones_do_not() {
        let mut off = sub("off", Some(10.0));
        off.enabled_upstream = Some(false);
        let mut dup = sub("dup", Some(95.0));
        dup.duplicate_of = Some("claude:work".into());
        let (accounts, reports) = setup(
            Status::Ready,
            vec![sub("a1", Some(80.0)), off, dup, sub("b2", None)],
            "",
        );
        let views = snapshot_views(&accounts, &reports);
        // The aggregator is not an item while it has active connections.
        assert_eq!(names(&views), ["work", "conn-a1", "conn-b2"]);
        let router = accounts[1].state_key();
        let a1 = &views[1];
        assert_eq!(a1.key, format!("{router}/a1"));
        assert_eq!(a1.parent.as_deref(), Some(router.as_str()));
        assert_eq!(a1.provider, "claude");
        assert!((a1.remaining.unwrap() - 0.2).abs() < 1e-9);
        assert!(!a1.stale);
        // The tightest item is now a connection, never "—".
        let s = summary(
            &views,
            TrayStyle::TightestPercent,
            &TraySelection::Auto,
            now(),
            &Utc,
            "en-US",
        );
        assert_eq!(s.title.as_deref(), Some("20% · Claude"));
    }

    /// "Show in both" counts once, as the direct account.
    #[test]
    fn a_connection_kept_in_both_places_counts_once() {
        let (accounts, reports) = setup(
            Status::Ready,
            vec![sub("a1", Some(80.0)), sub("both", Some(99.0))],
            "a1x, BOTH",
        );
        assert_eq!(
            names(&snapshot_views(&accounts, &reports)),
            ["work", "conn-a1"]
        );
    }

    /// A failed instance is one failing item; its connections are not.
    #[test]
    fn a_failed_instance_is_one_failing_item() {
        let (accounts, reports) = setup(Status::Error, vec![sub("a1", Some(80.0))], "");
        let views = snapshot_views(&accounts, &reports);
        assert_eq!(names(&views), ["work", "home"]);
        assert_eq!(views[1].key, accounts[1].state_key());
        assert!(views[1].stale);
        assert_eq!(views[1].remaining, None);
        assert_eq!(views[1].parent, None);
    }

    /// An instance without an active connection is one unmeasured item.
    #[test]
    fn an_empty_instance_is_one_unmeasured_item() {
        let mut off = sub("off", Some(10.0));
        off.enabled_upstream = Some(false);
        for subs in [Vec::new(), vec![off]] {
            let (accounts, reports) = setup(Status::Ready, subs, "");
            let views = snapshot_views(&accounts, &reports);
            assert_eq!(names(&views), ["work", "home"]);
            assert_eq!(views[1].remaining, None);
            assert!(!views[1].stale);
        }
    }

    /// The desktop's "connections in All: Hidden" leaves every aggregator
    /// item out (connections and instance items); direct accounts stay.
    #[test]
    fn the_hidden_setting_drops_aggregator_items() {
        let (accounts, reports) = setup(Status::Ready, vec![sub("a1", Some(80.0))], "");
        let views = snapshot_views(&accounts, &reports);
        assert_eq!(names(&without_aggregators(&views)), ["work"]);
        let (accounts, reports) = setup(Status::Error, Vec::new(), "");
        let views = snapshot_views(&accounts, &reports);
        assert_eq!(names(&without_aggregators(&views)), ["work"]);
    }

    /// A custom selection naming the aggregator picks its connections.
    #[test]
    fn a_custom_aggregator_id_picks_its_connections() {
        let (accounts, reports) = setup(
            Status::Ready,
            vec![sub("a1", Some(80.0)), sub("b2", Some(10.0))],
            "",
        );
        let views = snapshot_views(&accounts, &reports);
        let picked = TraySelection::Custom(vec![
            accounts[1].state_key(),
            format!("{}/a1", accounts[1].state_key()),
        ])
        .pick(&views);
        let picked: Vec<&str> = picked.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(picked, ["conn-a1", "conn-b2"]);
    }

    #[test]
    fn the_connection_rule() {
        let flags = ConnectionFlags::default();
        assert!(connection_counts(flags));
        assert!(!connection_counts(ConnectionFlags {
            enabled_upstream: Some(false),
            ..flags
        }));
        assert!(!connection_counts(ConnectionFlags {
            duplicate_of: Some("claude:work"),
            ..flags
        }));
        assert!(!connection_counts(ConnectionFlags {
            show_both: true,
            ..flags
        }));
    }
}
