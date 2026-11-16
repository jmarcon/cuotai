//! PRD 0025 RF1/RF2: the menu-bar summary.
//!
//! One pure function, [`summary`], turns what a front end already knows per
//! account ([`AccountView`]) into what the menu bar shows: the selected items
//! in order, the title text, the badge and an accessible label. The desktop
//! tray and `cuotai --status-line` (PRD 0031 RF2) render from it; neither
//! decides figures, levels, stale marks or the width budget on its own.
//!
//! Honesty (RF1.4): an unknown share reads "—", never 0 %; a stale reading is
//! marked and left out of "tightest" unless every reading is stale; an
//! exhausted account shows its return time.
//!
//! The core owns the clock (PRD 0025 re-validation V-02): the title writes
//! a return or projected time with [`clock::clock_short`] (today's time in
//! the locale's hour cycle, else the short weekday or date) and the label
//! nests the full [`clock::clock`], so the desktop and `--status-line`
//! agree. Under `Auto` the title names the provider, never the account
//! (privacy, PRD §6); a selected account whose name looks like an e-mail
//! is also shown as its provider. Account names stay in the label, which
//! is bounded to [`LABEL_ITEMS`] accounts (S-03).

use std::fmt::Display;

use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};

use crate::freshness::Freshness;
use crate::metrics;
use crate::text::{clock, number, params, Localized, ParamValue};
use crate::{Account, Report};

/// RF1.5: the title never exceeds this many characters.
pub const TITLE_BUDGET: usize = 24;
/// RF2.1: `Auto` shows at most this many meters…
pub const AUTO_METERS: usize = 4;
/// …and this many "logo + %" items.
pub const AUTO_LOGOS: usize = 6;
/// The unknown figure (DS-CMP-MTR-5).
pub const UNKNOWN: &str = "—";
/// The trailing stale mark (RF1.4).
pub const STALE_MARK: &str = "·";
/// Separator between title items.
const SEPARATOR: &str = " · ";
/// The mark of a name cut to fit (DS-MSG-15).
const ELLIPSIS: char = '…';
/// S-03: the label (tooltip, accessibility) names at most this many
/// accounts, then "+N more".
pub const LABEL_ITEMS: usize = 6;
/// S-03: a name in the label is cut to this many characters.
pub const LABEL_NAME: usize = 32;

/// RF1.1: the menu-bar styles. `TightestPercent` is the default
/// (decision N). PRD 0043 RF4.2 adds `HourlyStrip` (macOS only): the last
/// 24 hours of the tightest account as hourly bars.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayStyle {
    Meters,
    LogosPercent,
    #[default]
    TightestPercent,
    IconOnly,
    HourlyStrip,
}

impl TrayStyle {
    pub const ALL: [Self; 5] = [
        Self::Meters,
        Self::LogosPercent,
        Self::TightestPercent,
        Self::IconOnly,
        Self::HourlyStrip,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Meters => "meters",
            Self::LogosPercent => "logos_percent",
            Self::TightestPercent => "tightest_percent",
            Self::IconOnly => "icon_only",
            Self::HourlyStrip => "hourly_strip",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.slug() == text)
    }
}

/// RF2.1: which accounts the menu bar shows, in which order. `Custom` holds
/// stable account ids (PRD 0029) in the user's order, independent of the
/// dashboard order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraySelection {
    #[default]
    Auto,
    Custom(Vec<String>),
}

impl TraySelection {
    /// The accounts the selection picks from `accounts` (dashboard order), in
    /// the selection's order, before any style cap.
    pub fn pick<'a>(&self, accounts: &'a [AccountView]) -> Vec<&'a AccountView> {
        match self {
            Self::Auto => accounts.iter().collect(),
            Self::Custom(keys) => keys
                .iter()
                .filter_map(|key| accounts.iter().find(|a| &a.key == key))
                .collect(),
        }
    }

    /// RF2.2: drop the ids of accounts that no longer exist (and duplicates);
    /// `Auto` stays `Auto`, so new accounts appear only there.
    pub fn pruned(&self, existing: &[String]) -> Self {
        match self {
            Self::Auto => Self::Auto,
            Self::Custom(keys) => {
                let mut kept: Vec<String> = Vec::new();
                for key in keys {
                    if existing.contains(key) && !kept.contains(key) {
                        kept.push(key.clone());
                    }
                }
                Self::Custom(kept)
            }
        }
    }
}

