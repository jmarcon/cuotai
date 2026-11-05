//! PRD 0043 RF5: `cuotai --stats --rhythm` and `cuotai --digest`, read from
//! the local history only (no provider is queried). The figures and texts
//! come from `cuotai_core::insights` and `cuotai_core::digest`; this module
//! only picks the account, the range and the output format.

use crate::l10n;
use crate::ui;
use chrono::{DateTime, Utc};
use cuotai_core::config_file::ConfigFile;
use cuotai_core::digest::{self, DigestWeek};
use cuotai_core::history::HistoryStore;
use cuotai_core::insights::{self, DaysOff, RhythmQuery, RhythmRange};
use std::io::Write;

/// The bars of the text chart.
const CHART_WIDTH: usize = 30;

/// `--range` for `--stats --rhythm` (`None` without `--rhythm`): 24h by
/// default, 7d or typical.
pub(crate) fn rhythm_range(
    rhythm: bool,
    range: Option<&str>,
) -> Result<Option<RhythmRange>, String> {
    if !rhythm {
        return Ok(None);
    }
    match range {
        None => Ok(Some(RhythmRange::Last24h)),
        Some(value) => RhythmRange::parse(value).map(Some).ok_or_else(|| {
            l10n::tr(
                "insights.flag.range",
                "--range accepts 24h, 7d or typical with --stats --rhythm.",
            )
            .to_owned()
        }),
    }
}

/// The person's days off, from the shared notification preferences.
fn days_off() -> DaysOff {
    cuotai_core::notify_prefs::Source::open()
        .loaded()
        .notify
        .days_off
}

/// The position of the account named `name` (`None` for all accounts); an
/// unknown name is a local error.
fn scope(file: &ConfigFile, name: Option<&str>) -> Result<Option<usize>, String> {
    let Some(name) = name else {
        return Ok(None);
    };
    file.config
        .accounts
        .iter()
        .position(|account| account.name == name || account.display_name() == name)
        .map(Some)
        .ok_or_else(|| {
            l10n::trf(
                "cli.account_not_found",
                "Account not found in the configuration: {name}",
                &[("name", &ui::safe(name))],
            )
        })
}

fn unreadable(error: cuotai_core::history::HistoryError) -> u8 {
    crate::cli::message(
        crate::cli::Level::Warn,
        "history.unreadable",
        format_args!("cuotai: {}", error.l10n().resolve(ui::tui_locale())),
    );
    2
}

/// RF5: `--stats --rhythm [ACCOUNT] [--range …] [--json]`. Exit 0 with
/// data, 2 when nothing was recorded in the range or the store is
/// unreadable.
pub(crate) fn rhythm(
    store: &HistoryStore,
    file: &ConfigFile,
    account: Option<String>,
    range: RhythmRange,
    json: bool,
    now: DateTime<Utc>,
) -> Result<u8, Box<dyn std::error::Error>> {
    let query = RhythmQuery {
        scope: scope(file, account.as_deref())?,
        metric: None,
        range,
        days_off: days_off(),
    };
    let rhythm = match insights::rhythm(store, &file.config.accounts, &query, now, &chrono::Local) {
        Ok(rhythm) => rhythm,
        Err(error) => return Ok(unreadable(error)),
    };
    let mut stdout = std::io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut stdout, &rhythm)?;
        writeln!(stdout)?;
    } else {
        write!(
            stdout,
            "{}",
            rhythm.text_chart(ui::tui_locale(), CHART_WIDTH)
        )?;
    }
    let recorded = rhythm
        .buckets
        .iter()
        .any(|bucket| bucket.consumption.is_some());
    Ok(if recorded { 0 } else { 2 })
}

/// PRD 0043 §7: the `rhythm` (all accounts, every range) and the
/// `limit_changes` of the last 7 days that `--stats --json` adds.
pub(crate) fn insights_json(
    store: &HistoryStore,
    file: &ConfigFile,
    now: DateTime<Utc>,
) -> Result<serde_json::Map<String, serde_json::Value>, cuotai_core::history::HistoryError> {
    let mut rhythm = serde_json::Map::new();
    let days_off = days_off();
    for (key, range) in [
        ("last_24h", RhythmRange::Last24h),
        ("last_7d", RhythmRange::Last7d),
        ("typical", RhythmRange::Typical30d),
    ] {
        let query = RhythmQuery {
            scope: None,
            metric: None,
            range,
            days_off,
        };
        let value = insights::rhythm(store, &file.config.accounts, &query, now, &chrono::Local)?;
        rhythm.insert(
            key.into(),
            serde_json::to_value(value.buckets).unwrap_or_default(),
        );
    }
    let mut changes = Vec::new();
    for account in &file.config.accounts {
        for change in store.limit_changes(account, now - chrono::TimeDelta::days(7)..=now)? {
            let mut value = serde_json::to_value(&change).unwrap_or_default();
            if let Some(object) = value.as_object_mut() {
                object.insert("account_id".into(), account.state_key().into());
            }
            changes.push(value);
        }
    }
    let mut out = serde_json::Map::new();
    out.insert("rhythm".into(), rhythm.into());
    out.insert("limit_changes".into(), changes.into());
    Ok(out)
}

/// RF3.2: `--digest [--week last|current] [--json|--markdown]`. Exit 0; 2
/// when nothing was recorded in the week or the store is unreadable.
pub(crate) fn digest(
    store: &HistoryStore,
    file: &ConfigFile,
    week: Option<&str>,
    json: bool,
    markdown: bool,
    now: DateTime<Utc>,
) -> Result<u8, Box<dyn std::error::Error>> {
    let week = week.and_then(DigestWeek::parse).unwrap_or(DigestWeek::Last);
    let latest = match store.latest_reports(&file.config) {
        Ok(latest) => latest,
        Err(error) => return Ok(unreadable(error)),
    };
    let digest = match digest::weekly(
        store,
        &file.config.accounts,
        &latest,
        week,
        now,
        &chrono::Local,
        days_off(),
    ) {
        Ok(digest) => digest,
        Err(error) => return Ok(unreadable(error)),
    };
    let locale = ui::tui_locale();
    let mut stdout = std::io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(&mut stdout, &digest)?;
        writeln!(stdout)?;
    } else if markdown {
        write!(stdout, "{}", digest.markdown(locale))?;
    } else {
        writeln!(stdout, "{}", digest.title().resolve(locale))?;
        for line in digest.short_lines(locale) {
            writeln!(stdout, "  {}", line.resolve(locale))?;
        }
    }
    Ok(if digest.recorded { 0 } else { 2 })
}
