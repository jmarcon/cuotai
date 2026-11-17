//! `cuotai --status-line` (PRD 0031 RF2): one line for a status bar, from
//! the core's `tray::summary` after one refresh. The figures, levels, stale
//! marks and width budget are the core's; this module only picks the style
//! and the accounts, and optionally paints the line.

use std::fmt::Display;
use std::path::Path;

use chrono::{DateTime, TimeZone, Utc};
use cuotai_core::{
    serve::{self, client},
    tray::{self, AccountView, Level, TraySelection, TrayStyle},
    Config, Snapshot,
};

/// The styles `--status-line` accepts (RF2.1): the two that make a title.
pub(crate) const STYLES: [&str; 2] = ["tightest_percent", "logos_percent"];

/// The one line for `snapshot`, in `config`'s account order. `ids` empty
/// selects every item of the summary (direct accounts and the active
/// connections of aggregators, [`tray::summary_units`]); otherwise only
/// those ids, in the order given (an aggregator's id picks its
/// connections).
/// Times read in `tz` and `locale`, as the core writes them (PRD 0025
/// re-validation V-02: the same clock as the desktop menu bar).
#[allow(clippy::too_many_arguments)]
pub(crate) fn render<Tz>(
    config: &Config,
    snapshot: &Snapshot,
    style: TrayStyle,
    ids: &[String],
    color: bool,
    now: DateTime<Utc>,
    tz: &Tz,
    locale: &str,
) -> String
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    // PRD 0036 RF5.3 (V-05): the items are the core's units, so an
    // aggregator counts through its active connections.
    let views = tray::snapshot_views(&config.accounts, &snapshot.reports);
    render_views(&views, style, ids, color, now, tz, locale)
}

/// The one line for `views`, already in order (the same rules as
/// [`render`]).
pub(crate) fn render_views<Tz>(
    views: &[AccountView],
    style: TrayStyle,
    ids: &[String],
    color: bool,
    now: DateTime<Utc>,
    tz: &Tz,
    locale: &str,
) -> String
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let selection = if ids.is_empty() {
        TraySelection::Auto
    } else {
        TraySelection::Custom(ids.to_vec())
    };
    let summary = tray::summary(views, style, &selection, now, tz, locale);
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

/// PRD 0031 RF2.1: the line from a running server's `/v1/limits` (`url`
/// empty: the default address), without querying a provider. The token is
/// `token_file`'s, or the `--serve` default file's when it exists (never
/// created here). Exit code `0`, or `2` when a selected account's reading
/// is stale or failing, like `--once`; a server that cannot be read is a
/// local error.
pub(crate) async fn from_server(
    url: &str,
    token_file: Option<&Path>,
    style: TrayStyle,
    ids: &[String],
    color: bool,
) -> Result<(String, u8), String> {
    let locale = crate::ui::tui_locale();
    let token = match token_file {
        Some(path) => Some(path.to_path_buf()),
        None => serve::default_token_file().filter(|path| path.is_file()),
    }
    .map(|path| client::read_token(&path))
    .transpose()
    .map_err(|error| error.l10n().resolve(locale))?;
    let url = if url.is_empty() {
        client::default_url()
    } else {
        url.to_owned()
    };
    let accounts = client::limits_accounts(&url, token.as_ref())
        .await
        .map_err(|error| error.l10n().resolve(locale))?;
    // V-01: an id, a key or a unique name; the line and the exit code then
    // cover the selected accounts only (R-01).
    let candidates: Vec<Candidate<'_>> = accounts
        .iter()
        .map(|account| Candidate {
            id: &account.view.key,
            key: &account.key,
            name: &account.view.name,
        })
        .collect();
    let picked = resolve(&candidates, ids, locale)?;
    let ids: Vec<String> = picked
        .iter()
        .map(|&i| accounts[i].view.key.clone())
        .collect();
    // PRD 0036 RF5.3 (V-05): the line covers the summary's items only.
    let views: Vec<AccountView> = accounts
        .into_iter()
        .filter(|account| account.in_summary)
        .map(|account| account.view)
        .collect();
    let line = render_views(
        &views,
        style,
        &ids,
        color,
        Utc::now(),
        &chrono::Local,
        locale,
    );
    let selection = if ids.is_empty() {
        TraySelection::Auto
    } else {
        TraySelection::Custom(ids)
    };
    let failing = selection.pick(&views).iter().any(|view| view.stale);
    Ok((line, if failing { 2 } else { 0 }))
}

