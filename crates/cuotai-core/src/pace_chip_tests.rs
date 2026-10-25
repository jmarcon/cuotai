//! PRD 0024 RF1: chips in both units, countdown and catalog budgets.

use super::*;
use crate::text::{countdown, duration};
use chrono::TimeZone;

fn pace(used: f64, expected: f64) -> Pace {
    let delta_points = (used - expected) * 100.0;
    let stage = if used >= 1.0 {
        PaceStage::Exhausted
    } else if delta_points <= -5.0 {
        PaceStage::Reserve
    } else if delta_points >= 5.0 {
        PaceStage::Ahead
    } else {
        PaceStage::OnPace
    };
    Pace {
        elapsed_fraction: expected,
        expected_used_fraction: expected,
        used_fraction: used,
        delta_points,
        delta_ratio: delta_ratio(used, expected),
        stage,
        tone: PaceTone::Neutral,
        source: PaceSource::Average,
        resets_at: chrono::Utc::now(),
        projected_exhaustion_at: None,
        lasts_until_reset: true,
    }
}

fn chip(used: f64, expected: f64, unit: PaceUnit) -> (String, String) {
    let chip = pace(used, expected).chip(unit);
    (chip.glyph.to_owned(), chip.text.resolve("en-US"))
}

#[test]
fn stage_chips_in_both_units() {
    use PaceUnit::*;
    let cases = [
        // (used, expected, unit, glyph, text)
        (0.6, 0.5, Points, "↑", "10 pts ahead"),
        (0.6, 0.5, Percent, "↑", "20% faster than pace"),
        (0.3, 0.5, Points, "↓", "20 pts in reserve"),
        (0.3, 0.5, Percent, "↓", "40% slower than pace"),
        (0.502, 0.5, Points, "→", "On pace"),
        (0.501, 0.5, Percent, "→", "On pace"),
        (1.0, 0.5, Points, "✕", "Exhausted"),
        (1.0, 0.5, Percent, "✕", "Exhausted"),
        // Used 0 with at least 10% expected.
        (0.0, 0.1, Percent, "↓", "100% slower than pace"),
        // Capped at 999%.
        (0.99, 0.1, Percent, "↑", "890% faster than pace"),
        (0.9, 0.0, Points, "↑", "90 pts ahead"),
    ];
    for (used, expected, unit, glyph, text) in cases {
        assert_eq!(
            chip(used, expected, unit),
            (glyph.to_owned(), text.to_owned()),
            "{used} {expected} {unit:?}"
        );
    }
    let mut huge = pace(0.5, 0.1);
    huge.delta_ratio = Some(50.0);
    assert_eq!(
        huge.chip(PaceUnit::Percent).text.resolve("en-US"),
        "999% faster than pace"
    );
    assert_eq!(
        pace(0.6, 0.5).chip(PaceUnit::Percent).compact.as_deref(),
        Some("20%")
    );
}

#[test]
fn percent_falls_back_to_points_below_a_tenth_of_the_window() {
    for expected in [0.0, 0.099] {
        let chip = pace(0.5, expected).chip(PaceUnit::Percent);
        assert!(chip.text.id.starts_with("pace.chip.ahead"), "{expected}");
        assert_eq!(chip.text.id, "pace.chip.ahead");
        assert_eq!(
            chip.hint.unwrap().resolve("en-US"),
            "Too early in the window for a percentage"
        );
    }
    let at_ten = pace(0.5, 0.10).chip(PaceUnit::Percent);
    assert_eq!(at_ten.text.id, "pace.chip.ahead_pct");
    assert_eq!(delta_ratio(f64::NAN, 0.5), None);
    assert_eq!(delta_ratio(0.5, f64::INFINITY), None);
}

