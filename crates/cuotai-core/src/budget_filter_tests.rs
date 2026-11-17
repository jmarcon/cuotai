//! PRD 0041 re-validation F-14, F-15: the budget quotas go through the
//! report filter, and a budget problem fails only the strict check.

use super::*;
use chrono::TimeZone;

fn spend(label: &str) -> Metric {
    Metric {
        label: label.into(),
        used: Some(4.0),
        limit: None,
        remaining: None,
        unit: "USD".into(),
        kind: MetricKind::Spend,
        window: Some(MetricWindow::Monthly),
        period: None,
        resets_at: None,
        billing: Default::default(),
        scope: None,
        derived_from: None,
        unlimited: false,
    }
}

fn account(amount: f64) -> Account {
    Account {
        name: "Work".into(),
        provider: Provider::OpenAiApi,
        credential_env: Some("OPENAI_FILTER_TEST".into()),
        budgets: vec![budget::Budget {
            kind: Some(budget::BudgetKind::Spend),
            scope: None,
            label_contains: None,
            amount,
            unit: Some("USD".into()),
            period: budget::BudgetPeriod::Monthly,
            starts: None,
            label: None,
            row: None,
        }],
        ..Default::default()
    }
}

#[test]
fn budget_metrics_and_breakdown_rows_leave_apply_budgets_cleaned() {
    let account = account(50.0);
    let config = Config {
        accounts: vec![account.clone()],
        ..Config::default()
    };
    let mut monitor = Monitor::new(config).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 10, 21, 12, 0, 0).unwrap();
    monitor.clock = Some(Arc::new(parking_lot::Mutex::new(now)));
    // A reading that skipped the query boundary (a reused metered one).
    let mut report = Report {
        account: account.name.clone(),
        account_id: None,
        provider: Provider::OpenAiApi,
        checked_at: now,
        status: Status::Ready,
        metrics: vec![spend("Spend\u{202e}gnp")],
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
        diagnostic: None,
        sub_reports: Vec::new(),
        as_of: None,
        overage_hint: None,
        breakdown: vec![BreakdownRow {
            scope: ScopeKind::Key,
            id: "k1".into(),
            label: "Key\u{2066}one".into(),
            metrics: vec![spend("Row\u{202e}spend")],
            truncated: false,
            notes: Vec::new(),
        }],
    };
    monitor.apply_budgets(&account, &mut report);
    let derived = report
        .metrics
        .iter()
        .find(|metric| metric.derived_from.is_some())
        .expect("the budget metric");
    assert!(
        !derived.label.chars().any(text::is_disguising),
        "{derived:?}"
    );
    let json = serde_json::to_string(&report).unwrap();
    for hidden in ['\u{202e}', '\u{2066}'] {
        assert!(!json.contains(hidden), "{json}");
    }
    assert!(
        !json.contains("\\u202e") && !json.contains("\\u2066"),
        "{json}"
    );
    assert_eq!(report.breakdown[0].label, "Keyone");
}

#[test]
fn an_invalid_budget_fails_only_the_strict_check() {
    let config = Config {
        accounts: vec![account(0.0)],
        ..Config::default()
    };
    assert!(
        config.validate().is_ok(),
        "runtime readers keep the account"
    );
    assert!(matches!(
        config.validate_strict(),
        Err(ConfigError::Budget { field, rule: budget::BudgetRule::Amount })
            if field == "accounts[0].budgets[0].amount"
    ));
    let text = "[[accounts]]\nname = \"A\"\nprovider = \"open_ai_api\"\ncredential_env = \"A\"\n\n\
                [[accounts.budgets]]\nkind = \"spend\"\namount = 0\n\n\
                [[accounts.budgets]]\nkind = \"spend\"\namount = 50\nperiod = \"daily\"\n";
    // `--check-config` (strict) still refuses the file, naming the field.
    let check = config_file::check_bytes(text.as_bytes());
    assert!(!check.valid);
    assert_eq!(
        check.diagnostics[0].path.as_deref(),
        Some("accounts[0].budgets[0].amount")
    );
    // A runtime load keeps the account and both entries, as written.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cuotai.toml");
    std::fs::write(&path, text).unwrap();
    let file = config_file::ConfigFile::load(Some(path), false).expect("tolerant load");
    assert_eq!(file.config.accounts.len(), 1);
    assert_eq!(file.config.accounts[0].budgets.len(), 2);
}
