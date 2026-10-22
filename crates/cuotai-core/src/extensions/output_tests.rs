//! PRD 0017 RF2: the output contract, as table tests over printed outputs
//! (valid, partial, oversized, `NaN`/infinity, unknown `kind`, malformed
//! JSON and every other violation) and the reading each one becomes.

use super::*;
use crate::events::FailureCategory;
use serde_json::json;
use std::collections::BTreeSet;

fn valid() -> Value {
    json!({
        "schema": 1,
        "status": "ready",
        "metrics": [
            {
                "label": "Balance", "used": null, "limit": null, "remaining": 12.5,
                "unit": "USD", "kind": "balance", "window": null, "period": null,
                "resets_at": null
            },
            {
                "label": "Requests", "used": 40, "limit": 100, "remaining": 60,
                "unit": "req", "kind": "quota", "window": "daily",
                "period": "2026-10-21", "resets_at": "2026-10-22T00:00:00-03:00"
            }
        ],
        "notes": ["Proxy online."]
    })
}

/// `valid()` with `change` applied.
fn edited(change: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut value = valid();
    change(&mut value);
    value.to_string().into_bytes()
}

fn report(checked: Checked) -> Report {
    checked.into_report("Proxy", Provider::Claude, Utc::now())
}

fn violation_ids(checked: &Checked) -> Vec<String> {
    checked
        .violations
        .iter()
        .map(|violation| violation.id.to_string())
        .collect()
}

fn param(note: &Localized, key: &str) -> String {
    match note.params.get(key) {
        Some(crate::text::ParamValue::Text(text)) => text.clone(),
        other => panic!("{key}: {other:?}"),
    }
}

#[test]
fn a_valid_output_becomes_a_ready_report_of_the_extension() {
    let report = report(check("proxy", valid().to_string().as_bytes()));
    assert_eq!(report.status, Status::Ready);
    assert_eq!(report.source, "extension:proxy");
    assert_eq!(report.account, "Proxy");
    assert_eq!(FailureCategory::of(&report), None);
    let [balance, requests] = &report.metrics[..] else {
        panic!("{:?}", report.metrics);
    };
    assert_eq!(balance.label, "Balance");
    assert_eq!(
        (balance.used, balance.limit, balance.remaining),
        (None, None, Some(12.5))
    );
    assert_eq!(balance.kind, MetricKind::Balance);
    assert_eq!((balance.window, &balance.period), (None, &None));
    assert_eq!(balance.resets_at, None);
    assert_eq!(
        (requests.used, requests.limit, requests.remaining),
        (Some(40.0), Some(100.0), Some(60.0))
    );
    assert_eq!(requests.unit, "req");
    assert_eq!(requests.window, Some(MetricWindow::Daily));
    assert_eq!(requests.period.as_deref(), Some("2026-10-21"));
    assert_eq!(
        requests.resets_at,
        Some("2026-10-22T03:00:00Z".parse().unwrap())
    );
    assert_eq!(report.notes, ["Proxy online."]);
    assert_eq!(report.notes_l10n.len(), 1);
    assert_eq!(report.notes_l10n[0].id, "extensions.output.note");
    assert_eq!(report.notes_l10n[0].resolve("en-US"), "Proxy online.");
    assert_eq!(report.notes_l10n[0].resolve("es"), "Proxy online.");
}

#[test]
fn absent_values_stay_unknown_and_unknown_fields_are_ignored() {
    let partial = json!({
        "schema": 1,
        "status": "ready",
        "metrics": [{ "label": "Tokens", "unit": "", "kind": "usage", "colour": "red" }],
        "generated_by": "a newer contract"
    });
    let checked = check("p", partial.to_string().as_bytes());
    assert!(checked.violations.is_empty(), "{:?}", checked.violations);
    assert_eq!(checked.status, Status::Ready);
    let metric = &checked.metrics[0];
    assert_eq!(
        (metric.used, metric.limit, metric.remaining),
        (None, None, None)
    );
    assert_eq!((metric.window, metric.resets_at), (None, None));
    assert_eq!(metric.period, None);
    assert_eq!(metric.unit, "");
    assert!(checked.notes.is_empty());
}

/// A named output and either the number of metrics it reads or the ids of
/// its violations.
type Case = (&'static str, Vec<u8>, Result<usize, Vec<&'static str>>);