/// One account `--account` may name: its stable id, its legacy key and its
/// name as written in the TOML (or shown by the server).
pub(crate) struct Candidate<'a> {
    pub id: &'a str,
    pub key: &'a str,
    pub name: &'a str,
}

/// PRD 0031 re-validation V-01: each `--account` value, resolved in this
/// order to the stable id, then the key, then a name that only one account
/// has (case-sensitive). The positions of the matches, in the order given;
/// a value that matches nothing, or a name several accounts share, is a
/// local error that lists the ids (at most 10).
pub(crate) fn resolve(
    candidates: &[Candidate<'_>],
    wanted: &[String],
    locale: &str,
) -> Result<Vec<usize>, String> {
    let mut picked = Vec::with_capacity(wanted.len());
    for value in wanted {
        let by = |field: usize| -> Vec<usize> {
            candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| [c.id, c.key, c.name][field] == value)
                .map(|(i, _)| i)
                .collect()
        };
        let found = (0..3)
            .map(by)
            .find(|hits| !hits.is_empty())
            .filter(|hits| hits.len() == 1)
            .map(|hits| hits[0]);
        match found {
            Some(index) if !picked.contains(&index) => picked.push(index),
            Some(_) => {}
            None => return Err(unknown(candidates, value, locale)),
        }
    }
    Ok(picked)
}