/// The DS-ID-5 level of a remaining share: `good` ≥ 70 %, `warn` 30–70 %,
/// `bad` < 30 %.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Good,
    Warn,
    Bad,
}

impl Level {
    /// Remaining share from which a level is `good` (DS-ID-5). The only
    /// definition: the TUI maps this type and the desktop test asserts that
    /// `lib.js` `level()` uses the same numbers.
    pub const GOOD_AT: f64 = 0.7;
    /// Remaining share from which a level is `warn` rather than `bad`.
    pub const WARN_AT: f64 = 0.3;

    pub fn of(remaining: f64) -> Self {
        if remaining >= Self::GOOD_AT {
            Self::Good
        } else if remaining >= Self::WARN_AT {
            Self::Warn
        } else {
            Self::Bad
        }
    }
}

/// What the icon carries besides the figures. An outage wins over an alert.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Badge {
    #[default]
    None,
    Alert,
    Outage,
}

/// RF1.1: what a front end knows about one account, already decided by the
/// core elsewhere (binding, freshness, alerts, incidents, overage).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct AccountView {
    /// The stable account id (PRD 0029).
    pub key: String,
    /// Provider slug.
    pub provider: String,
    /// The provider's display name; the title uses its initial.
    pub provider_label: String,
    /// The account's display name (PRD 0029 RF3.4: the configured name,
    /// else the provider's); a blank name reads as `provider_label`. The
    /// title never shows it under `Auto`, nor when it looks like an e-mail.
    pub name: String,
    /// Remaining share of the binding metric in `[0, 1]`; `None` when
    /// unmeasured, uncapped or failing without a last good reading.
    pub remaining: Option<f64>,
    /// The reading is stale (PRD 0010) or a preserved last good reading.
    pub stale: bool,
    /// The binding metric is consumed.
    pub exhausted: bool,
    /// When the binding metric resets, or when a blocked account returns.
    pub resets_at: Option<DateTime<Utc>>,
    /// PRD 0024: projected exhaustion before the reset, if any.
    pub runs_out_at: Option<DateTime<Utc>>,
    /// PRD 0008: an active usage alert.
    pub alert: bool,
    /// PRD 0011: the provider reports an outage.
    pub incident: bool,
    /// PRD 0023: the account is consuming extra usage.
    pub on_extra: bool,
}

impl AccountView {
    /// The view of `account` from its current report alone (no alerts,
    /// incidents or pace): what a headless consumer such as the status line
    /// can build from a snapshot.
    pub fn from_report(account: &Account, report: Option<&Report>, freshness: Freshness) -> Self {
        let binding = report.and_then(metrics::binding);
        Self {
            key: account.id().to_string(),
            provider: account.provider.slug().to_owned(),
            provider_label: account.provider.label().to_owned(),
            name: account.display_name().into_owned(),
            remaining: binding
                .as_ref()
                .map(|b| (1.0 - b.used_ratio).clamp(0.0, 1.0)),
            stale: freshness == Freshness::Stale,
            exhausted: binding
                .as_ref()
                .is_some_and(|b| b.reason == metrics::BindingReason::Exhausted),
            resets_at: binding
                .as_ref()
                .and_then(|b| b.blocked_until.or(b.resets_at)),
            // PRD 0023: paying for extra usage right now (active or at its cap).
            on_extra: report
                .and_then(crate::overage::assess)
                .is_some_and(|overage| overage.state.is_billing()),
            ..Self::default()
        }
    }
}

/// RF1.2: one selected account as the menu bar shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrayItem {
    pub key: String,
    pub provider: String,
    pub name: String,
    pub remaining: Option<f64>,
    /// `None` while the share is unknown.
    pub level: Option<Level>,
    pub stale: bool,
    pub exhausted: bool,
    pub alert: bool,
    pub on_extra: bool,
    /// The figure as the title writes it ("22%", "<1%", "—", "0% → 19:30").
    pub figure: String,
}