#[test]
fn outputs_are_checked_against_the_contract() {
    let at_limit = {
        let mut bytes = valid().to_string().into_bytes();
        bytes.resize(MAX_BYTES, b' ');
        bytes
    };
    let mut oversized = at_limit.clone();
    oversized.push(b'\n');
    let metric = json!({ "label": "M", "unit": "u", "kind": "usage" });
    let note = json!("n");
    let raw = |used: &str| {
        format!(
            r#"{{"schema":1,"status":"ready","metrics":[{{"label":"M","unit":"u","kind":"usage","used":{used}}}]}}"#
        )
        .into_bytes()
    };
    let cases: Vec<Case> = vec![
        ("valid", valid().to_string().into_bytes(), Ok(2)),
        (
            "no metrics, no notes",
            br#"{"schema":1,"status":"ready","metrics":[]}"#.to_vec(),
            Ok(0),
        ),
        ("exactly the size limit", at_limit, Ok(2)),
        (
            "the most metrics",
            edited(|v| v["metrics"] = json!(vec![metric.clone(); MAX_METRICS])),
            Ok(MAX_METRICS),
        ),
        (
            "the most notes",
            edited(|v| v["notes"] = json!(vec![note.clone(); MAX_NOTES])),
            Ok(2),
        ),
        (
            "oversized",
            oversized,
            Err(vec!["extensions.output.too_large"]),
        ),
        ("NaN", raw("NaN"), Err(vec!["extensions.output.json"])),
        (
            "Infinity",
            raw("Infinity"),
            Err(vec!["extensions.output.json"]),
        ),
        (
            "-Infinity",
            raw("-Infinity"),
            Err(vec!["extensions.output.json"]),
        ),
        ("1e999", raw("1e999"), Err(vec!["extensions.output.json"])),
        (
            "number as text",
            raw("\"12\""),
            Err(vec!["extensions.output.number"]),
        ),
        (
            "unknown kind",
            edited(|v| v["metrics"][1]["kind"] = json!("tokens")),
            Err(vec!["extensions.output.choice"]),
        ),
        (
            "unknown window",
            edited(|v| v["metrics"][0]["window"] = json!("hourly")),
            Err(vec!["extensions.output.choice"]),
        ),
        (
            "malformed JSON",
            br#"{"schema": 1, "status": "ready","#.to_vec(),
            Err(vec!["extensions.output.json"]),
        ),
        (
            "two objects",
            br#"{"schema":1,"status":"ready","metrics":[]} {}"#.to_vec(),
            Err(vec!["extensions.output.json"]),
        ),
        (
            "plain text",
            b"OK\n".to_vec(),
            Err(vec!["extensions.output.json"]),
        ),
        (
            "not an object",
            b"[1, 2]".to_vec(),
            Err(vec!["extensions.output.not_object"]),
        ),
        (
            "not UTF-8",
            b"{\"schema\":1,\"x\":\"\xff\"}".to_vec(),
            Err(vec!["extensions.output.not_utf8"]),
        ),
        (
            "empty",
            b" \n".to_vec(),
            Err(vec!["extensions.output.empty"]),
        ),
        (
            "schema missing",
            edited(|v| drop(v.as_object_mut().unwrap().remove("schema"))),
            Err(vec!["extensions.output.schema"]),
        ),
        (
            "a newer schema, whatever else it carries",
            edited(|v| {
                v["schema"] = json!(2);
                v["metrics"] = json!("anything");
            }),
            Err(vec!["extensions.output.schema"]),
        ),
        (
            "schema as text",
            edited(|v| v["schema"] = json!("1")),
            Err(vec!["extensions.output.schema"]),
        ),
        (
            "status missing",
            edited(|v| drop(v.as_object_mut().unwrap().remove("status"))),
            Err(vec!["extensions.output.missing"]),
        ),
        (
            "unknown status",
            edited(|v| v["status"] = json!("great")),
            Err(vec!["extensions.output.choice"]),
        ),
        (
            "metrics missing",
            edited(|v| drop(v.as_object_mut().unwrap().remove("metrics"))),
            Err(vec!["extensions.output.missing"]),
        ),
        (
            "metrics not a list",
            edited(|v| v["metrics"] = json!({})),
            Err(vec!["extensions.output.list"]),
        ),
        (
            "a metric not an object",
            edited(|v| v["metrics"][0] = json!(3)),
            Err(vec!["extensions.output.object"]),
        ),
        (
            "too many metrics",
            edited(|v| v["metrics"] = json!(vec![metric.clone(); MAX_METRICS + 1])),
            Err(vec!["extensions.output.too_many"]),
        ),
        (
            "too many notes",
            edited(|v| v["notes"] = json!(vec![note.clone(); MAX_NOTES + 1])),
            Err(vec!["extensions.output.too_many"]),
        ),
        (
            "notes not a list",
            edited(|v| v["notes"] = json!("n")),
            Err(vec!["extensions.output.list"]),
        ),
        (
            "a note not a string",
            edited(|v| v["notes"] = json!(["ok", 1])),
            Err(vec!["extensions.output.text"]),
        ),
        (
            "invalid date",
            edited(|v| v["metrics"][0]["resets_at"] = json!("tomorrow")),
            Err(vec!["extensions.output.date"]),
        ),
        (
            "date as number",
            edited(|v| v["metrics"][0]["resets_at"] = json!(1_761_000_000)),
            Err(vec!["extensions.output.date"]),
        ),
        (
            "long unit",
            edited(|v| v["metrics"][0]["unit"] = json!("u".repeat(UNIT_MAX_CHARS + 1))),
            Err(vec!["extensions.output.unit"]),
        ),
        (
            "unit with a control character",
            edited(|v| v["metrics"][0]["unit"] = json!("US\u{7}D")),
            Err(vec!["extensions.output.unit"]),
        ),
        (
            "label missing",
            edited(|v| drop(v["metrics"][0].as_object_mut().unwrap().remove("label"))),
            Err(vec!["extensions.output.missing"]),
        ),
        (
            "label only markup",
            edited(|v| v["metrics"][0]["label"] = json!("<b></b> **")),
            Err(vec!["extensions.output.empty_text"]),
        ),
        (
            "label not a string",
            edited(|v| v["metrics"][0]["label"] = json!(5)),
            Err(vec!["extensions.output.text"]),
        ),
        (
            "period not a string",
            edited(|v| v["metrics"][1]["period"] = json!(7)),
            Err(vec!["extensions.output.text"]),
        ),
        (
            "every violation is listed",
            edited(|v| {
                v["status"] = json!(null);
                v["metrics"][0]["kind"] = json!(null);
                v["metrics"][1]["limit"] = json!(true);
                v["notes"] = json!([null]);
            }),
            Err(vec![
                "extensions.output.missing",
                "extensions.output.missing",
                "extensions.output.number",
                "extensions.output.text",
            ]),
        ),
    ];
    for (name, output, expected) in cases {
        let checked = check("proxy", &output);
        assert_eq!(checked.source, "extension:proxy", "{name}");
        match expected {
            Ok(metrics) => {
                assert!(
                    checked.violations.is_empty(),
                    "{name}: {:?}",
                    checked.violations
                );
                assert_eq!(checked.status, Status::Ready, "{name}");
                assert_eq!(checked.metrics.len(), metrics, "{name}");
            }
            Err(ids) => {
                assert_eq!(violation_ids(&checked), ids, "{name}");
                assert_eq!(checked.status, Status::Error, "{name}");
                assert!(checked.metrics.is_empty(), "{name}");
                let [note] = &checked.notes[..] else {
                    panic!("{name}: {:?}", checked.notes);
                };
                assert_eq!(note.id, INVALID_NOTE, "{name}");
                assert_eq!(param(note, "count"), ids.len().to_string(), "{name}");
                let report = report(checked);
                assert_eq!(
                    FailureCategory::of(&report),
                    Some(FailureCategory::InvalidOutput),
                    "{name}"
                );
            }
        }
    }
}