/// The error of a `--account` value that names no single account.
fn unknown(candidates: &[Candidate<'_>], value: &str, locale: &str) -> String {
    const SHOWN: usize = 10;
    let mut list: Vec<String> = candidates
        .iter()
        .take(SHOWN)
        .map(|c| {
            format!(
                "{} ({})",
                cuotai_core::text::strip_disguising(c.id),
                cuotai_core::text::strip_disguising(c.name)
            )
        })
        .collect();
    if candidates.len() > SHOWN {
        list.push("…".to_owned());
    }
    let list = if list.is_empty() {
        crate::l10n::tr_in(locale, "status_line.no_accounts", "none").to_owned()
    } else {
        list.join(", ")
    };
    // PRD 0034 re-validation (S-03): one pass, so a `{ids}` inside the value stays literal.
    let value = cuotai_core::text::strip_disguising(value);
    crate::l10n::trf_in(
        locale,
        "status_line.unknown_account",
        "--account {value} names no single account; use an id (from --json-v1, /v1/limits or the TOML's id line), a key or a unique name. Ids: {ids}.",
        &[("value", &value), ("ids", &list)],
    )
}

/// ANSI foreground of a level: the theme roles good, warn and bad.
pub(crate) fn ansi(level: Level) -> u8 {
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
    use cuotai_core::{Metric, MetricKind, MetricWindow, Provider, Report, Status};

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
                derived_from: None,
                unlimited: false,
            }],
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
            breakdown: Vec::new(),
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
        render(&cfg, &snap, style, &ids, color, now, &Utc, "en-US")
    }

    /// Automatic names the provider (PRD 0025 re-validation V-07); a
    /// chosen account is named.
    #[test]
    fn tightest_percent_names_the_tightest_account() {
        assert_eq!(line(TrayStyle::TightestPercent, &[], false), "22% · Claude");
        let id = config(&["main", "side"]).accounts[0].id().to_string();
        assert_eq!(
            line(TrayStyle::TightestPercent, &[&id], false),
            "22% · main"
        );
    }

    /// V-02: a return on another day reads as the weekday, in the core's
    /// clock, never a bare time and never cut by the alert mark.
    #[test]
    fn a_return_on_another_day_reads_as_its_weekday() {
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, 0).unwrap();
        let view = AccountView {
            key: "acc_a".into(),
            provider: "claude".into(),
            provider_label: "Claude".into(),
            name: "Work".into(),
            remaining: Some(0.0),
            exhausted: true,
            alert: true,
            resets_at: Some(Utc.with_ymd_and_hms(2026, 1, 4, 23, 30, 0).unwrap()),
            ..AccountView::default()
        };
        let line = render_views(
            &[view],
            TrayStyle::TightestPercent,
            &[],
            false,
            now,
            &Utc,
            "en-US",
        );
        assert_eq!(line, "! 0% → Sun · Claude");
    }

    /// PRD 0036 RF5.3 (V-05): an aggregator-only setup reads its active
    /// connections instead of "—"; a connection off in the aggregator or
    /// duplicating a direct account is not an item, and the aggregator's
    /// id picks its connections.
    #[test]
    fn an_aggregator_counts_through_its_connections() {
        let cfg = Config::from_toml("[[accounts]]\nname = \"home\"\nprovider = \"nine_router\"\n")
            .unwrap();
        let sub = |id: &str, used: f64| cuotai_core::SubReport {
            id: id.into(),
            upstream: "claude".into(),
            provider: Some(Provider::Claude),
            label: id.into(),
            status: Status::Ready,
            metrics: report("x", used, Status::Ready).metrics,
            notes_l10n: Vec::new(),
            checked_at: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
            as_of: None,
            enabled_upstream: Some(true),
            duplicate_of: None,
            plan: None,
            via: None,
            identity: None,
        };
        let mut off = sub("off", 99.0);
        off.enabled_upstream = Some(false);
        let mut dup = sub("dup", 98.0);
        dup.duplicate_of = Some("claude:work".into());
        let router = Report {
            provider: Provider::NineRouter,
            metrics: Vec::new(),
            sub_reports: vec![sub("a1", 40.0), sub("b2", 70.0), off, dup],
            ..report("home", 0.0, Status::Ready)
        };
        let snap = snapshot(vec![router]);
        let now = Utc.with_ymd_and_hms(2026, 1, 1, 0, 1, 0).unwrap();
        let go = |style, ids: &[String]| render(&cfg, &snap, style, ids, false, now, &Utc, "en-US");
        assert_eq!(go(TrayStyle::TightestPercent, &[]), "30% · Claude");
        assert_eq!(go(TrayStyle::LogosPercent, &[]), "C 60% · C 30%");
        let id = cfg.accounts[0].id().to_string();
        assert_eq!(go(TrayStyle::LogosPercent, &[id]), "C 60% · C 30%");
    }

    #[test]
    fn logos_percent_lists_each_account() {
        assert_eq!(line(TrayStyle::LogosPercent, &[], false), "C 22% · C 80%");
    }

    #[test]
    fn account_selection_limits_the_line() {
        let id = config(&["main", "side"]).accounts[1].id().to_string();
        assert_eq!(
            line(TrayStyle::TightestPercent, &[&id], false),
            "80% · side"
        );
    }

    #[test]
    fn colour_wraps_the_line_by_its_level() {
        assert_eq!(
            line(TrayStyle::TightestPercent, &[], true),
            "\x1b[31m22% · Claude\x1b[0m"
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

    /// PRD 0034 re-validation (S-03): the value is substituted in one pass, so
    /// a `{ids}` it carries stays literal, in English and from a catalog.
    #[test]
    fn unknown_value_placeholders_stay_literal() {
        let candidates = [Candidate {
            id: "id-one",
            key: "k1",
            name: "one",
        }];
        for locale in ["en-US", "pt-BR"] {
            let error = resolve(&candidates, &["{ids}".to_owned()], locale).unwrap_err();
            assert!(error.starts_with("--account {ids} "), "{locale}: {error}");
            assert_eq!(error.matches("id-one (one)").count(), 1, "{locale}");
        }
    }
}