/// RF1.2: everything the menu bar needs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TraySummary {
    pub style: TrayStyle,
    /// The selected accounts, in selection order, after the style's cap.
    pub items: Vec<TrayItem>,
    /// Selected accounts left out by the cap or the width budget.
    pub hidden: usize,
    /// The text beside the icon; `None` for `Meters`, `IconOnly` and an
    /// empty selection.
    pub title: Option<String>,
    pub badge: Badge,
    pub accessible_label: Localized,
    pub tooltip: Localized,
}

/// The rounded figure of a remaining share: "<1%" for a positive share below
/// 1 % and never "100%" below 100 % (DS-CMP-MTR-4), with the percent sign
/// placed as the reader's language places it (V-17: "64\u{a0}%", "%64").
pub fn percent_l10n(remaining: f64) -> Localized {
    let p = remaining.clamp(0.0, 1.0) * 100.0;
    if p > 0.0 && p < 1.0 {
        number::percent_of("<1")
    } else if p < 100.0 && p.round() >= 100.0 {
        number::percent(99.0)
    } else {
        number::percent(p.round())
    }
}

/// [`percent_l10n`] in `locale`.
pub fn percent(remaining: f64, locale: &str) -> String {
    percent_l10n(remaining).resolve(locale)
}

/// The clock the summary writes times with: the short form for the title
/// (already in the reader's language) and the full form for the label.
struct Clock<'a> {
    short: &'a dyn Fn(DateTime<Utc>) -> String,
    full: &'a dyn Fn(DateTime<Utc>) -> Localized,
    /// The title's language, for its percents.
    locale: &'a str,
}

fn figure(view: &AccountView, clock: &Clock<'_>) -> String {
    let base = match (view.exhausted, view.remaining) {
        (true, _) => match view.resets_at {
            Some(when) => format!("{} → {}", percent(0.0, clock.locale), (clock.short)(when)),
            None => percent(0.0, clock.locale),
        },
        (false, Some(r)) => percent(r, clock.locale),
        (false, None) => UNKNOWN.to_owned(),
    };
    if view.stale && view.remaining.is_some() {
        format!("{base}{STALE_MARK}")
    } else {
        base
    }
}

/// The display name: the configured name, else the provider's.
fn display_name(view: &AccountView) -> &str {
    crate::model::display_title(&view.provider_label, &view.name).0
}

/// Whether `text` looks like an e-mail address (it carries an `@`).
fn looks_like_email(text: &str) -> bool {
    text.contains('@')
}

/// V-07: the name the title shows: the provider's under `Auto` (nobody
/// chose to show the account) and for an e-mail-shaped name; otherwise the
/// display name.
fn title_name<'a>(view: &'a AccountView, selection: &TraySelection) -> &'a str {
    let name = display_name(view);
    if matches!(selection, TraySelection::Auto) || looks_like_email(name) {
        view.provider_label.as_str()
    } else {
        name
    }
}

/// V-01: the name the label reads: the provider and the account ("Claude
/// Work"), or the provider alone for an unnamed account; cut with "…"
/// past [`LABEL_NAME`] characters (S-03).
fn label_name(view: &AccountView) -> String {
    let full = match crate::model::display_title(&view.provider_label, &view.name) {
        (name, Some(provider)) if !provider.is_empty() => format!("{provider} {name}"),
        (name, _) => name.to_owned(),
    };
    cut(&full, LABEL_NAME)
}

/// `text` within `max` characters: whole, or cut at a character boundary
/// (at a word boundary when one is near) and ended with "…".
fn cut(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    if max == 0 {
        return String::new();
    }
    let kept: String = text.chars().take(max - 1).collect();
    // Prefer the last word boundary in the second half of what is kept.
    let half = kept.chars().count() / 2;
    let kept = match kept.rfind(' ') {
        Some(space) if kept[..space].chars().count() >= half.max(1) => kept[..space].to_owned(),
        _ => kept,
    };
    format!("{}{ELLIPSIS}", kept.trim_end())
}

