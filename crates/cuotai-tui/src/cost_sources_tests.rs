//! PRD 0042 re-validation S-07: `--cost-sources` shows the index size.

use super::*;

#[test]
fn cost_sources_shows_the_index_size_in_text_and_json() {
    let dir = tempfile::tempdir().unwrap();
    let env = Env::new(Some(dir.path().join("home")), []);
    let settings = SettingsStore::at(dir.path().join("cost/settings.json"));
    let index = CostIndex::new(dir.path().join("cost/cost.sqlite3"));

    // No index yet: zero bytes, and the file is not created.
    let json = sources(&settings, &index, &env, true, "en-US").unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["size"], 0, "{json}");
    assert!(!index.path().exists());

    // A scan with one source on and no logs creates the index.
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
            chrono::Utc::now(),
        )
        .unwrap();
    let scanner = Scanner::new(index, settings, env.clone()).with_limits(FOREGROUND);
    scanner.scan(chrono::Utc::now()).unwrap();
    let (settings, index) = (scanner.settings(), scanner.index());
    let bytes = index.size_on_disk().unwrap();
    assert!(bytes > 0);
    let json = sources(settings, index, &env, true, "en-US").unwrap();
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value["size"], bytes, "{json}");
    let text = sources(settings, index, &env, false, "en-US").unwrap();
    let line = cost::index_size_text(bytes).resolve("en-US");
    assert!(
        line.starts_with("Index: ") && line.ends_with(" on disk"),
        "{line}"
    );
    assert!(text.contains(&line), "{text}");
}