#[test]
fn violations_name_the_field_and_never_carry_the_printed_content() {
    let secret = "sk-live-0123456789";
    let outputs = [
        edited(|v| v["metrics"][1]["kind"] = json!(secret)),
        edited(|v| v["status"] = json!(secret)),
        edited(|v| v["metrics"][0]["resets_at"] = json!(secret)),
        edited(|v| v["metrics"][0]["unit"] = json!(format!("{secret}{secret}"))),
        edited(|v| v["metrics"][0]["used"] = json!(secret)),
        format!("{{\"schema\": 1, \"status\": \"{secret}").into_bytes(),
        format!("{secret}\n").into_bytes(),
        edited(|v| v["notes"] = json!([secret, 1])),
    ];
    for output in outputs {
        let checked = check("proxy", &output);
        assert!(!checked.violations.is_empty());
        let json = serde_json::to_string(&checked).unwrap();
        assert!(!json.contains(secret), "{json}");
        for text in checked.violations.iter().chain(&checked.notes) {
            for locale in ["pt-BR", "en-US", "es", "it"] {
                assert!(!text.resolve(locale).contains(secret), "{text:?}");
            }
        }
        let report = serde_json::to_string(&report(checked)).unwrap();
        assert!(!report.contains(secret), "{report}");
    }
    let checked = check(
        "proxy",
        &edited(|v| v["metrics"][1]["kind"] = json!("tokens")),
    );
    assert_eq!(param(&checked.violations[0], "field"), "metrics[1].kind");
    assert_eq!(
        checked.violations[0].resolve("en-US"),
        "The field metrics[1].kind must be one of: quota, rate_limit, balance, credits, \
         spend, usage."
    );
}

