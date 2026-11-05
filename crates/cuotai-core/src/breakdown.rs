//! PRD 0045: breakdowns of an organizational account by API key, virtual
//! key, project, workspace or team.
//!
//! A breakdown is a set of sub-rows under one account, each for a
//! **sub-scope** the provider reports. It is never a per-user report: the
//! adapters never read user ids, names or e-mails into a row, labels go
//! through [`redact_label`] and ids are opaque ([`opaque_id`],
//! [`digest_id`]). The account opts in with `breakdown = ["keys"]`
//! ([`BreakdownKind`]); the values a provider accepts, and the extra
//! requests each one costs, come from its access descriptor
//! ([`crate::access::UsageAccess::breakdowns`]). Metered providers accept
//! none.
//!
//! Rows are capped at [`MAX_ROWS`] per account, ordered by the provider's
//! primary spend or usage; the rest are summed into one "Others (N)" row
//! ([`OTHERS_ID`]). A row carries at most [`MAX_ROW_METRICS`] metrics,
//! which are ordinary [`Metric`]s. A failing breakdown request never fails
//! the account: the report keeps its total and gains
//! [`unavailable_note`].

use crate::metric_text::metric_text;
use crate::text::{params, Localized, ParamValue};
use crate::{Metric, MetricKind, ScopeKind};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// RF1.2: most rows per account and refresh, the "Others" row included.
pub const MAX_ROWS: usize = 25;
/// RF1.2: most metrics per row.
pub const MAX_ROW_METRICS: usize = 6;
/// RF2–RF4: most pages a breakdown request follows.
pub const MAX_PAGES: usize = 3;
/// RF6.2: rows `--once` prints under an account.
pub const ONCE_ROWS: usize = 5;
/// RF6.1: rows named on the card's "Top:" line.
pub const CARD_ROWS: usize = 2;
/// RF1.1: longest row id.
pub const ID_MAX: usize = 64;
/// RF1.1: longest row label, in characters.
pub const LABEL_MAX: usize = 48;
/// RF1.2: the id of the row that sums the rows past the cap.
pub const OTHERS_ID: &str = "others";

/// RF1.4: one value of an account's `breakdown = [...]` array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakdownKind {
    /// API keys (OpenRouter keys, OpenAI API keys) or LiteLLM virtual keys.
    Keys,
    Projects,
    Workspaces,
    Teams,
}

impl BreakdownKind {
    pub const ALL: [BreakdownKind; 4] = [Self::Keys, Self::Projects, Self::Workspaces, Self::Teams];

    /// The TOML value (`"keys"`).
    pub fn slug(self) -> &'static str {
        match self {
            Self::Keys => "keys",
            Self::Projects => "projects",
            Self::Workspaces => "workspaces",
            Self::Teams => "teams",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|kind| kind.slug() == value.trim())
    }

    /// `breakdown.kind.*`: the option label of the editors.
    pub fn label(self) -> Localized {
        let text = match self {
            Self::Keys => "API keys",
            Self::Projects => "Projects",
            Self::Workspaces => "Workspaces",
            Self::Teams => "Teams",
        };
        Localized::fixed(format!("breakdown.kind.{}", self.slug()), text)
    }
}

/// RF1.1: one sub-scope of an account and what the provider reports for it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BreakdownRow {
    /// What the row is: a key, a virtual key, a project, a workspace or a
    /// team (never `model`, which belongs to [`Metric::scope`]).
    pub scope: ScopeKind,
    /// Opaque, at most [`ID_MAX`] characters: the provider's id when it is
    /// already opaque, a digest otherwise; [`OTHERS_ID`] for the sum row.
    pub id: String,
    /// At most [`LABEL_MAX`] characters, redacted by [`redact_label`].
    pub label: String,
    /// At most [`MAX_ROW_METRICS`].
    pub metrics: Vec<Metric>,
    /// Whether the provider reported more metrics than the row keeps.
    #[serde(default)]
    pub truncated: bool,
    /// Row-level notes (`note.breakdown.disabled`), English fallbacks with
    /// ids. Additive and skipped when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<Localized>,
}