fn initial(view: &AccountView) -> String {
    view.provider_label
        .chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_else(|| "?".to_owned())
}

/// The tightest account: the lowest remaining share among fresh readings,
/// else among stale ones; an exhausted account counts as zero.
fn tightest<'a>(views: &[&'a AccountView]) -> Option<&'a AccountView> {
    let share = |v: &AccountView| if v.exhausted { Some(0.0) } else { v.remaining };
    let pick = |stale: bool| {
        views
            .iter()
            .copied()
            .filter(|v| v.stale == stale)
            .filter_map(|v| share(v).map(|s| (s, v)))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, v)| v)
    };
    pick(false).or_else(|| pick(true))
}

/// Joins `parts` with the separator inside the budget, dropping parts from
/// the end with "+N" before any part is cut. Returns the title and how many
/// parts were dropped.
fn fit(parts: &[String], alert: bool) -> (String, usize) {
    fit_more(parts, alert, 0)
}

/// [`fit`] where `beyond` more accounts were left out before (the `Auto`
/// cap): the "+N" counts them too (re-validation V-09).
fn fit_more(parts: &[String], alert: bool, beyond: usize) -> (String, usize) {
    let prefix = if alert { "! " } else { "" };
    let len = |s: &str| s.chars().count();
    for keep in (1..=parts.len()).rev() {
        let mut text = format!("{prefix}{}", parts[..keep].join(SEPARATOR));
        let dropped = parts.len() - keep;
        if dropped + beyond > 0 {
            text.push_str(&format!(" +{}", dropped + beyond));
        }
        if len(&text) <= TITLE_BUDGET {
            return (text, dropped);
        }
    }
    // Even one part does not fit: cut it, marked (never the case for
    // figures alone, which stay within the budget).
    let first = parts.first().map(String::as_str).unwrap_or("");
    let text = format!(
        "{prefix}{}",
        cut(first, TITLE_BUDGET - prefix.chars().count())
    );
    (text, parts.len().saturating_sub(1))
}

/// V-07: the figure and the name inside the budget. The figure (and with it
/// a return time) is never cut; the name gives way first, ended with "…",
/// and is dropped when not even one character of it would fit.
fn fit_named(fig: &str, name: &str, alert: bool) -> String {
    let prefix = if alert { "! " } else { "" };
    let head = format!("{prefix}{fig}");
    let name = name.trim();
    if name.is_empty() {
        return head;
    }
    let room = TITLE_BUDGET.saturating_sub(head.chars().count() + SEPARATOR.chars().count());
    if name.chars().count() <= room {
        return format!("{head}{SEPARATOR}{name}");
    }
    if room >= 2 {
        return format!("{head}{SEPARATOR}{}", cut(name, room));
    }
    head
}

fn item_label(view: &AccountView, clock: &Clock<'_>) -> Localized {
    let name = label_name(view);
    let name = name.as_str();
    let base = match (view.exhausted, view.remaining, view.resets_at) {
        (true, _, Some(when)) => Localized::with_params(
            "tray.item.exhausted_until",
            params([
                ("name", name.into()),
                ("at", ParamValue::l10n((clock.full)(when))),
            ]),
            "{name} exhausted, returns {at}",
        ),
        (true, _, None) => {
            Localized::with_param("tray.item.exhausted", "name", name, "{name} exhausted")
        }
        (false, Some(r), Some(when)) => Localized::with_params(
            "tray.item.left_resets",
            params([
                ("name", name.into()),
                ("pct", ParamValue::l10n(percent_l10n(r))),
                ("at", ParamValue::l10n((clock.full)(when))),
            ]),
            "{name} {pct} left, resets {at}",
        ),
        (false, Some(r), None) => Localized::with_params(
            "tray.item.left",
            params([
                ("name", name.into()),
                ("pct", ParamValue::l10n(percent_l10n(r))),
            ]),
            "{name} {pct} left",
        ),
        (false, None, _) => {
            Localized::with_param("tray.item.unknown", "name", name, "{name}: no data")
        }
    };
    let mut label = base;
    if view.stale && view.remaining.is_some() {
        label = wrap("tray.item.stale", label, "{item} (old reading)");
    }
    if view.alert {
        label = wrap("tray.item.alert", label, "{item}, alert active");
    }
    if view.on_extra {
        label = wrap("tray.item.extra", label, "{item}, using extra usage");
    }
    label
}

