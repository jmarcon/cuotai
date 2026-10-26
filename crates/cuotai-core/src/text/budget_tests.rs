//! PRD 0035 RF9.2, RF9.3 and RF9.6: length budgets per component class,
//! measured on the real catalogs of every language.
//!
//! A budget is a display width in terminal-style columns: a Han, kana or
//! Hangul character counts two, a combining mark (Vietnamese stacked
//! diacritics, Devanagari vowel signs) counts zero, everything else one. It
//! is the same rule the TUI lays out with and a stand-in for the rendered
//! pixel width until the screenshot pipeline (PRD 0022 RF18) measures it.
//! Placeholders are replaced by their widest sample before measuring.
//!
//! `docs/i18n-length-budgets.md` is generated from this table
//! (`CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core length_budget`); a
//! plain run fails when it drifts.

use super::*;

/// One component class: its budget, where its ids live and which ids.
struct Class {
    name: &'static str,
    budget: usize,
    /// `true`: the core catalogs (en-US is the code's English); `false`: the
    /// desktop UI catalogs.
    core: bool,
    ids: &'static [&'static str],
}

const CLASSES: &[Class] = &[
    Class {
        name: "Pace chip (points)",
        budget: 18,
        core: true,
        ids: &[
            "pace.chip.ahead",
            "pace.chip.reserve",
            "pace.chip.on_pace",
            "pace.chip.exhausted",
            "pace.chip.lasts",
            "pace.chip.runs_out",
        ],
    },
    Class {
        name: "Pace chip (percent)",
        budget: 22,
        core: true,
        ids: &["pace.chip.ahead_pct", "pace.chip.reserve_pct"],
    },
    Class {
        name: "Time chip",
        budget: 18,
        core: true,
        ids: &["resets.chip.returns", "resets.chip.return_unknown"],
    },
    Class {
        name: "Button",
        budget: 24,
        core: false,
        ids: &[
            "common.refresh",
            "common.refreshAll",
            "common.add",
            "common.edit",
            "common.remove",
            "common.cancel",
            "common.save",
            "common.open",
            "common.quit",
            "common.back",
            "common.show",
            "common.hide",
            "common.close",
            "common.dismiss",
            "common.details",
        ],
    },
    Class {
        name: "Tab",
        budget: 20,
        core: false,
        ids: &["tabs.all", "tabs.more"],
    },
    Class {
        name: "State chip",
        budget: 22,
        core: false,
        ids: &[
            "freshness.fresh",
            "freshness.aging",
            "freshness.stale",
            "status.needs_configuration",
            "status.unsupported",
            "status.authentication_error",
            "status.rate_limited",
            "status.error",
            "stage.ahead",
            "stage.on_pace",
            "stage.reserve",
            "stage.exhausted",
        ],
    },
    Class {
        name: "Sort option",
        budget: 26,
        core: false,
        ids: &["sort.custom", "sort.binding"],
    },
];

/// Catalog entries over their class budget today, waiting for the
/// translator to reword them (RF9.3). The list only shrinks: an entry that
/// fits again fails the test until it is removed.
const PENDING_REWORDING: &[(&str, &str)] = &[
    ("de", "pace.chip.lasts"),
    ("fr", "pace.chip.lasts"),
    ("ru", "pace.chip.lasts"),
    ("tr", "pace.chip.lasts"),
    ("id-ID", "pace.chip.ahead_pct"),
    ("id-ID", "pace.chip.reserve_pct"),
    ("ru", "resets.chip.return_unknown"),
    ("id-ID", "resets.chip.return_unknown"),
    ("vi", "resets.chip.returns"),
    ("vi", "resets.chip.return_unknown"),
];

/// The widest sample of each placeholder (RF9.6).
const SAMPLES: &[(&str, &str)] = &[("{d}", "99"), ("{p}", "100"), ("{at}", "23:59")];

fn is_wide(c: char) -> bool {
    matches!(u32::from(c),
        0x1100..=0x115F | 0x2E80..=0x303E | 0x3041..=0x33FF | 0x3400..=0x4DBF
        | 0x4E00..=0x9FFF | 0xA000..=0xA4CF | 0xAC00..=0xD7A3 | 0xF900..=0xFAFF
        | 0xFE30..=0xFE4F | 0xFF00..=0xFF60 | 0xFFE0..=0xFFE6)
}

fn is_combining(c: char) -> bool {
    matches!(u32::from(c),
        0x0300..=0x036F | 0x0900..=0x0902 | 0x093A | 0x093C | 0x0941..=0x0948
        | 0x094D | 0x0951..=0x0957 | 0x0962..=0x0963 | 0x200B..=0x200D)
}

/// Display width in columns (see the module docs).
pub(super) fn display_width(text: &str) -> usize {
    text.chars()
        .map(|c| {
            if is_combining(c) {
                0
            } else if is_wide(c) {
                2
            } else {
                1
            }
        })
        .sum()
}

fn substituted(template: &str) -> String {
    SAMPLES
        .iter()
        .fold(template.to_owned(), |text, (from, to)| {
            text.replace(from, to)
        })
}

fn ui_catalog(code: &str) -> Catalog {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/ui/locales")
        .join(format!("{code}.json"));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{code}: {e}"));
    Catalog::parse(&raw).unwrap()
}

