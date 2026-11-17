//! PRD 0035 re-validation V35-12 (design G8): a count text is a plural.
//!
//! In a language whose plural rule has more than one category (one/other,
//! or Russian's one/few/many/other), a text that shows `{count}` as a plain
//! string reads wrong for some numbers ("21 командам" instead of "21
//! команде"). So, per layer (core, TUI, desktop UI): every id whose value
//! carries `{count}` in any catalog of that layer must be a plural object
//! in every catalog of a language with several categories. The core's
//! English source lives in the code (`Localized::plural`), so the core
//! check covers the thirteen translated catalogs; the parity tests keep the
//! plural shape the same across them.
//!
//! The allow-lists below hold the ids that legitimately keep a plain text,
//! each with its reason, and (marked FOLLOW-UP) the count texts found when
//! the guard was added whose code or catalogs belong to another change. An
//! id there that no longer needs the exception fails the test, so a list
//! cannot go stale.

use super::*;
use std::collections::BTreeSet;

// Legitimate exceptions: the number stands without a noun to inflect.
// Kinds, named in each reason: a colon form ("files: {count}", the noun is
// a label, not inflected by the number), a number in parentheses or after
// a comma (a total beside a label), a bare "{count} more" (no noun), and
// "of {count}" (the noun, if any, belongs to another number).

/// Core ids that legitimately keep a plain `{count}` text.
const CORE_ALLOWED: &[(&str, &str)] = &[
    (
        "access.breakdown_requests",
        "colon form: \"requests per refresh: up to {count}\"",
    ),
    ("alerts.item.and_count", "symbol: \"{first} +{count}\""),
    (
        "breakdown.others",
        "number in parentheses: \"Others ({count})\"",
    ),
    ("cost.by.more", "bare \"{count} more\", no noun"),
    (
        "digest.line.budgets",
        "colon form: \"Budgets: {over} of {count}\"",
    ),
    ("doctor.config.valid", "colon form: \"accounts: {count}\""),
    ("doctor.hooks.ok", "colon form: \"rules: {count}\""),
    (
        "edit_error.index_out_of_range",
        "a length in parentheses: \"({count})\"",
    ),
    (
        "history.relocation.adopted",
        "colon form: \"readings: {count}\"",
    ),
    (
        "history.relocation.prompt",
        "colon form: \"(readings: {count}; accounts: …)\"",
    ),
    (
        "note.alibaba_plans.model_studio_free_tier_more",
        "\"of {count}\": the noun goes with {shown}, a fixed number",
    ),
    ("tray.more", "bare \"{item}; {count} more\", no noun"),
];

/// Core ids that should become plurals; their code is outside this change.
const CORE_FOLLOW_UP: &[(&str, &str)] = &[(
    "doctor.trust_store.ok",
    "FOLLOW-UP: \"{count} certificates\" (de \"Zertifikate\"); `doctor.rs` needs `Localized::plural`",
)];

/// TUI chrome ids that legitimately keep a plain `{count}` text.
const TUI_ALLOWED: &[(&str, &str)] = &[
    (
        "cli.restore.done",
        "colon form: \"Files restored: {count}\"",
    ),
    (
        "once.breakdown.more",
        "bare \"… {count} more in the details\", no noun",
    ),
];

/// TUI chrome ids that should become plurals (the TUI catalogs and code
/// are outside this change).
const TUI_FOLLOW_UP: &[(&str, &str)] = &[
    (
        "cli.history_erased",
        "FOLLOW-UP: \"{count} history samples erased\"",
    ),
    ("history.metric", "FOLLOW-UP: \"{count} measurements\""),
    ("login.code.footer", "FOLLOW-UP: \"[{count} characters]\""),
    (
        "wizard.session.accepted",
        "FOLLOW-UP: \"({count} metrics)\"",
    ),
];

/// Desktop UI ids that legitimately keep a plain `{count}` text.
const UI_ALLOWED: &[(&str, &str)] = &[
    ("models.meta.filtered", "\"{shown} of {count}\", no noun"),
    ("settings.channels.count", "colon form: \"On: {count}\""),
    (
        "settings.vault.prune",
        "number in parentheses: \"Remove unused ({count})\"",
    ),
    (
        "stats.rhythm.samples",
        "colon form: \"days with data: {count}\"",
    ),
    (
        "tabs.allAria",
        "a total after a comma: \"All accounts, {count}, …\"",
    ),
    (
        "tabs.allAriaNoPct",
        "a total after a comma: \"All accounts, {count}\"",
    ),
    (
        "tray.catTitle",
        "number in parentheses: \"{label} ({count}):\"",
    ),
    ("tray.more", "bare \"and {count} more…\", no noun"),
    (
        "userContent.protected",
        "number in parentheses: \"were not used ({count})\"",
    ),
];

