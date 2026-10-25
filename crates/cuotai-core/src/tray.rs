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

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::freshness::Freshness;
use crate::metrics;
use crate::text::{params, Localized, ParamValue};
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

/// RF1.1: the four menu-bar styles. `TightestPercent` is the default
/// (decision N).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrayStyle {
    Meters,
    LogosPercent,
    #[default]
    TightestPercent,
    IconOnly,
}

impl TrayStyle {
    pub const ALL: [Self; 4] = [
        Self::Meters,
        Self::LogosPercent,
        Self::TightestPercent,
        Self::IconOnly,
    ];

    pub fn slug(self) -> &'static str {
        match self {
            Self::Meters => "meters",
            Self::LogosPercent => "logos_percent",
            Self::TightestPercent => "tightest_percent",
            Self::IconOnly => "icon_only",
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
    pub fn of(remaining: f64) -> Self {
        if remaining >= 0.7 {
            Self::Good
        } else if remaining >= 0.3 {
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
    /// The account name (never an e-mail).
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
            name: account.name.clone(),
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
/// 1 % and never "100%" below 100 % (DS-CMP-MTR-4).
pub fn percent(remaining: f64) -> String {
    let p = remaining.clamp(0.0, 1.0) * 100.0;
    if p > 0.0 && p < 1.0 {
        "<1%".to_owned()
    } else if p < 100.0 && p.round() >= 100.0 {
        "99%".to_owned()
    } else {
        format!("{:.0}%", p.round())
    }
}

fn figure(view: &AccountView, at: &dyn Fn(DateTime<Utc>) -> String) -> String {
    let base = match (view.exhausted, view.remaining) {
        (true, _) => match view.resets_at {
            Some(when) => format!("0% → {}", at(when)),
            None => "0%".to_owned(),
        },
        (false, Some(r)) => percent(r),
        (false, None) => UNKNOWN.to_owned(),
    };
    if view.stale && view.remaining.is_some() {
        format!("{base}{STALE_MARK}")
    } else {
        base
    }
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
    let prefix = if alert { "! " } else { "" };
    let len = |s: &str| s.chars().count();
    for keep in (1..=parts.len()).rev() {
        let mut text = format!("{prefix}{}", parts[..keep].join(SEPARATOR));
        let dropped = parts.len() - keep;
        if dropped > 0 {
            text.push_str(&format!(" +{dropped}"));
        }
        if len(&text) <= TITLE_BUDGET {
            return (text, dropped);
        }
    }
    // Even one part does not fit: cut it (never the case for figures alone).
    let text: String = format!(
        "{prefix}{}",
        parts.first().map(String::as_str).unwrap_or("")
    )
    .chars()
    .take(TITLE_BUDGET)
    .collect();
    (text, parts.len().saturating_sub(1))
}

fn item_label(view: &AccountView, at: &dyn Fn(DateTime<Utc>) -> String) -> Localized {
    let name = view.name.as_str();
    let base = match (view.exhausted, view.remaining, view.resets_at) {
        (true, _, Some(when)) => {
            let at = at(when);
            Localized::with_params(
                "tray.item.exhausted_until",
                params([("name", name.into()), ("at", at.as_str().into())]),
                "{name} exhausted, returns {at}",
            )
        }
        (true, _, None) => {
            Localized::with_param("tray.item.exhausted", "name", name, "{name} exhausted")
        }
        (false, Some(r), Some(when)) => {
            let (pct, at) = (percent(r), at(when));
            Localized::with_params(
                "tray.item.left_resets",
                params([
                    ("name", name.into()),
                    ("pct", pct.as_str().into()),
                    ("at", at.as_str().into()),
                ]),
                "{name} {pct} left, resets {at}",
            )
        }
        (false, Some(r), None) => {
            let pct = percent(r);
            Localized::with_params(
                "tray.item.left",
                params([("name", name.into()), ("pct", pct.as_str().into())]),
                "{name} {pct} left",
            )
        }
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

/// RF1.1: the menu-bar summary of `accounts` (dashboard order) for `style`
/// and `selection`. `at` formats a time in the caller's clock ("19:30").
/// `now` drops return and reset times already past.
pub fn summary(
    accounts: &[AccountView],
    style: TrayStyle,
    selection: &TraySelection,
    now: DateTime<Utc>,
    at: impl Fn(DateTime<Utc>) -> String,
) -> TraySummary {
    let at: &dyn Fn(DateTime<Utc>) -> String = &at;
    let views: Vec<AccountView> = selection
        .pick(accounts)
        .into_iter()
        .map(|v| {
            let mut v = v.clone();
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
        name: v.name.clone(),
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
        figure: figure(v, at),
    };
    let (title, label_views): (Option<String>, Vec<&AccountView>) = match style {
        TrayStyle::Meters => (None, shown.clone()),
        TrayStyle::IconOnly => (None, shown.clone()),
        TrayStyle::LogosPercent if !shown.is_empty() => {
            let parts: Vec<String> = shown
                .iter()
                .map(|v| format!("{} {}", initial(v), figure(v, at)))
                .collect();
            let (text, dropped) = fit(&parts, alert);
            hidden += dropped;
            (Some(text), shown[..shown.len() - dropped].to_vec())
        }
        TrayStyle::TightestPercent => match tightest(&shown) {
            Some(v) => {
                let fig = figure(v, at);
                let percent_text = format!("{fig}{SEPARATOR}{}", v.name);
                let chip = v
                    .runs_out_at
                    .filter(|_| !v.exhausted && !v.stale)
                    .map(|t| format!("↑ ≈{}", at(t)));
                let text = match chip {
                    Some(chip) if chip.chars().count() < percent_text.chars().count() => chip,
                    _ => percent_text,
                };
                let (mut text, _) = fit(&[text], alert);
                if text.chars().count() > TITLE_BUDGET {
                    text = fit(&[fig], alert).0;
                }
                (Some(text), shown.clone())
            }
            None if !shown.is_empty() => (Some(fit(&[UNKNOWN.to_owned()], alert).0), shown.clone()),
            None => (None, Vec::new()),
        },
        _ => (None, Vec::new()),
    };
    let items: Vec<TrayItem> = shown.iter().map(|v| item(v)).collect();
    let mut label = join(label_views.iter().map(|v| item_label(v, at)).collect())
        .unwrap_or_else(|| Localized::fixed("tray.empty", "No accounts in the menu bar"));
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

#[cfg(test)]
#[path = "tray_tests.rs"]
mod tests;