/// The text of `id` in `code`, or `None` when that catalog lacks it.
fn text_of(class: &Class, code: &str, id: &str, ui: &Option<Catalog>) -> Option<String> {
    if class.core {
        let catalog = core_catalog(code).filter(|_| code != source_locale().code);
        match catalog {
            Some(catalog) => catalog.messages().get(id)?.as_text().map(str::to_owned),
            // The source language: the code's English, read from the
            // desktop's en-US catalog, which carries the core chips too.
            // The time chips are not in it: take them from the core's code.
            None => match id {
                "resets.chip.returns" => Some(
                    crate::resets::returns_chip(Some("{at}"))
                        .text
                        .text()
                        .to_owned(),
                ),
                "resets.chip.return_unknown" => {
                    Some(crate::resets::returns_chip(None).text.text().to_owned())
                }
                _ => ui_catalog(code)
                    .messages()
                    .get(id)?
                    .as_text()
                    .map(str::to_owned),
            },
        }
    } else {
        ui.as_ref()?
            .messages()
            .get(id)?
            .as_text()
            .map(str::to_owned)
    }
}

/// Every (locale, class, id, width) measured on the shipped catalogs.
fn measurements() -> Vec<(&'static str, usize, &'static str, usize)> {
    let mut out = Vec::new();
    for row in LOCALES.iter().filter(|row| row.is_translated()) {
        let ui = Some(ui_catalog(row.code));
        for (index, class) in CLASSES.iter().enumerate() {
            for id in class.ids {
                if let Some(text) = text_of(class, row.code, id, &ui) {
                    out.push((row.code, index, *id, display_width(&substituted(&text))));
                }
            }
        }
    }
    out
}

fn table() -> String {
    let measured = measurements();
    let codes: Vec<&str> = LOCALES
        .iter()
        .filter(|row| row.is_translated())
        .map(|row| row.code)
        .collect();
    let mut doc = String::from(
        "# Length budgets per component class\n\n\
         <!-- Generated by `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core length_budget`; do not edit. -->\n\n\
         PRD 0035 RF9.2: the longest text of each component class in every shipped language, measured on the real \
         catalogs, against the class budget. Widths are display columns: a Han, kana or Hangul character counts two, a \
         combining mark zero, any other character one; placeholders take their widest sample (`{d}` = 99, `{p}` = 100, \
         `{at}` = a 5-character time). Components are laid out against the budget, not the English text. A label over \
         its budget is reworded by the translator (RF9.3), never truncated by the UI; `cargo test -p cuotai-core \
         length_budget` fails on a new one. Entries marked `*` are over budget today and waiting for that rewording.\n\n",
    );
    doc.push_str("| Class | Budget |");
    for code in &codes {
        doc.push_str(&format!(" {code} |"));
    }
    doc.push_str("\n|---|---:|");
    for _ in &codes {
        doc.push_str("---:|");
    }
    doc.push('\n');
    for (index, class) in CLASSES.iter().enumerate() {
        doc.push_str(&format!("| {} | {} |", class.name, class.budget));
        for code in &codes {
            let widest = measured
                .iter()
                .filter(|(c, i, _, _)| c == code && *i == index)
                .max_by_key(|(_, _, _, width)| *width);
            match widest {
                Some((_, _, id, width)) => {
                    let mark = if PENDING_REWORDING.contains(&(code, id)) {
                        "*"
                    } else {
                        ""
                    };
                    doc.push_str(&format!(" {width}{mark} |"));
                }
                None => doc.push_str(" — |"),
            }
        }
        doc.push('\n');
    }
    doc.push_str("\nIds per class:\n\n");
    for class in CLASSES {
        let ids: Vec<String> = class.ids.iter().map(|id| format!("`{id}`")).collect();
        let catalog = if class.core { "core" } else { "desktop UI" };
        doc.push_str(&format!(
            "- **{}** ({catalog}): {}.\n",
            class.name,
            ids.join(", ")
        ));
    }
    doc
}

#[test]
fn length_budgets_hold_in_every_catalog() {
    let mut over = Vec::new();
    let mut fixed = Vec::new();
    for (code, index, id, width) in measurements() {
        let class = &CLASSES[index];
        let pending = PENDING_REWORDING.contains(&(code, id));
        if width > class.budget && !pending {
            over.push(format!(
                "{code} {id}: {width} > {} ({})",
                class.budget, class.name
            ));
        }
        if width <= class.budget && pending {
            fixed.push(format!("{code} {id}"));
        }
    }
    assert!(
        over.is_empty(),
        "over the length budget, reword (RF9.3): {over:#?}"
    );
    assert!(
        fixed.is_empty(),
        "fits again, remove from PENDING_REWORDING: {fixed:?}"
    );
    // The source language always fits: budgets are never below English.
    for (code, index, id, width) in measurements() {
        if code == source_locale().code {
            assert!(width <= CLASSES[index].budget, "{id}");
        }
    }
}

#[test]
fn length_budget_doc_matches_the_catalogs() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/i18n-length-budgets.md");
    let expected = table();
    if std::env::var_os("CUOTAI_REGENERATE_DOCS").is_some() {
        std::fs::write(&path, &expected).unwrap();
    }
    let on_disk = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        on_disk.replace("\r\n", "\n") == expected,
        "docs/i18n-length-budgets.md is stale: run \
         `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core length_budget`"
    );
}

#[test]
fn display_width_counts_wide_and_combining_characters() {
    assert_eq!(display_width("On pace"), 7);
    assert_eq!(display_width("符合节奏"), 8);
    assert_eq!(display_width("Tiê\u{0301}ng"), 5);
    assert_eq!(display_width("कि"), 2);
    assert_eq!(substituted("{p}% faster"), "100% faster");
}
