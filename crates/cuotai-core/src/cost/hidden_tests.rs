//! PRD 0042 M-05 (re-validation): a hidden project is left out of every
//! total and figure (days, models, sessions, blocks, the live block, the
//! summary and `export::v1`), stays listed only in the Projects view, and
//! every view says how many hidden projects it leaves out.

use super::*;

/// The shown project: three requests in the last ten minutes before noon.
const SHOWN: &str = "/srv/example/shown";
/// The project the tests hide.
const HIDDEN: &str = "/srv/example/hidden";

fn two_projects(f: &Fixture) {
    write(
        &f.claude_log("shown"),
        &[
            claude_at(1, "s-shown", SHOWN, "2025-10-03T10:05:00Z", 1000),
            claude_at(2, "s-shown", SHOWN, "2025-10-03T11:51:00Z", 1000),
            claude_at(3, "s-shown", SHOWN, "2025-10-03T11:55:00Z", 1000),
            claude_at(4, "s-shown", SHOWN, "2025-10-03T11:57:00Z", 1000),
        ]
        .concat(),
    );
    write(
        &f.claude_log("hidden"),
        &[
            // Yesterday evening: a block of its own, only hidden.
            claude_at(5, "s-hidden", HIDDEN, "2025-10-02T20:00:00Z", 7000),
            // Opens today's block at 09:00, before the shown project.
            claude_at(6, "s-hidden", HIDDEN, "2025-10-03T09:30:00Z", 800_000),
            claude_at(7, "s-hidden", HIDDEN, "2025-10-03T11:58:00Z", 2000),
        ]
        .concat(),
    );
}

/// The shown project's tokens: four requests of 1000 input and 10 output.
const SHOWN_TOKENS: u64 = 4 * 1010;

fn hide(f: &Fixture, pricing: &PricingFile) -> String {
    let page = f.scanner.projects(pricing, Span::Days(7), now()).unwrap();
    let hidden = page
        .projects
        .iter()
        .find(|p| p.estimate.requests == 3)
        .and_then(|p| p.short.clone())
        .unwrap();
    f.scanner.index().set_hidden(&hidden, true).unwrap();
    hidden
}