impl BreakdownRow {
    /// Whether this is the "Others (N)" row.
    pub fn is_others(&self) -> bool {
        self.id == OTHERS_ID
    }

    /// The label to show: "Others (N)" through the catalog for the sum row,
    /// the redacted provider name verbatim otherwise.
    pub fn label_text(&self) -> Localized {
        match self.others_count() {
            Some(count) => others_label(count),
            None => metric_text(&self.label),
        }
    }

    fn others_count(&self) -> Option<usize> {
        if !self.is_others() {
            return None;
        }
        self.label
            .strip_prefix("Others (")
            .and_then(|rest| rest.strip_suffix(')'))
            .and_then(|count| count.parse().ok())
    }

    /// The index of the metric that orders the rows: the first `Spend`, else
    /// the first `Quota`, else the first `Usage` with a value.
    pub fn primary_index(&self) -> Option<usize> {
        [MetricKind::Spend, MetricKind::Quota, MetricKind::Usage]
            .into_iter()
            .find_map(|kind| {
                self.metrics
                    .iter()
                    .position(|metric| metric.kind == kind && metric.used.is_some())
            })
    }

    /// The primary metric ([`Self::primary_index`]).
    pub fn primary(&self) -> Option<&Metric> {
        self.primary_index().map(|index| &self.metrics[index])
    }

    /// The consumed fraction of the row's first capped metric (a `Quota`
    /// with a positive finite limit), for sorting by percentage and for the
    /// meter; `None` when no metric has a cap.
    pub fn fraction(&self) -> Option<f64> {
        self.capped().and_then(|metric| {
            let limit = metric.limit?;
            let used = metric
                .used
                .or_else(|| metric.remaining.map(|remaining| limit - remaining))?;
            (limit.is_finite() && limit > 0.0 && used.is_finite()).then(|| used / limit)
        })
    }

    /// The row's first `Quota` with a positive finite limit.
    pub fn capped(&self) -> Option<&Metric> {
        self.metrics.iter().find(|metric| {
            metric.kind == MetricKind::Quota
                && metric
                    .limit
                    .is_some_and(|limit| limit.is_finite() && limit > 0.0)
        })
    }
}

/// `breakdown.others`: "Others (N)".
pub fn others_label(count: usize) -> Localized {
    Localized::with_param(
        "breakdown.others",
        "count",
        count as u64,
        "Others ({count})",
    )
}

/// RF6.1: how the details sort the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortBy {
    /// Primary spend or usage, highest first (the provider's order).
    Spend,
    /// Consumed fraction of the row's cap, highest first; rows without a
    /// cap after the others, in spend order.
    Percent,
}

/// RF6.1: `rows` in the order `by` asks, the "Others" row always last.
pub fn sorted(rows: &[BreakdownRow], by: SortBy) -> Vec<&BreakdownRow> {
    let mut out: Vec<&BreakdownRow> = rows.iter().collect();
    let amount = |row: &BreakdownRow| row.primary().and_then(|m| m.used);
    out.sort_by(|a, b| {
        a.is_others().cmp(&b.is_others()).then_with(|| {
            let by_amount = desc(amount(a), amount(b));
            match by {
                SortBy::Spend => by_amount,
                SortBy::Percent => desc(a.fraction(), b.fraction()).then(by_amount),
            }
        })
    });
    out
}