/// Desktop UI ids that should become plurals (the UI catalogs and
/// `apps/desktop/ui/js` are outside this change).
const UI_FOLLOW_UP: &[(&str, &str)] = &[
    ("backup.done", "FOLLOW-UP: \"({count} files)\""),
    ("dialog.session.testOk", "FOLLOW-UP: \"({count} metrics)\""),
    ("restore.done", "FOLLOW-UP: \"Restored: {count} files\""),
    (
        "settings.history.erased",
        "FOLLOW-UP: \"{count} samples erased\"",
    ),
    (
        "settings.history.exported",
        "FOLLOW-UP: \"{count} samples exported\"",
    ),
];

/// The catalogs of one layer, by language, read from disk.
fn layer(dir: &str) -> Vec<(&'static LocaleRow, Catalog)> {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(dir);
    LOCALES
        .iter()
        .filter_map(|row| {
            let raw = std::fs::read_to_string(base.join(format!("{}.json", row.code))).ok()?;
            Some((row, Catalog::parse(&raw).unwrap()))
        })
        .collect()
}

/// The problems of one layer: an id with `{count}` that is a plain text in
/// a language with several plural categories, or an allow-list entry that
/// no longer matches such an id.
fn problems(
    name: &str,
    catalogs: &[(&'static LocaleRow, Catalog)],
    allowed: &[(&str, &str)],
) -> Vec<String> {
    let counted: BTreeSet<&str> = catalogs
        .iter()
        .flat_map(|(_, catalog)| catalog.iter())
        .filter(|(_, message)| {
            message
                .templates()
                .iter()
                .any(|text| text.contains("{count}"))
        })
        .map(|(id, _)| id.as_str())
        .collect();
    let mut out = Vec::new();
    let mut excused = BTreeSet::new();
    for (row, catalog) in catalogs {
        if row.plural_categories().len() < 2 {
            continue;
        }
        for id in &counted {
            if !matches!(catalog.get(*id), Some(Message::Text(_))) {
                continue;
            }
            if allowed.iter().any(|(entry, _)| entry == id) {
                excused.insert(*id);
            } else {
                out.push(format!(
                    "{name} {} {id}: a {{count}} text, not a plural",
                    row.code
                ));
            }
        }
    }
    for (id, _) in allowed {
        if !excused.contains(id) {
            out.push(format!(
                "{name} allow-list: {id} is no longer a plain {{count}} text"
            ));
        }
    }
    out
}

#[test]
fn count_texts_are_plurals_in_every_layer() {
    let core = layer("locales");
    let tui = layer("../cuotai-tui/locales");
    let ui = layer("../../apps/desktop/ui/locales");
    assert_eq!(core.len(), 13, "core catalogs");
    assert_eq!(tui.len(), 13, "TUI catalogs");
    assert_eq!(ui.len(), 14, "desktop UI catalogs");
    let both = |allowed: &[(&'static str, &'static str)],
                later: &[(&'static str, &'static str)]| {
        [allowed, later].concat()
    };
    let mut out = problems("core", &core, &both(CORE_ALLOWED, CORE_FOLLOW_UP));
    out.extend(problems("tui", &tui, &both(TUI_ALLOWED, TUI_FOLLOW_UP)));
    out.extend(problems("ui", &ui, &both(UI_ALLOWED, UI_FOLLOW_UP)));
    assert!(out.is_empty(), "{out:#?}");
}

/// PRD 0037 re-validation (V-03), the "selector plural" rule: a counted
/// unit is printed beside its figure ("1 voice", "4 000 кредитов"), so
/// every `metric.unit.*` id is a plural object (with no `{count}`: the
/// figure is not part of the text) in each language with several plural
/// categories, even though it never shows `{count}`.
#[test]
fn counted_units_are_selector_plurals() {
    let core = layer("locales");
    let mut out = Vec::new();
    for (row, catalog) in &core {
        if row.plural_categories().len() < 2 {
            continue;
        }
        for (id, message) in catalog.iter() {
            if !id.starts_with("metric.unit.") {
                continue;
            }
            match message {
                Message::Plural(_) => {
                    if message
                        .templates()
                        .iter()
                        .any(|text| text.contains("{count}"))
                    {
                        out.push(format!("core {} {id}: a unit holds {{count}}", row.code));
                    }
                }
                Message::Text(_) => {
                    out.push(format!("core {} {id}: a unit, not a plural", row.code));
                }
            }
        }
    }
    assert!(
        core.iter()
            .any(|(_, catalog)| catalog.iter().any(|(id, _)| id.starts_with("metric.unit."))),
        "the core catalogs hold counted units"
    );
    assert!(out.is_empty(), "{out:#?}");
}