fn wrap(id: &'static str, inner: Localized, template: &'static str) -> Localized {
    Localized::with_params(id, params([("item", ParamValue::l10n(inner))]), template)
}

fn join(labels: Vec<Localized>) -> Option<Localized> {
    labels.into_iter().rev().reduce(|rest, first| {
        Localized::with_params(
            "tray.join",
            params([
                ("first", ParamValue::l10n(first)),
                ("rest", ParamValue::l10n(rest)),
            ]),
            "{first}; {rest}",
        )
    })
}

/// S-03: the label of `views`, at most [`LABEL_ITEMS`] accounts and then
/// "+N more".
fn bounded_label(views: &[&AccountView], clock: &Clock<'_>) -> Option<Localized> {
    let label = join(
        views
            .iter()
            .take(LABEL_ITEMS)
            .map(|v| item_label(v, clock))
            .collect(),
    )?;
    let more = views.len().saturating_sub(LABEL_ITEMS);
    Some(if more == 0 {
        label
    } else {
        Localized::with_params(
            "tray.more",
            params([
                ("item", ParamValue::l10n(label)),
                ("count", more.to_string().as_str().into()),
            ]),
            "{item}; +{count} more",
        )
    })
}

/// RF1.1: the menu-bar summary of `accounts` (dashboard order) for `style`
/// and `selection`. Times read in `tz` and the title's in `locale` (V-02:
/// the core owns the clock, so every front end writes them alike). `now`
/// drops return and reset times already past.
pub fn summary<Tz>(
    accounts: &[AccountView],
    style: TrayStyle,
    selection: &TraySelection,
    now: DateTime<Utc>,
    tz: &Tz,
    locale: &str,
) -> TraySummary
where
    Tz: TimeZone,
    Tz::Offset: Display,
{
    let short = |at: DateTime<Utc>| clock::clock_short(at, now, tz).resolve(locale);
    let full = |at: DateTime<Utc>| clock::clock(at, now, tz);
    let clock = Clock {
        short: &short,
        full: &full,
        locale,
    };
    let clock = &clock;
    let views: Vec<AccountView> = selection
        .pick(accounts)
        .into_iter()
        .map(|v| {
            let mut v = v.clone();
            // PRD 0031 re-validation S-04: names from any source (a local
            // server's answer included) reach a title or a terminal line
            // without control or direction characters.
            for text in [
                &mut v.name,
                &mut v.provider_label,
                &mut v.provider,
                &mut v.key,
            ] {
                if let std::borrow::Cow::Owned(clean) = crate::text::strip_disguising(text) {
                    *text = clean;
                }
            }
            v.resets_at = v.resets_at.filter(|t| *t > now);
            v.runs_out_at = v.runs_out_at.filter(|t| *t > now);
            v
        })
        .collect();
    let refs: Vec<&AccountView> = views.iter().collect();
    let cap = match (selection, style) {
        (TraySelection::Auto, TrayStyle::Meters) => AUTO_METERS,
        (TraySelection::Auto, TrayStyle::LogosPercent) => AUTO_LOGOS,
        (_, TrayStyle::Meters) => AUTO_METERS,
        _ => usize::MAX,
    };
    let shown: Vec<&AccountView> = refs.iter().copied().take(cap).collect();
    let mut hidden = refs.len() - shown.len();
    let alert = views.iter().any(|v| v.alert);
    let badge = if views.iter().any(|v| v.incident) {
        Badge::Outage
    } else if alert {
        Badge::Alert
    } else {
        Badge::None
    };
    let item = |v: &AccountView| TrayItem {
        key: v.key.clone(),
        provider: v.provider.clone(),
        name: display_name(v).to_owned(),
        remaining: if v.exhausted { Some(0.0) } else { v.remaining },
        level: if v.exhausted {
            Some(Level::Bad)
        } else {
            v.remaining.map(Level::of)
        },
        stale: v.stale,
        exhausted: v.exhausted,
        alert: v.alert,
        on_extra: v.on_extra,
        figure: figure(v, clock),
    };
    let (title, label_views): (Option<String>, Vec<&AccountView>) = match style {
        TrayStyle::Meters => (None, shown.clone()),
        TrayStyle::IconOnly => (None, shown.clone()),
        // PRD 0043 RF4.2: the image is the tightest account's last 24
        // hours; no title, the label names that account.
        TrayStyle::HourlyStrip => (None, tightest(&shown).into_iter().collect()),
        TrayStyle::LogosPercent if !shown.is_empty() => {
            let parts: Vec<String> = shown
                .iter()
                .map(|v| format!("{} {}", initial(v), figure(v, clock)))
                .collect();
            let (text, dropped) = fit_more(&parts, alert, hidden);
            hidden += dropped;
            (Some(text), shown[..shown.len() - dropped].to_vec())
        }
        TrayStyle::TightestPercent => match tightest(&shown) {
            Some(v) => {
                let fig = figure(v, clock);
                let name = title_name(v, selection);
                let named = fit_named(&fig, name, alert);
                let chip = v
                    .runs_out_at
                    .filter(|_| !v.exhausted && !v.stale)
                    .map(|t| fit(&[format!("↑ ≈{}", (clock.short)(t))], alert).0);
                let text = match chip {
                    Some(chip) if chip.chars().count() < named.chars().count() => chip,
                    _ => named,
                };
                (Some(text), shown.clone())
            }
            None if !shown.is_empty() => (Some(fit(&[UNKNOWN.to_owned()], alert).0), shown.clone()),
            None => (None, Vec::new()),
        },
        _ => (None, Vec::new()),
    };
    let items: Vec<TrayItem> = if style == TrayStyle::HourlyStrip {
        hidden = refs.len().saturating_sub(label_views.len());
        label_views.iter().map(|v| item(v)).collect()
    } else {
        shown.iter().map(|v| item(v)).collect()
    };
    let mut label = bounded_label(&label_views, clock)
        .unwrap_or_else(|| Localized::fixed("tray.empty", "No accounts in the menu bar"));
    if style == TrayStyle::HourlyStrip && !label_views.is_empty() {
        label = wrap(
            "tray.strip",
            label,
            "{item}; the last 24 hours as hourly bars",
        );
    }
    if badge == Badge::Outage {
        label = wrap("tray.outage", label, "{item}; provider incident");
    }
    let tooltip = Localized::with_params(
        "tray.tooltip",
        params([("label", ParamValue::l10n(label.clone()))]),
        "Cuotai — {label}",
    );
    TraySummary {
        style,
        items,
        hidden,
        title,
        badge,
        accessible_label: label,
        tooltip,
    }
}

/// PRD 0043 RF4.2: the account whose last 24 hours the strip shows: the
/// tightest of `selection` (as the `TightestPercent` title picks it).
pub fn strip_account(accounts: &[AccountView], selection: &TraySelection) -> Option<String> {
    let picked = selection.pick(accounts);
    tightest(&picked).map(|view| view.key.clone())
}

/// PRD 0043 RF4.2: the 24 hourly bars of a strip from a `Last24h`
/// rhythm, oldest first: the consumption of each hour (`None`, a gap,
/// never 0) and whether a reset happened in it.
pub fn strip_bars(rhythm: &crate::insights::Rhythm) -> Vec<StripBar> {
    rhythm
        .buckets
        .iter()
        .map(|bucket| StripBar {
            consumption: bucket.consumption,
            reset: bucket.reset,
        })
        .collect()
}

/// One hour of the strip.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct StripBar {
    pub consumption: Option<f64>,
    pub reset: bool,
}

#[cfg(test)]
#[path = "tray_tests.rs"]
mod tests;
