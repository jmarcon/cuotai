//! `cuotai --status-line` (PRD 0031 RF2): one line for a status bar, from
//! the core's `tray::summary` after one refresh. The figures, levels, stale
//! marks and width budget are the core's; this module only picks the style
//! and the accounts, and optionally paints the line.

use chrono::{DateTime, Utc};
use cuotai_core::{
    freshness::Freshness,
    tray::{self, AccountView, Level, TrayStyle, TraySelection},
    Config, Snapshot, Status,
};

/// The styles `--status-line` accepts (RF2.1): the two that make a title.
pub(crate) const STYLES: [&str; 2] = ["tightest_percent", "logos_percent"];

/// The one line for `snapshot`, in `config`'s account order. `ids` empty
/// selects every account; otherwise only those ids, in the order given.
pub(crate) fn render(
    config: &Config,
    snapshot: &Snapshot,
    style: TrayStyle,
    ids: &[String],
    color: bool,
    now: DateTime<Utc>,
    at: impl Fn(DateTime<Utc>) -> String,
) -> String {
    let views: Vec<AccountView> = config
        .accounts
        .iter()
        .zip(&snapshot.reports)
        .map(|(account, report)| {
            // A failed query keeps its (possibly preserved) reading, marked.
            let freshness = if report.status == Status::Ready {
                Freshness::Fresh
            } else {
                Freshness::Stale
            };
            AccountView::from_report(account, Some(report), freshness)
        })
        .collect();
    let selection = if ids.is_empty() {
        TraySelection::Auto
    } else {
        TraySelection::Custom(ids.to_vec())
    };
    let summary = tray::summary(&views, style, &selection, now, at);
    let title = summary.title.unwrap_or_default();
    let level = summary
        .items
        .iter()
        .filter(|item| !item.stale)
        .filter_map(|item| item.remaining.zip(item.level))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, level)| level);
    match (color, level) {
        (true, Some(level)) if !title.is_empty() => {
            format!("\x1b[{}m{title}\x1b[0m", ansi(level))
        }
        _ => title,
    }
}

/// ANSI foreground of a level: the theme roles good, warn and bad.
fn ansi(level: Level) -> u8 {
    match level {
        Level::Good => 32,
        Level::Warn => 33,
        Level::Bad => 31,
    }
}

/// RF2.2: colour only when asked, on a terminal, and `NO_COLOR` is unset
/// (an empty `NO_COLOR` counts as unset, per no-color.org).
pub(crate) fn use_color(flag: bool, is_terminal: bool, no_color: Option<&str>) -> bool {
    flag && is_terminal && no_color.is_none_or(str::is_empty)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use cuotai_core::{Metric, MetricKind, MetricWindow, Provider, Report};

    fn config(names: &[&str]) -> Config {
        let toml: String = names
            .iter()
            .map(|name| format!("[[accounts]]\nname = \"{name}\"\nprovider = \"claude\"\n\n"))
            .collect();
        Config::from_toml(&toml).unwrap()
    }

    fn report(name: &str, used: f64, status: Status) -> Report {
        Report {
            account_id: None,
            diagnostic: None,
            account: name.into(),
            provider: Provider::Claude,
            checked_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            status,
            metrics: vec![Metric {
                billing: Default::default(),
                scope: None,
                label: "5h".into(),
                used: Some(used),
                limit: Some(100.0),
                remaining: None,
                unit: "%".into(),
                kind: MetricKind::Quota,
                window: Some(MetricWindow::Weekly),
                period: None,
                resets_at: None,
            }],
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        }
    }

    fn snapshot(reports: Vec<Report>) -> Snapshot {
        Snapshot {
            started_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            finished_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            reports,
        }
    }

    fn line(style: TrayStyle, ids: &[&str], color: bool) -> String {
        let cfg = config(&["main", "side"]);
        let snap = snapshot(vec![
            report("main", 78.0, Status::Ready),
            report("side", 20.0, Status::Ready),
        ]);
        let ids: Vec<String> = ids.iter().map(|s| (*s).to_owned()).collect();
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, 0).unwrap();
        render(&cfg, &snap, style, &ids, color, now, |t| t.to_rfc3339())
    }

    #[test]
    fn tightest_percent_names_the_tightest_account() {
        assert_eq!(line(TrayStyle::TightestPercent, &[], false), "22% · main");
    }

    #[test]
    fn logos_percent_lists_each_account() {
        assert_eq!(line(TrayStyle::LogosPercent, &[], false), "C 22% · C 80%");
    }

    #[test]
    fn account_selection_limits_the_line() {
        let id = config(&["main", "side"]).accounts[1].id().to_string();
        assert_eq!(line(TrayStyle::TightestPercent, &[&id], false), "80% · side");
    }

    #[test]
    fn colour_wraps_the_line_by_its_level() {
        assert_eq!(
            line(TrayStyle::TightestPercent, &[], true),
            "\x1b[31m22% · main\x1b[0m"
        );
    }

    #[test]
    fn no_color_and_non_terminals_disable_colour() {
        assert!(use_color(true, true, None));
        assert!(use_color(true, true, Some("")));
        assert!(!use_color(true, true, Some("1")));
        assert!(!use_color(true, false, None));
        assert!(!use_color(false, true, None));
    }
}