#[test]
fn a_hidden_project_leaves_every_total() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    two_projects(&f);
    f.scan();
    let pricing = pricing(&f);
    let span = Span::Days(7);
    let before = f
        .scanner
        .summary(&pricing, None, Some(span), now())
        .unwrap();
    assert_eq!(before.hidden_projects, 0);
    assert!(before.hidden_note.is_none());
    let aligned = f
        .scanner
        .blocks(&pricing, None, span, now(), &[])
        .unwrap()
        .blocks;
    let prefix = hide(&f, &pricing);

    // Days, models, today, yesterday and the span.
    let summary = f
        .scanner
        .summary(&pricing, None, Some(span), now())
        .unwrap();
    assert_eq!(summary.total.tokens.total(), SHOWN_TOKENS);
    assert_eq!(summary.total.requests, 4);
    assert_eq!(summary.hidden_projects, 1);
    assert_eq!(
        summary.hidden_note.as_ref().unwrap().resolve("en-US"),
        "1 hidden project is not counted in these figures."
    );
    let usage = summary.sources[0].usage.as_ref().unwrap();
    assert_eq!(usage.today.tokens.total(), SHOWN_TOKENS);
    assert_eq!(usage.yesterday.tokens.total(), 0, "only hidden yesterday");
    assert!(usage.yesterday.models.is_empty());
    assert_eq!(
        usage.days.iter().map(|d| d.tokens.total()).sum::<u64>(),
        SHOWN_TOKENS
    );
    let models: u64 = usage.period.models.iter().map(|m| m.tokens.total()).sum();
    assert_eq!(models, SHOWN_TOKENS);

    // The usage rows lose the hidden-only session of each day.
    let hidden = f
        .scanner
        .index()
        .hidden_usage(None, now().date_naive())
        .unwrap();
    assert_eq!(hidden.rows.len(), 2, "{hidden:?}");
    assert!(hidden.rows.iter().all(|r| r.sessions == 1));
    assert_eq!(hidden.projects_between(None, now().date_naive()), 1);
    assert_eq!(
        hidden.projects_between(
            Some(now().date_naive() + chrono::Days::new(1)),
            now().date_naive()
        ),
        0
    );

    // Sessions.
    let sessions = f.scanner.sessions(&pricing, span, now(), None, 0).unwrap();
    assert_eq!(sessions.count, 1);
    assert_eq!(sessions.total.tokens.total(), SHOWN_TOKENS);
    assert!(sessions.hidden_note.is_some());

    // Blocks: aligned on every request, figures of the shown ones only.
    let page = f.scanner.blocks(&pricing, None, span, now(), &[]).unwrap();
    assert_eq!(page.blocks.len(), 1, "a block of only hidden requests goes");
    let active = &page.blocks[0];
    assert!(active.active);
    assert_eq!(
        active.start, aligned[0].start,
        "hiding does not move the window"
    );
    assert_eq!(
        active.start,
        Utc.with_ymd_and_hms(2025, 10, 3, 9, 0, 0).unwrap()
    );
    assert_eq!(active.estimate.tokens.total(), SHOWN_TOKENS);
    assert_eq!((active.sessions, active.projects), (1, 1));
    assert_eq!(
        active.last_request,
        Utc.with_ymd_and_hms(2025, 10, 3, 11, 57, 0).unwrap()
    );
    assert_eq!(page.total, active.estimate, "R-02: the total is the rows'");
    assert!(page.hidden_note.is_some());

    // The live block: its figures and burn rate leave the hidden out.
    let (live, note) = f.scanner.live_view(&pricing, None, now(), &[]).unwrap();
    assert_eq!(live.len(), 1);
    assert_eq!(live[0].block.estimate.tokens.total(), SHOWN_TOKENS);
    let rate = live[0].burn_rate_tokens_per_min.unwrap();
    assert!((rate - 303.0).abs() < 0.5, "{rate}");
    assert!(note.is_some());

    // Projects: listed and flagged, out of the total.
    let projects = f.scanner.projects(&pricing, span, now()).unwrap();
    assert_eq!(projects.projects.len(), 2);
    assert_eq!(projects.hidden, 1);
    assert_eq!(projects.total.tokens.total(), SHOWN_TOKENS);
    assert!(projects.hidden_note.is_some());

    // The summary's blocks, live block and projects.
    assert_eq!(summary.blocks.len(), 1);
    assert_eq!(summary.blocks[0].estimate.tokens.total(), SHOWN_TOKENS);
    assert_eq!(summary.live[0].block.estimate.tokens.total(), SHOWN_TOKENS);
    assert_eq!(summary.projects.len(), 1);

    // The plan value reads the shown total too.
    let settings = f.scanner.settings();
    let account = crate::Account {
        name: "Work".into(),
        provider: crate::Provider::Claude,
        ..crate::Account::default()
    };
    settings
        .set_plan(
            &account.state_key(),
            Some(PlanSetting::new(100.0, "USD", Some(1)).unwrap()),
        )
        .unwrap();
    let plans = f
        .scanner
        .plan_values(&pricing, std::slice::from_ref(&account), now())
        .unwrap();
    assert_eq!(plans[0].estimate, summary.total.total);

    // Counted again.
    f.scanner.index().set_hidden(&prefix, false).unwrap();
    let again = f
        .scanner
        .summary(&pricing, None, Some(span), now())
        .unwrap();
    assert_eq!(again.total, before.total);
    assert_eq!(again.hidden_projects, 0);
}