/// Highest first, `None` last.
fn desc(a: Option<f64>, b: Option<f64>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => b.total_cmp(&a),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// RF6.1: the card's one line, "Top: Proj-A $412 · Proj-B $298", from the
/// first `count` rows with a primary amount (never the "Others" row);
/// `None` without such a row.
pub fn top_line(rows: &[BreakdownRow], count: usize) -> Option<Localized> {
    let items: Vec<String> = sorted(rows, SortBy::Spend)
        .into_iter()
        .filter(|row| !row.is_others())
        .filter_map(|row| {
            let metric = row.primary()?;
            Some(format!(
                "{} {}",
                row.label,
                amount_text(metric.used?, &metric.unit)
            ))
        })
        .take(count)
        .collect();
    if items.is_empty() {
        return None;
    }
    Some(Localized::with_param(
        "breakdown.top",
        "items",
        items.join(" · "),
        "Top: {items}",
    ))
}

/// "$412", "$0.25", "1.2k tokens": a compact amount for the card line.
pub fn amount_text(value: f64, unit: &str) -> String {
    let compact = |value: f64| {
        if value.abs() >= 1000.0 {
            format!("{:.1}k", value / 1000.0)
        } else if value.abs() >= 100.0 || value.fract() == 0.0 {
            format!("{value:.0}")
        } else {
            format!("{value:.2}")
        }
    };
    match unit {
        "USD" => format!("${}", compact(value)),
        unit => format!("{} {}", compact(value), metric_text(unit).text()),
    }
}

/// RF6.1: "Showing the top 24 of 40; the rest are summed in Others." when
/// the rows were capped; `None` otherwise.
pub fn truncated_note(rows: &[BreakdownRow]) -> Option<Localized> {
    let others = rows.iter().find_map(BreakdownRow::others_count)?;
    let shown = rows.iter().filter(|row| !row.is_others()).count();
    Some(Localized::with_params(
        "breakdown.truncated",
        params([
            ("shown", ParamValue::from(shown as u64)),
            ("total", ParamValue::from((shown + others) as u64)),
        ]),
        "Showing the top {shown} of {total}; the rest are summed in Others.",
    ))
}

// ------------------------------------------------------------------ privacy

/// RF1.1: `raw` when it is already an opaque, short identifier (letters,
/// digits, `_ - . :`, at most [`ID_MAX`] characters, not shaped like a
/// secret), otherwise its [`digest_id`].
pub fn opaque_id(raw: &str) -> String {
    let raw = raw.trim();
    let plain = (1..=ID_MAX).contains(&raw.len())
        && raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':'));
    if plain && !secret_like(raw) {
        raw.to_owned()
    } else {
        digest_id(raw)
    }
}

/// RF1.1, §6: a truncated SHA-256 of `raw` (16 hex characters), for ids
/// that are key hashes or otherwise must not leave the adapter.
pub fn digest_id(raw: &str) -> String {
    Sha256::digest(raw.trim().as_bytes())
        .iter()
        .take(8)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Prefixes of credentials (`sk-…`, `sk-or-…`, `sk-ant-…`, `or-…`).
const KEY_PREFIXES: [&str; 6] = ["sk-", "sk_", "or-", "pk-", "rk-", "key-"];

/// Whether `word` looks like a credential: a known key prefix followed by
/// at least six more characters, or a long run (32 or more) of letters and
/// digits.
fn secret_like(word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    let prefixed = KEY_PREFIXES
        .iter()
        .any(|prefix| lower.starts_with(prefix) && word.len() >= prefix.len() + 6);
    let long_run = word
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|run| run.len() >= 32);
    prefixed || long_run
}

/// The last four letters or digits of `text`, for "…ab12".
fn last4(text: &str) -> String {
    let tail: Vec<char> = text
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .take(4)
        .collect();
    tail.into_iter().rev().collect()
}

/// The generic label of a row without a usable name: "Key …ab12".
pub fn fallback_label(scope: ScopeKind, id: &str) -> String {
    let noun = match scope {
        ScopeKind::Key => "Key",
        ScopeKind::VirtualKey => "Virtual key",
        ScopeKind::Project => "Project",
        ScopeKind::Workspace => "Workspace",
        ScopeKind::Team => "Team",
        ScopeKind::Model => "Model",
    };
    match last4(id) {
        tail if tail.is_empty() => noun.to_owned(),
        tail => format!("{noun} …{tail}"),
    }
}

