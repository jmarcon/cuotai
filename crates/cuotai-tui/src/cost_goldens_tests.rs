//! PRD 0042 R-05 (K7): one golden file per `--cost --by` in text, en-US,
//! with a fixed clock, a fixed zone (UTC+2) and a fixed fingerprint key,
//! over synthetic Claude Code logs: a labelled session in a labelled
//! project, a session across local midnight, one without a project, and
//! the active block. `CUOTAI_REGENERATE_GOLDENS=1` rewrites the files;
//! review the diff before committing it.
//!
//! Also the `--top` rules (L-05) and the screen fit of `--cost-live`
//! (L-13).

use super::*;
use chrono::{FixedOffset, TimeZone};
use cuotai_core::cost::LabelTarget;

fn now() -> chrono::DateTime<chrono::Utc> {
    chrono::Utc
        .with_ymd_and_hms(2026, 11, 10, 12, 0, 0)
        .unwrap()
}

struct Fixture {
    _dir: tempfile::TempDir,
    scanner: Scanner,
    pricing: PricingFile,
    report: ScanReport,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let env = Env::new(Some(dir.path().join("home")), []);
    let settings = SettingsStore::at(dir.path().join("cost/settings.json"));
    let index = CostIndex::new(dir.path().join("cost/cost.sqlite3"));
    // A fixed key, so the fingerprints (and their codes) never change.
    std::fs::create_dir_all(dir.path().join("cost")).unwrap();
    std::fs::write(dir.path().join("cost/fingerprint.key"), [7u8; 32]).unwrap();
    let disclosure = settings
        .load()
        .unwrap()
        .disclosure(SourceId::ClaudeCode, &env);
    settings
        .enable(
            SourceId::ClaudeCode,
            Vec::new(),
            &disclosure.digest,
            &env,
            now() - chrono::TimeDelta::days(3),
        )
        .unwrap();
    let logs = dir.path().join("home/.claude/projects/p");
    std::fs::create_dir_all(&logs).unwrap();
    let line = |n: u32, session: &str, cwd: Option<&str>, model: &str, at: &str, input: u32| {
        let cwd = cwd
            .map(|cwd| format!(r#""cwd":"{cwd}","#))
            .unwrap_or_default();
        format!(
            r#"{{{cwd}"sessionId":"{session}","message":{{"id":"m{n}","model":"{model}","usage":{{"input_tokens":{input},"output_tokens":200}}}},"requestId":"r{n}","timestamp":"{at}"}}"#
        ) + "\n"
    };
    let sonnet = "claude-sonnet-4-5-20250929";
    let opus = "claude-opus-4-5";
    let one = Some("/srv/example/one");
    std::fs::write(
        logs.join("a.jsonl"),
        [
            line(1, "s-a", one, sonnet, "2026-11-10T10:50:00Z", 40_000),
            line(2, "s-a", one, opus, "2026-11-10T11:52:00Z", 20_000),
            line(3, "s-a", one, sonnet, "2026-11-10T11:55:00Z", 30_000),
            line(4, "s-a", one, sonnet, "2026-11-10T11:58:00Z", 10_000),
        ]
        .concat(),
    )
    .unwrap();
    // Across local midnight (23:30 to 01:15 at UTC+2).
    let two = Some("/srv/example/two");
    std::fs::write(
        logs.join("b.jsonl"),
        [
            line(5, "s-b", two, sonnet, "2026-11-09T21:30:00Z", 50_000),
            line(6, "s-b", two, opus, "2026-11-09T23:15:00Z", 5_000),
        ]
        .concat(),
    )
    .unwrap();
    std::fs::write(
        logs.join("c.jsonl"),
        line(7, "s-c", None, sonnet, "2026-11-10T06:05:00Z", 8_000),
    )
    .unwrap();
    let zone = DayZone::Fixed(FixedOffset::east_opt(2 * 3600).unwrap());
    let scanner = Scanner::new(index, settings, env)
        .with_limits(FOREGROUND)
        .with_zone(zone);
    let report = scanner.scan(now()).unwrap();
    let pricing = PricingFile::at(dir.path().join("cost/pricing.json"));
    // Label the long session and its project from the codes the lists show.
    let span = Span::Days(2);
    let page = scanner.sessions(&pricing, span, now(), None, 0).unwrap();
    let a = page
        .sessions
        .iter()
        .find(|s| s.estimate.requests == 4)
        .unwrap();
    let index = scanner.index();
    index
        .set_label(LabelTarget::Session, &a.short, Some("Parser refactor"))
        .unwrap();
    let project = a.project.clone().unwrap();
    index
        .set_label(LabelTarget::Project, &project[..8], Some("Client work"))
        .unwrap();
    Fixture {
        _dir: dir,
        scanner,
        pricing,
        report,
    }
}

fn text(f: &Fixture, by: Breakdown) -> String {
    let span = Span::Days(2);
    match by {
        Breakdown::Day | Breakdown::Model => {
            let summary = f
                .scanner
                .summary(&f.pricing, Some(&f.report), Some(span), now())
                .unwrap();
            if by == Breakdown::Day {
                usage_text(&summary, None, &[], "en-US")
            } else {
                models_text(&summary, "en-US")
            }
        }
        _ => breakdown_text(
            &f.scanner,
            &f.pricing,
            by,
            span,
            Some(DEFAULT_TOP),
            false,
            now(),
            "en-US",
        )
        .unwrap(),
    }
}

#[test]
fn every_breakdown_matches_its_golden_file() {
    let f = fixture();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cost_by");
    let regenerate = std::env::var_os("CUOTAI_REGENERATE_GOLDENS").is_some();
    let mut drift = Vec::new();
    for by in Breakdown::ALL {
        let got = text(&f, by);
        let path = dir.join(format!("{}.txt", by.slug()));
        if regenerate {
            std::fs::write(&path, &got).unwrap();
            continue;
        }
        let want = std::fs::read_to_string(&path).unwrap_or_default();
        if got != want {
            drift.push(format!("--by {}:\n{got}", by.slug()));
        }
    }
    assert!(
        drift.is_empty(),
        "outputs differ from tests/fixtures/cost_by (CUOTAI_REGENERATE_GOLDENS=1 rewrites them):\n{}",
        drift.join("\n")
    );
}

#[test]
fn top_applies_only_to_lists_and_from_one() {
    assert_eq!(
        top_rows(Breakdown::Session, None, false),
        Some(Some(DEFAULT_TOP))
    );
    assert_eq!(top_rows(Breakdown::Project, Some(3), false), Some(Some(3)));
    assert_eq!(top_rows(Breakdown::Session, None, true), Some(None));
    assert_eq!(top_rows(Breakdown::Session, Some(0), false), None);
    assert_eq!(top_rows(Breakdown::Block, Some(5), false), None);
    assert_eq!(top_rows(Breakdown::Day, None, true), None);
    // Without --top or --all, any breakdown runs.
    assert!(top_rows(Breakdown::Block, None, false).is_some());
}

#[test]
fn the_live_view_fits_the_terminal() {
    let text = "one\ntwo\nthree\nfour\n";
    assert_eq!(fit_to_screen(text, 80, 3), "one\r\ntwo");
    assert_eq!(fit_to_screen("abcdef", 4, 10), "abcd");
    // Wide characters count twice.
    assert_eq!(fit_to_screen("日本語です", 5, 10), "日本");
    assert_eq!(fit_to_screen(text, 80, 0), "one");
}
