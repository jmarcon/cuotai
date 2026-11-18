//! PRD 0043 re-validation (V-03, V-04, R-03, R-04): the limit-change note
//! and the text Rhythm chart read naturally in every language.

use super::*;
use chrono::{NaiveDate, TimeZone};
use unicode_width::UnicodeWidthStr;

fn change(label: &str) -> LimitChange {
    LimitChange {
        label: label.into(),
        label_text: crate::metric_text::metric_text(label),
        window: None,
        from: 100.0,
        to: 150.0,
        unit: "USD".into(),
        // 23:30 UTC on Oct 25 is already Oct 26 in Tokyo.
        observed_at: Utc.with_ymd_and_hms(2026, 10, 25, 23, 30, 0).unwrap(),
    }
}

#[test]
fn a_limit_metric_says_limit_once() {
    let note = change("Limite da chave").note_local(&Utc).resolve("en");
    // The metric stands alone before the colon: never "Key limit limit".
    assert!(!note.contains("limit limit"), "{note}");
    assert!(
        note.starts_with("Key limit: limit changed 100 → 150"),
        "{note}"
    );
}

#[test]
fn the_note_writes_the_day_in_the_viewers_zone_and_language() {
    let change = change("Semanal");
    let en = change.note_local(&Utc).resolve("en");
    assert!(en.ends_with(" on 10/25"), "{en}");
    assert!(!en.contains("2026-"), "{en}");
    let tokyo = chrono::FixedOffset::east_opt(9 * 3600).unwrap();
    let de = change.note_local(&tokyo).resolve("de");
    assert!(de.contains("26.10."), "{de}");
    // Every catalog words it, with no ISO date.
    for locale in crate::text::catalog_locales() {
        let text = change.note_local(&Utc).resolve(locale);
        assert!(!text.contains("2026"), "{locale}: {text}");
        assert!(!text.contains('{'), "{locale}: {text}");
    }
}

fn bucket(day: Option<NaiveDate>, hour: Option<u8>, consumption: Option<f64>) -> RhythmBucket {
    let label = match (day, hour) {
        (Some(day), _) => crate::text::clock::day(day, day),
        (_, Some(hour)) => crate::text::clock::hour(hour.into()),
        _ => Localized::fixed("insights.rhythm.day_off", "off"),
    };
    RhythmBucket {
        start: Utc.with_ymd_and_hms(2026, 10, 20, 0, 0, 0).unwrap(),
        hour,
        day,
        consumption,
        samples: 4,
        reset: false,
        day_off: false,
        label,
    }
}

fn week() -> Rhythm {
    let mut buckets: Vec<RhythmBucket> = (14..=20)
        .map(|d| {
            let day = NaiveDate::from_ymd_opt(2026, 10, d).unwrap();
            bucket(Some(day), None, Some(f64::from(d - 13)))
        })
        .collect();
    buckets[3].day_off = true;
    buckets[4].day_off = true;
    buckets[6].consumption = None;
    Rhythm {
        range: RhythmRange::Last7d,
        buckets,
        metrics: vec!["Semanal".into()],
    }
}

/// The display column where each line's bar (or gap mark) starts.
fn bar_columns(chart: &str) -> Vec<usize> {
    chart
        .lines()
        .map(|line| {
            let at = line.find(['█', '·']).expect(line);
            line[..at].width()
        })
        .collect()
}

#[test]
fn every_bar_starts_in_the_same_display_column() {
    let typical = Rhythm {
        range: RhythmRange::Typical30d,
        buckets: (0..24u8)
            .map(|h| bucket(None, Some(h), (h % 5 != 0).then_some(f64::from(h) + 20.0)))
            .collect(),
        metrics: vec!["Semanal".into()],
    };
    for locale in ["ru", "pt-BR", "ja", "de", "en", "ko", "hi-IN"] {
        for rhythm in [week(), typical.clone()] {
            let chart = rhythm.text_chart(locale, 8);
            let columns = bar_columns(&chart);
            assert!(
                columns.windows(2).all(|w| w[0] == w[1]),
                "{locale}: {columns:?}\n{chart}"
            );
        }
    }
}

#[test]
fn the_day_off_and_sample_marks_follow_the_language() {
    let de = week().text_chart("de", 8);
    assert!(!de.contains("(off)"), "{de}");
    assert!(de.contains("(frei)"), "{de}");
    let en = week().text_chart("en", 8);
    assert!(en.contains(" (off)"), "{en}");
    let typical = Rhythm {
        range: RhythmRange::Typical30d,
        buckets: vec![bucket(None, Some(9), Some(2.0))],
        metrics: Vec::new(),
    };
    assert!(typical.text_chart("en", 8).contains("(4 days)"));
    let pt = typical.text_chart("pt-BR", 8);
    assert!(pt.contains("(4 dias)") && !pt.contains("n="), "{pt}");
    for locale in crate::text::catalog_locales() {
        let chart = typical.text_chart(locale, 8);
        assert!(
            !chart.contains("n=") && !chart.contains('{'),
            "{locale}: {chart}"
        );
    }
}