/// RF1.3: the label a row may show. Control characters go; e-mail
/// addresses and URLs are removed (a label that is only an e-mail becomes
/// [`fallback_label`]); a word shaped like a key (`sk-…`, `or-…`, long
/// random runs) is replaced by "…" and its last four characters; the
/// result is cut at [`LABEL_MAX`] characters. Empty input gives
/// [`fallback_label`].
pub fn redact_label(raw: &str, scope: ScopeKind, id: &str) -> String {
    let clean: String = raw
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut words = Vec::new();
    for word in clean.split_whitespace() {
        let trimmed = word.trim_matches(|c: char| "\"'<>()[]{},;".contains(c));
        if crate::export::v1::is_email(trimmed) {
            continue;
        }
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            continue;
        }
        if trimmed.contains('@') {
            // A partial address ("name@host") is personal data too.
            continue;
        }
        if secret_like(trimmed) {
            words.push(format!("…{}", last4(trimmed)));
            continue;
        }
        words.push(word.to_owned());
    }
    let joined = words.join(" ");
    let joined = joined.trim();
    if joined.is_empty() {
        return fallback_label(scope, id);
    }
    if joined.chars().count() <= LABEL_MAX {
        return joined.to_owned();
    }
    let mut cut: String = joined.chars().take(LABEL_MAX - 1).collect();
    cut = cut.trim_end().to_owned();
    cut.push('…');
    cut
}

// ---------------------------------------------------------------- assembly

/// RF1.2: `rows` ordered by their primary amount (highest first, rows
/// without one last, ties by label), each cut to [`MAX_ROW_METRICS`]
/// metrics, and past [`MAX_ROWS`] the tail summed into one "Others (N)"
/// row: per unit, the sum of the folded rows' primary amounts (as `Spend`
/// for money, `Usage` otherwise, without limit or remaining).
pub(crate) fn finish(mut rows: Vec<BreakdownRow>) -> Vec<BreakdownRow> {
    for row in &mut rows {
        if row.metrics.len() > MAX_ROW_METRICS {
            row.metrics.truncate(MAX_ROW_METRICS);
            row.truncated = true;
        }
        if row.id.len() > ID_MAX || row.id.is_empty() {
            row.id = digest_id(&row.id);
        }
    }
    let amount = |row: &BreakdownRow| row.primary().and_then(|metric| metric.used);
    rows.sort_by(|a, b| desc(amount(a), amount(b)).then_with(|| a.label.cmp(&b.label)));
    if rows.len() <= MAX_ROWS {
        return rows;
    }
    let folded = rows.split_off(MAX_ROWS - 1);
    let mut sums: Vec<Metric> = Vec::new();
    for metric in folded.iter().filter_map(BreakdownRow::primary) {
        let Some(used) = metric.used.filter(|used| used.is_finite()) else {
            continue;
        };
        match sums
            .iter_mut()
            .find(|sum| sum.unit == metric.unit && sum.label == metric.label)
        {
            Some(sum) => sum.used = sum.used.map(|total| total + used),
            None => {
                let kind = if metric.unit == "USD" || metric.kind == MetricKind::Spend {
                    MetricKind::Spend
                } else {
                    MetricKind::Usage
                };
                sums.push(Metric {
                    used: Some(used),
                    limit: None,
                    remaining: None,
                    kind,
                    scope: None,
                    ..metric.clone()
                });
            }
        }
    }
    let scope = folded.first().map_or(ScopeKind::Key, |row| row.scope);
    let truncated = sums.len() > MAX_ROW_METRICS;
    sums.truncate(MAX_ROW_METRICS);
    rows.push(BreakdownRow {
        scope,
        id: OTHERS_ID.into(),
        label: format!("Others ({})", folded.len()),
        metrics: sums,
        truncated,
        notes: Vec::new(),
    });
    rows
}

/// `note.breakdown.disabled`: a key the provider reports as disabled.
pub(crate) fn disabled_note() -> Localized {
    Localized::fixed(
        "note.breakdown.disabled",
        "Disabled on the provider: it cannot be used until it is enabled again.",
    )
}

