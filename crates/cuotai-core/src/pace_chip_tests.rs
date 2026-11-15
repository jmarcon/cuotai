//! PRD 0024 RF1: chips in both units, countdown and catalog budgets.

use super::*;
use crate::text::{countdown, duration, ParamValue};
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
        // PRD 0007 re-validation (V-03): inside the ±5 band the chip
        // agrees with the stage, never "2 pts ahead" beside →.
        (0.52, 0.5, Points, "→", "On pace"),
        (0.54, 0.5, Percent, "→", "On pace"),
        (0.46, 0.5, Points, "→", "On pace"),
        (0.47, 0.5, Percent, "→", "On pace"),
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
        pace(0.6, 0.5)
            .chip(PaceUnit::Percent)
            .compact
            .map(|c| c.resolve("fr")),
        Some("20 %".to_owned())
    );
}

/// Outside the band a shown point count is always at least 5, so "1 pts"
/// is unreachable and the chip catalogs need no plural forms.
#[test]
fn a_shown_point_count_is_never_below_five() {
    for used in 0..1000 {
        for expected in [0.0, 0.05, 0.2, 0.5, 0.8, 0.999] {
            let pace = pace(f64::from(used) / 1000.0, expected);
            for unit in [PaceUnit::Points, PaceUnit::Percent] {
                let chip = pace.chip(unit);
                if let Some(ParamValue::Text(d)) = chip.text.params.get("d") {
                    let d: f64 = d.parse().unwrap();
                    assert!(d >= 5.0, "{used} {expected} {unit:?}: {d}");
                }
                if let Some(ParamValue::Text(p)) = chip.text.params.get("p") {
                    let p: f64 = p.parse().unwrap();
                    assert!(p >= 1.0, "{used} {expected} {unit:?}: {p}");
                }
                if pace.stage == PaceStage::OnPace {
                    assert_eq!(chip.text.id, "pace.chip.on_pace");
                }
            }
        }
    }
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
    assert_eq!(
        chip.compact.map(|c| c.resolve("en-US")),
        Some("≈18:53".to_owned())
    );
    let lasts = pace(0.3, 0.5).outlook_chip(hhmm).unwrap();
    assert_eq!(
        (lasts.glyph, lasts.text.resolve("en-US").as_str()),
        ("↓", "Lasts to reset")
    );
    assert_eq!(pace(1.0, 0.5).outlook_chip(hhmm), None);
    let returns = crate::resets::returns_chip(Some("19:30".into()));
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
        crate::resets::returns_chip(Some(wide_at.into())),
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

/// PRD 0024 re-validation (V-01): with the core clock in every language, a
/// chip shows the time today and only the short weekday or the short date
/// beyond, within the DS-CMP-TIME-1 budget of 18 columns.
#[test]
fn clock_chips_fit_the_budget_in_every_language() {
    use crate::text::{catalog_locales, clock_short};
    let now = Utc.with_ymd_and_hms(2026, 11, 16, 0, 30, 0).unwrap(); // Monday
    let mut forms = vec![Utc.with_ymd_and_hms(2026, 11, 16, 12, 59, 0).unwrap()];
    // Every weekday ahead (the widest short name), then a two-digit date.
    forms.extend((1..=6).map(|d| now + TimeDelta::days(d)));
    forms.push(Utc.with_ymd_and_hms(2026, 12, 28, 23, 59, 0).unwrap());
    let mut runs_out = pace(0.8, 0.5);
    runs_out.lasts_until_reset = false;
    for locale in std::iter::once("en-US").chain(catalog_locales()) {
        for at in &forms {
            let short = clock_short(*at, now, &Utc);
            runs_out.projected_exhaustion_at = Some(*at);
            let chips = [
                runs_out.outlook_chip(|_| short.clone()).unwrap(),
                crate::resets::returns_chip(Some(short.clone().into())),
            ];
            for chip in chips {
                let text = chip.text.resolve(locale);
                let width = crate::text::budget_width(&text);
                assert!(width <= 18, "{locale}: {text} ({width})");
                assert!(!text.contains("2026"), "{locale}: {text}");
            }
        }
    }
}

/// PRD 0024 re-validation (D-01, V-03, V-09, R-01): the pace line hides "On
/// pace" beside a projection, keeps the stage sentences in percent, and
/// says why the percent form fell back.
#[test]
fn the_pace_line_follows_the_stage_and_the_unit() {
    let at = |t: DateTime<Utc>| t.format("%H:%M").to_string();
    let margin = |_| "2h".to_owned();
    let mut on_pace = pace(0.52, 0.5);
    on_pace.lasts_until_reset = false;
    on_pace.projected_exhaustion_at = Some(Utc.with_ymd_and_hms(2026, 2, 1, 18, 53, 0).unwrap());
    on_pace.resets_at = Utc.with_ymd_and_hms(2026, 2, 1, 20, 53, 0).unwrap();
    assert!(!on_pace.shows_stage());
    assert_eq!(on_pace.glyph(), GLYPH_AHEAD);
    for unit in [PaceUnit::Points, PaceUnit::Percent] {
        assert_eq!(
            on_pace.summary(unit, at, margin).resolve("en-US"),
            "Runs out at 18:53, 2h before the reset"
        );
    }
    assert_eq!(
        on_pace
            .summary(PaceUnit::Points, at, margin)
            .resolve("pt-BR"),
        "Acaba às 18:53, 2h antes da renovação"
    );
    let lasting = pace(0.52, 0.5);
    assert!(lasting.shows_stage());
    assert_eq!(
        lasting
            .summary(PaceUnit::Points, at, margin)
            .resolve("en-US"),
        "On pace · lasts until the reset"
    );
    // Exhausted keeps its sentence in both units.
    let mut exhausted = pace(1.0, 0.5);
    exhausted.lasts_until_reset = false;
    for unit in [PaceUnit::Points, PaceUnit::Percent] {
        assert_eq!(
            exhausted.summary(unit, at, margin).resolve("en-US"),
            "Limit reached until the reset"
        );
    }
    // Percent with a ratio names the comparison; without one the points
    // sentence carries the hint.
    assert_eq!(
        pace(0.6, 0.5)
            .summary(PaceUnit::Percent, at, margin)
            .resolve("en-US"),
        "20% faster than pace · lasts until the reset"
    );
    assert_eq!(
        pace(0.18, 0.05)
            .stage_text(PaceUnit::Percent)
            .resolve("en-US"),
        "13 points above the pace (too early for a percentage)"
    );
    assert_eq!(
        pace(0.18, 0.05)
            .stage_text(PaceUnit::Points)
            .resolve("en-US"),
        "13 points above the pace"
    );
    // The muted marker only for a recent-slope projection.
    assert_eq!(pace(0.6, 0.5).source_marker(), None);
    let mut recent = pace(0.6, 0.5);
    recent.source = PaceSource::Recent;
    assert_eq!(recent.source_marker().unwrap().resolve("en-US"), "recent");
}
