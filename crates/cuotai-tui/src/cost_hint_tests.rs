//! PRD 0042 re-validation K3 (S-01b), the terminal's half: with a
//! folder-name hint on and a hidden project, no `--cost` output carries a
//! folder name, in text or JSON.

use super::*;

#[test]
fn no_cost_output_carries_a_folder_name() {
    let dir = tempfile::tempdir().unwrap();
    let env = Env::new(Some(dir.path().join("home")), []);
    let settings = SettingsStore::at(dir.path().join("cost/settings.json"));
    let index = CostIndex::new(dir.path().join("cost/cost.sqlite3"));
    let source = SourceId::ClaudeCode;
    let disclosure = settings.load().unwrap().disclosure(source, &env);
    let now = chrono::Utc::now();
    settings
        .enable(source, Vec::new(), &disclosure.digest, &env, now)
        .unwrap();
    cost::set_folder_hint(&settings, &index, source, true).unwrap();
    let logs = dir.path().join("home/.claude/projects/p");
    std::fs::create_dir_all(&logs).unwrap();
    let line = |n: u32, cwd: &str, minutes_ago: i64| {
        let at = (now - chrono::TimeDelta::minutes(minutes_ago)).to_rfc3339();
        format!(
            r#"{{"cwd":"{cwd}","sessionId":"s{n}","message":{{"id":"m{n}","model":"claude-sonnet-4-5-20250929","usage":{{"input_tokens":1000,"output_tokens":10}}}},"requestId":"r{n}","timestamp":"{at}"}}"#
        ) + "\n"
    };
    std::fs::write(
        logs.join("a.jsonl"),
        [
            line(1, "/fake/HINTMARK-folder", 9),
            line(2, "/fake/HINTMARK-folder", 6),
            line(3, "/fake/HINTMARK-hidden", 4),
            line(4, "/fake/HINTMARK-hidden", 2),
        ]
        .concat(),
    )
    .unwrap();
    let scanner =
        Scanner::new(index.clone(), settings.clone(), env.clone()).with_limits(FOREGROUND);
    scanner.scan(now).unwrap();
    let projects = index.projects().unwrap();
    assert!(
        projects.iter().all(|p| p.folder_hint.is_some()),
        "the hint is kept"
    );
    let hidden = projects
        .iter()
        .find(|p| p.folder_hint.as_deref() == Some("HINTMARK-hidden"))
        .unwrap();
    index.set_hidden(&hidden.project, true).unwrap();

    let pricing = PricingFile::at(dir.path().join("cost/pricing.json"));
    let span = Span::Days(2);
    let summary = scanner.summary(&pricing, None, Some(span), now).unwrap();
    let live = scanner.live(&pricing, None, now, &[]).unwrap();
    let mut outputs = vec![
        serde_json::to_string(&summary).unwrap(),
        usage_text(&summary, None, &[], "en-US"),
        live_text(&live, None, &[source], "en-US"),
        sources(&settings, &index, &env, true, "en-US").unwrap(),
        sources(&settings, &index, &env, false, "en-US").unwrap(),
    ];
    for by in Breakdown::ALL {
        for json in [false, true] {
            if let Ok(text) = breakdown_text(&scanner, &pricing, by, span, None, json, now, "en-US")
            {
                outputs.push(text);
            }
        }
    }
    assert!(outputs.len() > 5, "the breakdowns printed");
    for output in outputs {
        assert!(!output.contains("HINTMARK"), "{output}");
    }
}