/// §6 Resilience: the account note of a breakdown request that failed,
/// with the failure class; the main report stands.
pub(crate) fn unavailable_note(error: &crate::providers::QueryError) -> Localized {
    use crate::Status;
    let (slug, text) = match error.status {
        Status::AuthenticationError | Status::NeedsConfiguration => {
            ("auth", "the credential may not read it")
        }
        Status::RateLimited => ("rate_limited", "rate limited"),
        _ if error.note.id == crate::providers::SCHEMA_NOTE => ("schema", "unexpected answer"),
        _ if error.note.id == crate::providers::TRANSPORT_NOTE => {
            ("transport", "network failure or timeout")
        }
        _ => ("other", "query failed"),
    };
    Localized::with_param(
        "note.breakdown.unavailable",
        "class",
        Localized::fixed(format!("breakdown.error.{slug}"), text),
        "The breakdown is unavailable right now ({class}); the account total is still correct.",
    )
}

/// RF2–RF4: "Showing the first N rows the provider returned in 3 pages."
pub(crate) fn paged_note() -> Localized {
    Localized::with_param(
        "note.breakdown.paged",
        "pages",
        MAX_PAGES as u64,
        "The provider has more rows than {pages} pages; the breakdown covers the first ones.",
    )
}

/// Adds the row-level notes, in order, to an adapter's notes.
pub(crate) fn push_notes(notes: &mut crate::text::Notes, list: Vec<Localized>) {
    for note in list {
        notes.push_note(note);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MetricWindow;

    fn spend(label: &str, used: f64) -> Metric {
        Metric::usage(
            label,
            used,
            "USD",
            MetricKind::Spend,
            Some(MetricWindow::Monthly),
        )
    }

    fn row(id: &str, used: Option<f64>) -> BreakdownRow {
        BreakdownRow {
            scope: ScopeKind::Project,
            id: id.into(),
            label: format!("Proj-{id}"),
            metrics: used
                .map(|used| spend("Gasto no mês", used))
                .into_iter()
                .collect(),
            truncated: false,
            notes: Vec::new(),
        }
    }

    #[test]
    fn kinds_round_trip_in_snake_case() {
        for kind in BreakdownKind::ALL {
            let json = serde_json::to_string(&kind).unwrap();
            assert_eq!(json, format!("\"{}\"", kind.slug()));
            assert_eq!(serde_json::from_str::<BreakdownKind>(&json).unwrap(), kind);
            assert_eq!(BreakdownKind::parse(kind.slug()), Some(kind));
        }
        assert_eq!(BreakdownKind::parse("users"), None);
        for (scope, slug) in [
            (ScopeKind::Key, "key"),
            (ScopeKind::VirtualKey, "virtual_key"),
            (ScopeKind::Project, "project"),
            (ScopeKind::Workspace, "workspace"),
            (ScopeKind::Team, "team"),
        ] {
            assert_eq!(
                serde_json::to_string(&scope).unwrap(),
                format!("\"{slug}\"")
            );
        }
    }

    #[test]
    fn a_row_round_trips_through_json() {
        let mut original = row("a", Some(12.5));
        original.notes.push(disabled_note());
        let json = serde_json::to_string(&original).unwrap();
        let back: BreakdownRow = serde_json::from_str(&json).unwrap();
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
        // `notes` is skipped when empty.
        let plain = serde_json::to_value(row("b", None)).unwrap();
        assert!(plain.get("notes").is_none());
        assert_eq!(plain["truncated"], false);
    }

    #[test]
    fn rows_are_ordered_and_capped_with_others() {
        let rows: Vec<_> = (0..40)
            .map(|i| row(&format!("p{i:02}"), Some(f64::from(i))))
            .collect();
        let done = finish(rows);
        assert_eq!(done.len(), MAX_ROWS);
        assert_eq!(done[0].id, "p39");
        let others = done.last().unwrap();
        assert!(others.is_others());
        assert_eq!(others.label, "Others (16)");
        // 0 + 1 + … + 15
        assert_eq!(others.metrics[0].used, Some(120.0));
        assert_eq!(others.label_text().text(), "Others (16)");
        assert_eq!(
            truncated_note(&done).unwrap().text(),
            "Showing the top 24 of 40; the rest are summed in Others."
        );
        // Under the cap nothing is folded and no note is written.
        let small = finish((0..5).map(|i| row(&i.to_string(), Some(1.0))).collect());
        assert_eq!(small.len(), 5);
        assert!(truncated_note(&small).is_none());
    }

    #[test]
    fn rows_without_an_amount_go_last_and_metrics_are_capped() {
        let mut wide = row("wide", Some(1.0));
        wide.metrics = (0..9).map(|i| spend(&format!("m{i}"), 1.0)).collect();
        let done = finish(vec![row("none", None), wide, row("big", Some(5.0))]);
        assert_eq!(
            done.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["big", "wide", "none"]
        );
        assert_eq!(done[1].metrics.len(), MAX_ROW_METRICS);
        assert!(done[1].truncated);
        assert!(!done[0].truncated);
    }

    #[test]
    fn redaction_table() {
        let id = "proj_abc123";
        for (raw, expected) in [
            ("Proj-A", "Proj-A"),
            ("  Team\u{7}  Blue ", "Team Blue"),
            ("dev@example.com", "Project …c123"),
            ("ci key (dev@example.com)", "ci key"),
            ("see https://example.com/x?token=1 now", "see now"),
            ("sk-or-v1-0123456789abcdef", "…cdef"),
            ("prod sk-ant-api03-XYZWabcd", "prod …abcd"),
            ("or-abcdef123456", "…3456"),
            ("", "Project …c123"),
            (
                "a label that is far too long to be shown on a single row as is",
                "a label that is far too long to be shown on a s…",
            ),
        ] {
            assert_eq!(redact_label(raw, ScopeKind::Project, id), expected, "{raw}");
        }
        assert_eq!(fallback_label(ScopeKind::Key, ""), "Key");
        assert!(
            redact_label(&"x".repeat(100), ScopeKind::Key, "k")
                .chars()
                .count()
                <= LABEL_MAX
        );
    }

    #[test]
    fn ids_are_opaque() {
        assert_eq!(opaque_id("proj_abc"), "proj_abc");
        assert_eq!(opaque_id("wrkspc_01Abc"), "wrkspc_01Abc");
        let hashed = opaque_id("sk-or-v1-secretsecretsecret");
        assert_eq!(hashed.len(), 16);
        assert!(!hashed.contains("secret"));
        assert_eq!(opaque_id(&"a".repeat(80)).len(), 16);
        assert_eq!(digest_id("x"), digest_id(" x "));
    }

    #[test]
    fn sorting_by_percent_and_the_card_line() {
        let capped = |id: &str, used: f64, limit: f64| BreakdownRow {
            metrics: vec![Metric {
                limit: Some(limit),
                remaining: Some(limit - used),
                ..Metric::usage("Limite da chave", used, "USD", MetricKind::Quota, None)
            }],
            ..row(id, None)
        };
        let rows = vec![
            capped("a", 90.0, 100.0),
            capped("b", 50.0, 50.0),
            row("c", Some(412.0)),
        ];
        let by_spend: Vec<_> = sorted(&rows, SortBy::Spend)
            .iter()
            .map(|r| r.id.clone())
            .collect();
        assert_eq!(by_spend, ["c", "a", "b"]);
        let by_percent: Vec<_> = sorted(&rows, SortBy::Percent)
            .iter()
            .map(|r| r.id.clone())
            .collect();
        assert_eq!(by_percent, ["b", "a", "c"]);
        assert_eq!(
            top_line(&rows, CARD_ROWS).unwrap().text(),
            "Top: Proj-c $412 · Proj-a $90"
        );
        assert!(top_line(&[], CARD_ROWS).is_none());
        assert_eq!(rows[1].fraction(), Some(1.0));
    }

    #[test]
    fn the_unavailable_note_names_the_class_only() {
        let note = unavailable_note(&crate::providers::QueryError::schema());
        assert_eq!(note.id, "note.breakdown.unavailable");
        assert!(note.text().contains("unexpected answer"));
    }
}

/// RF1.4 and RF7: the account field, its validation per provider and the
/// descriptors behind it.
#[cfg(test)]
mod config_tests {
    use super::*;
    use crate::{Account, Config, ConfigError, Provider};

    fn toml(provider: &str, extra: &str) -> String {
        format!(
            "[[accounts]]\nname = \"a\"\nprovider = \"{provider}\"\ncredential_env = \"X_KEY\"\n{extra}\n"
        )
    }

    #[test]
    fn the_field_round_trips_through_toml() {
        let config = Config::from_toml(&toml(
            "open_router",
            "params = { product = \"account\" }\nbreakdown = [\"keys\"]",
        ))
        .unwrap();
        assert_eq!(config.accounts[0].breakdown, [BreakdownKind::Keys]);
        let written = ::toml::to_string(&config).unwrap();
        assert!(written.contains("breakdown = [\"keys\"]"), "{written}");
        // An account without breakdowns writes no key.
        let plain = Config::from_toml(&toml("open_ai_api", "")).unwrap();
        assert!(plain.accounts[0].breakdown.is_empty());
        assert!(!::toml::to_string(&plain).unwrap().contains("breakdown"));
    }

    #[test]
    fn validation_follows_each_provider_descriptor() {
        for (provider, extra, ok) in [
            (
                "open_router",
                "params = { product = \"account\" }\nbreakdown = [\"keys\"]",
                true,
            ),
            // The inference-key view has no breakdown: its key is the account.
            ("open_router", "breakdown = [\"keys\"]", false),
            ("litellm", "breakdown = [\"keys\", \"teams\"]", true),
            ("litellm", "breakdown = [\"projects\"]", false),
            ("open_ai_api", "breakdown = [\"projects\", \"keys\"]", true),
            ("open_ai_api", "breakdown = [\"workspaces\"]", false),
            ("anthropic_api", "breakdown = [\"workspaces\"]", true),
            (
                "anthropic_api",
                "breakdown = [\"workspaces\", \"workspaces\"]",
                false,
            ),
            // Per-user seat data only: excluded by design.
            (
                "copilot",
                "params = { mode = \"org\", org = \"acme\" }\nbreakdown = [\"teams\"]",
                false,
            ),
            ("cursor", "breakdown = [\"teams\"]", false),
        ] {
            let result = Config::from_toml(&toml(provider, extra));
            assert_eq!(result.is_ok(), ok, "{provider} {extra}: {result:?}");
            if !ok {
                if let Err(ConfigError::Breakdown { field }) = &result {
                    assert!(field.starts_with("accounts[0].breakdown["), "{field}");
                }
            }
        }
        // A value no release knows is a parse error naming the key path.
        assert!(matches!(
            Config::from_toml(&toml("litellm", "breakdown = [\"users\"]")),
            Err(ConfigError::Parse { .. })
        ));
    }

    #[test]
    fn metered_providers_refuse_every_breakdown() {
        let account = Account {
            provider: Provider::AwsBedrock,
            params: [("profile".to_string(), "p".to_string())].into(),
            breakdown: vec![BreakdownKind::Projects],
            ..Account::default()
        };
        let access = crate::access::describe(&account);
        assert!(access.metered && access.breakdowns.is_empty());
        let config = Config {
            accounts: vec![account.clone()],
            ..Config::default()
        };
        assert!(matches!(
            config.validate(),
            Err(ConfigError::Breakdown { .. })
        ));
        let options = crate::account_edit::breakdown_options(&access);
        assert_eq!(options.reason.unwrap().id, "access.breakdown_metered");
        let mut edited = Account {
            breakdown: Vec::new(),
            ..account
        };
        let error = crate::account_edit::set_breakdown(&mut edited, &[BreakdownKind::Projects])
            .unwrap_err();
        assert_eq!(error.field(), Some("breakdown"));
        assert!(edited.breakdown.is_empty());
    }

    #[test]
    fn set_breakdown_orders_and_deduplicates() {
        let mut account = Account {
            provider: Provider::LiteLlm,
            ..Account::default()
        };
        crate::account_edit::set_breakdown(
            &mut account,
            &[
                BreakdownKind::Teams,
                BreakdownKind::Keys,
                BreakdownKind::Teams,
            ],
        )
        .unwrap();
        assert_eq!(
            account.breakdown,
            [BreakdownKind::Keys, BreakdownKind::Teams]
        );
        crate::account_edit::set_breakdown(&mut account, &[]).unwrap();
        assert!(account.breakdown.is_empty());
        let none = crate::account_edit::breakdown_options(&crate::access::describe(&Account {
            provider: Provider::Claude,
            ..Account::default()
        }));
        assert_eq!(none.reason.unwrap().id, "access.breakdown_none");
    }

    #[test]
    fn every_breakdown_mode_is_official_documented_and_bounded() {
        let modes = [
            (Provider::OpenRouter, vec![("product", "account")]),
            (Provider::LiteLlm, vec![]),
            (Provider::OpenAiApi, vec![]),
            (Provider::AnthropicApi, vec![]),
        ];
        for (provider, params) in modes {
            let account = Account {
                provider,
                params: params
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect(),
                ..Account::default()
            };
            let access = crate::access::describe(&account);
            assert!(!access.breakdowns.is_empty(), "{provider:?}");
            for mode in &access.breakdowns {
                assert_eq!(mode.level, crate::access::Level::Official);
                assert!(mode.evidence.url().starts_with("https://"));
                assert!((1..=4).contains(&mode.requests), "{provider:?}");
                assert_ne!(mode.row, crate::ScopeKind::Model);
                assert!(mode.summary.id.starts_with("access.breakdown."));
                for locale in crate::text::catalog_locales() {
                    let catalog = crate::text::core_catalog(locale).unwrap();
                    assert!(
                        catalog.get(&*mode.summary.id).is_some(),
                        "{locale} {}",
                        mode.summary.id
                    );
                }
            }
        }
        // No other provider offers one.
        let offered: Vec<_> = Provider::ALL
            .into_iter()
            .filter(|provider| {
                [false, true].into_iter().any(|oauth| {
                    let params = [("product".to_string(), "account".to_string())].into();
                    !crate::access::classify(*provider, oauth, &params)
                        .breakdowns
                        .is_empty()
                        || !crate::access::classify(*provider, oauth, &Default::default())
                            .breakdowns
                            .is_empty()
                })
            })
            .collect();
        assert_eq!(
            offered,
            [
                Provider::OpenAiApi,
                Provider::AnthropicApi,
                Provider::OpenRouter,
                Provider::LiteLlm
            ]
        );
    }

    #[test]
    fn an_edit_keeps_only_the_breakdowns_the_new_mode_offers() {
        use crate::account_edit::{build_account, AccountDraft, DraftField, DraftValue};
        let config = Config {
            accounts: vec![Account {
                name: "proxy".into(),
                provider: Provider::LiteLlm,
                credential_env: Some("LITELLM_KEY".into()),
                breakdown: vec![BreakdownKind::Keys, BreakdownKind::Teams],
                ..Account::default()
            }],
            ..Config::default()
        };
        config.validate().unwrap();
        let fields = [DraftField {
            key: "base_url",
            value: DraftValue::Text("http://127.0.0.1:4000".into()),
        }];
        let draft = |provider, credential| AccountDraft {
            provider,
            name: "proxy",
            credential,
            auth_mode: "credential",
            oauth_mode: false,
            oauth_existing: None,
            oauth_authorized: None,
            oauth_login_params: Default::default(),
            existing_params: None,
            credential_params: None,
            fields: &fields,
            interval_text: "",
            group_text: None,
        };
        let kept =
            build_account(&draft(Provider::LiteLlm, "LITELLM_KEY"), &config, Some(0)).unwrap();
        assert_eq!(kept.breakdown, [BreakdownKind::Keys, BreakdownKind::Teams]);
        // A provider switch starts without breakdowns.
        let switched = build_account(
            &draft(Provider::OpenAiApi, "OPENAI_ADMIN_KEY"),
            &config,
            Some(0),
        )
        .unwrap();
        assert!(switched.breakdown.is_empty());
        // A new account starts without any.
        let fresh = build_account(
            &draft(Provider::LiteLlm, "LITELLM_KEY"),
            &Config::default(),
            None,
        );
        assert!(fresh.is_ok_and(|account| account.breakdown.is_empty()));
    }
}
