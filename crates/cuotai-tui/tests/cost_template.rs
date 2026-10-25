//! PRD 0030 RF4.2: `--cost-prices --template` prints a fill-in fragment
//! without reading logs, prices or the network.

use std::process::Command;

#[test]
fn the_template_names_the_models_with_null_rates() {
    let output = Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .args(["--cost-prices", "--template", "claude-foo-6", "gpt-x-mini"])
        .output()
        .expect("run cuotai");
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).expect("JSON output");
    assert_eq!(json["currency"], "USD");
    assert!(json["effective"].as_str().is_some_and(|d| d.len() == 10));
    for model in ["claude-foo-6", "gpt-x-mini"] {
        for field in ["input", "output", "cache_read", "cache_write"] {
            assert!(json["models"][model][field].is_null(), "{model} {field}");
        }
    }
}

#[test]
fn the_template_needs_cost_prices() {
    let output = Command::new(env!("CARGO_BIN_EXE_cuotai"))
        .args(["--template", "claude-foo-6"])
        .output()
        .expect("run cuotai");
    assert!(!output.status.success());
}