#[test]
fn the_extension_status_and_notes_never_decide_behavior() {
    for status in [
        "rate_limited",
        "authentication_error",
        "needs_configuration",
        "unsupported",
        "error",
    ] {
        let output = edited(|v| {
            v["status"] = json!(status);
            v["notes"] = json!(["query.timeout", "Token expired."]);
        });
        let checked = check("proxy", &output);
        assert!(checked.violations.is_empty(), "{status}");
        let report = report(checked);
        assert_eq!(report.status, Status::Error, "{status}");
        assert!(report.metrics.is_empty(), "{status}");
        assert_eq!(
            FailureCategory::of(&report),
            Some(FailureCategory::ProviderError),
            "{status}"
        );
        let ids: Vec<&str> = report.notes_l10n.iter().map(|note| &*note.id).collect();
        assert_eq!(
            ids,
            [
                "extensions.output.failed",
                "extensions.output.note",
                "extensions.output.note"
            ]
        );
        assert_eq!(param(&report.notes_l10n[0], "status"), status);
        assert_eq!(report.notes[1..], ["query.timeout", "Token expired."]);
    }
}

#[test]
fn texts_lose_their_markup_and_are_truncated() {
    let long = "word ".repeat(200);
    let output = edited(|v| {
        v["metrics"][0]["label"] = json!(format!("<b>Balance</b> {long}"));
        v["metrics"][1]["period"] = json!(format!("**{long}**"));
        v["notes"] = json!([
            "<script>alert(1)</script> **Low** [balance](https://x.invalid)\u{1b}[31m",
            long,
            "<i></i>"
        ]);
    });
    let checked = check("proxy", &output);
    assert!(checked.violations.is_empty(), "{:?}", checked.violations);
    let label = &checked.metrics[0].label;
    assert!(label.starts_with("Balance word"), "{label}");
    assert_eq!(label.chars().count(), LABEL_MAX_CHARS);
    assert!(label.ends_with('…'));
    let period = checked.metrics[1].period.as_deref().unwrap();
    assert_eq!(period.chars().count(), PERIOD_MAX_CHARS);
    let notes = report(checked).notes;
    assert_eq!(notes.len(), 2, "the note left empty is dropped: {notes:?}");
    assert_eq!(notes[0], "alert(1) Low balance 31m");
    assert_eq!(notes[1].chars().count(), NOTE_MAX_CHARS);
    assert!(notes[1].ends_with('…'));
}

#[test]
fn every_output_text_is_translated() {
    let mut texts: Vec<Localized> = Vec::new();
    let outputs = [
        vec![b' '; MAX_BYTES + 1],
        b"\xff".to_vec(),
        b"".to_vec(),
        b"{".to_vec(),
        b"[]".to_vec(),
        b"{}".to_vec(),
        edited(|v| {
            v["status"] = json!("x");
            v["metrics"] = json!([
                3,
                { "label": "<b></b>", "used": "1", "unit": 1, "kind": "x",
                  "window": "x", "period": 1, "resets_at": "x" },
                { "label": 1 }
            ]);
            v["notes"] = json!(vec![json!("n"); MAX_NOTES + 1]);
        }),
        edited(|v| v["metrics"] = json!({})),
        edited(|v| v["status"] = json!("error")),
        edited(|v| drop(v.as_object_mut().unwrap().remove("status"))),
    ];
    for output in outputs {
        let checked = check("proxy", &output);
        texts.extend(checked.violations);
        texts.extend(checked.notes);
    }
    texts.push(FailureCategory::InvalidOutput.label());
    let ids: BTreeSet<&str> = texts.iter().map(|text| &*text.id).collect();
    assert_eq!(ids.len(), 20, "{ids:?}");
    for locale in ["en-US", "es", "it"] {
        let catalog = crate::text::core_catalog(locale).unwrap();
        let missing: Vec<_> = ids
            .iter()
            .filter(|id| !catalog.contains_key(**id))
            .collect();
        assert!(missing.is_empty(), "{locale} lacks {missing:?}");
        for text in &texts {
            let resolved = text.resolve(locale);
            assert!(!resolved.contains('{'), "{locale}: {resolved}");
        }
    }
}