#[test]
fn only_hidden_recent_usage_gives_no_live_line() {
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    write(
        &f.claude_log("hidden"),
        &[
            claude_at(1, "s-hidden", HIDDEN, "2025-10-03T11:51:00Z", 10),
            claude_at(2, "s-hidden", HIDDEN, "2025-10-03T11:55:00Z", 10),
            claude_at(3, "s-hidden", HIDDEN, "2025-10-03T11:58:00Z", 10),
        ]
        .concat(),
    );
    write(
        &f.claude_log("shown"),
        &claude_at(4, "s-shown", SHOWN, "2025-10-03T11:59:00Z", 10),
    );
    f.scan();
    let pricing = pricing(&f);
    let projects = f.scanner.projects(&pricing, Span::Days(7), now()).unwrap();
    let hidden = projects
        .projects
        .iter()
        .find(|p| p.estimate.requests == 3)
        .and_then(|p| p.short.clone())
        .unwrap();
    f.scanner.index().set_hidden(&hidden, true).unwrap();
    let (live, note) = f.scanner.live_view(&pricing, None, now(), &[]).unwrap();
    assert_eq!(live.len(), 1, "the shown request keeps the block");
    assert!(
        live[0].burn_rate_tokens_per_min.is_none(),
        "one shown request gives no rate"
    );
    assert!(note.is_some());
    // Only hidden requests: no block at all.
    let shown = projects
        .projects
        .iter()
        .find(|p| p.estimate.requests == 1)
        .and_then(|p| p.short.clone())
        .unwrap();
    f.scanner.index().set_hidden(&shown, true).unwrap();
    let (live, note) = f.scanner.live_view(&pricing, None, now(), &[]).unwrap();
    assert!(live.is_empty());
    assert_eq!(
        note.unwrap().resolve("en-US"),
        "2 hidden projects are not counted in these figures."
    );
}

/// The note's plural in English, Russian (one/few/many) and Japanese.
#[test]
fn the_hidden_note_is_a_plural() {
    assert!(hidden_note(0).is_none());
    let text = |count, locale| hidden_note(count).unwrap().resolve(locale);
    assert_eq!(
        text(2, "en-US"),
        "2 hidden projects are not counted in these figures."
    );
    for (count, word) in [
        (1, "скрытый проект"),
        (3, "скрытых проекта"),
        (5, "скрытых проектов"),
    ] {
        let ru = text(count, "ru");
        assert!(ru.starts_with(&format!("{count} {word}")), "{ru}");
    }
    let ja = text(4, "ja");
    assert!(ja.contains('4') && !ja.contains("{count}"), "{ja}");
}

/// The export scenario of the review: the same fixture through
/// `Scanner::summary` and `Export::with_local_cost`, serialized.
#[test]
fn the_export_leaves_a_hidden_project_out() {
    use crate::config_file::ConfigIdentity;
    use crate::export::v1::{Export, Options};
    let f = fixture();
    f.enable(SourceId::ClaudeCode);
    two_projects(&f);
    f.scan();
    let pricing = pricing(&f);
    hide(&f, &pricing);
    let summary = f.scanner.summary(&pricing, None, None, now()).unwrap();
    let config = crate::Config {
        accounts: vec![crate::Account {
            name: "Api".into(),
            provider: crate::Provider::OpenAiApi,
            credential_env: Some("CUOTAI_TEST_UNSET_KEY".into()),
            ..crate::Account::default()
        }],
        ..crate::Config::default()
    };
    let cycle = crate::refresh::RefreshCycle::new(&config, Instant::now());
    let export = Export::from_cycle(
        &config,
        &ConfigIdentity::Explicit("test".into()),
        &cycle,
        &[],
        now(),
        Options::default(),
    )
    .with_local_cost(Some(summary));
    let json = serde_json::to_value(&export).unwrap();
    let local = &json["local_cost"];
    let tokens = |estimate: &serde_json::Value| -> u64 {
        [
            "input",
            "output",
            "cache_read",
            "cache_write",
            "cache_write_1h",
        ]
        .iter()
        .map(|kind| estimate["tokens"][kind].as_u64().unwrap_or(0))
        .sum()
    };
    assert_eq!(tokens(&local["total"]), SHOWN_TOKENS);
    assert_eq!(tokens(&local["blocks"][0]["estimate"]), SHOWN_TOKENS);
    assert_eq!(tokens(&local["live"][0]["block"]["estimate"]), SHOWN_TOKENS);
    assert_eq!(local["projects"].as_array().unwrap().len(), 1);
    assert_eq!(local["hidden_projects"], 1);
    assert_eq!(local["hidden_note"]["id"], "cost.hidden.note");
    let text = json.to_string();
    for leak in ["/srv/example", SECRET] {
        assert!(!text.contains(leak), "{leak}");
    }
}