#[test]
fn outlook_chips() {
    let hhmm = |t: DateTime<Utc>| t.format("%H:%M").to_string();
    let mut runs_out = pace(0.8, 0.5);
    runs_out.lasts_until_reset = false;
    runs_out.projected_exhaustion_at = Some(Utc.with_ymd_and_hms(2026, 2, 1, 18, 53, 0).unwrap());
    let chip = runs_out.outlook_chip(hhmm).unwrap();
    assert_eq!(chip.glyph, "↑");
    assert_eq!(chip.text.resolve("en-US"), "Runs out ≈18:53");
    assert_eq!(chip.compact.as_deref(), Some("≈18:53"));
    let lasts = pace(0.3, 0.5).outlook_chip(hhmm).unwrap();
    assert_eq!(
        (lasts.glyph, lasts.text.resolve("en-US").as_str()),
        ("↓", "Lasts to reset")
    );
    assert_eq!(pace(1.0, 0.5).outlook_chip(hhmm), None);
    let returns = crate::resets::returns_chip(Some("19:30"));
    assert_eq!(returns.glyph, "↺");
    assert_eq!(returns.text.resolve("en-US"), "Returns 19:30");
    assert_eq!(
        crate::resets::returns_chip(None).text.resolve("en-US"),
        "Return unknown"
    );
    assert_eq!(
        crate::resets::pair_label("19:30", TimeDelta::minutes(95)).resolve("en-US"),
        "Resets 19:30 · in 1h 35m"
    );
}

#[test]
fn durations_at_the_unit_boundaries() {
    let cases = [
        (TimeDelta::seconds(0), "now", "0 min"),
        (TimeDelta::seconds(59), "in 1 min", "1 min"),
        (TimeDelta::minutes(4), "in 4 min", "4 min"),
        (TimeDelta::minutes(59), "in 59 min", "59 min"),
        (TimeDelta::seconds(3_599), "in 59 min", "59 min"),
        (TimeDelta::minutes(60), "in 1h", "1h"),
        (TimeDelta::minutes(95), "in 1h 35m", "1h 35m"),
        (TimeDelta::minutes(23 * 60 + 59), "in 23h 59m", "23h 59m"),
        (TimeDelta::hours(87), "in 3d 15h", "3d 15h"),
        (TimeDelta::days(6), "in 6d", "6d"),
    ];
    for (span, countdown_en, duration_en) in cases {
        assert_eq!(countdown(span).resolve("en-US"), countdown_en, "{span}");
        assert_eq!(duration(span).resolve("en-US"), duration_en, "{span}");
    }
    assert_eq!(
        countdown(TimeDelta::minutes(95)).resolve("pt-BR"),
        "em 1h 35m"
    );
    assert_eq!(countdown(TimeDelta::hours(87)).resolve("it"), "tra 3g 15h");
    assert_eq!(countdown(TimeDelta::zero()).resolve("es"), "ahora");
}

/// DS-CMP-TIME-1: 18 characters without the glyph (22 for the percent
/// forms) in every catalog, with the widest values.
#[test]
fn chip_texts_fit_the_budget_in_every_language() {
    let wide_at = "12:30 PM";
    let mut chips = vec![
        pace(0.99, 0.0).chip(PaceUnit::Points),
        pace(0.0, 0.99).chip(PaceUnit::Points),
        pace(0.5, 0.5).chip(PaceUnit::Points),
        pace(1.0, 0.5).chip(PaceUnit::Points),
        crate::resets::returns_chip(Some(wide_at)),
        crate::resets::returns_chip(None),
    ];
    let mut wide_points = pace(0.5, 0.5);
    wide_points.delta_points = 100.0;
    wide_points.stage = PaceStage::Ahead;
    chips.push(wide_points.chip(PaceUnit::Points));
    wide_points.delta_points = -100.0;
    chips.push(wide_points.chip(PaceUnit::Points));
    let mut runs_out = pace(0.8, 0.5);
    runs_out.projected_exhaustion_at = Some(runs_out.resets_at);
    chips.push(runs_out.outlook_chip(|_| wide_at.to_owned()).unwrap());
    chips.push(pace(0.3, 0.5).outlook_chip(|_| wide_at.to_owned()).unwrap());
    let mut percent = pace(0.5, 0.1);
    percent.delta_ratio = Some(50.0);
    let ahead_pct = percent.chip(PaceUnit::Percent);
    percent.delta_ratio = Some(-9.99);
    let reserve_pct = percent.chip(PaceUnit::Percent);
    for locale in ["pt-BR", "en-US", "es", "it"] {
        for chip in &chips {
            let text = chip.text.resolve(locale);
            assert!(text.chars().count() <= 18, "{locale}: {text}");
        }
        for chip in [&ahead_pct, &reserve_pct] {
            let text = chip.text.resolve(locale);
            assert!(text.chars().count() <= 22, "{locale}: {text}");
            assert!(text.contains("999"), "{locale}: {text}");
        }
    }
}
