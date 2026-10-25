//! PRD 0028 RF3: the provider-problem issue form is a valid GitHub issue
//! form (the generated dropdown included).

use std::collections::BTreeSet;

#[test]
fn provider_problem_form_is_a_valid_issue_form() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../.github/ISSUE_TEMPLATE/provider-problem.yml");
    let raw = std::fs::read_to_string(path).unwrap();
    let form: serde_yaml::Value = serde_yaml::from_str(&raw).expect("valid YAML");
    for key in ["name", "description", "body"] {
        assert!(form.get(key).is_some(), "missing {key}");
    }
    let mut ids = BTreeSet::new();
    for item in form["body"].as_sequence().expect("body is a list") {
        let kind = item["type"].as_str().expect("type");
        assert!(
            ["markdown", "input", "textarea", "dropdown", "checkboxes"].contains(&kind),
            "{kind}"
        );
        if kind != "markdown" {
            let id = item["id"].as_str().expect("id");
            assert!(ids.insert(id.to_string()), "duplicate id {id}");
            assert!(item["attributes"]["label"].as_str().is_some(), "{id} label");
        }
        if kind == "dropdown" {
            let options = item["attributes"]["options"].as_sequence().unwrap();
            assert!(!options.is_empty(), "{kind} options");
        }
    }
    for id in [
        "provider", "version", "mode", "category", "paths", "context",
    ] {
        assert!(ids.contains(id), "{id}");
    }
}
