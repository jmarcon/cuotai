//! PRD 0041 re-validation (F-06, F-09): the `--once` block of a budget in
//! German and Japanese: the period is a day in the reader's format, never
//! the RFC 3339 start, and the budget notes name the budget in the
//! reader's language.

use std::time::Duration;

use chrono::{DateTime, TimeZone, Utc};
use cuotai_core::budget::{Budget, BudgetKind, BudgetPeriod, NoHistory};
use cuotai_core::{Account, Billing, Metric, MetricKind, MetricWindow, Provider, Report, Status};

fn at(d: u32, h: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 10, d, h, 0, 0).unwrap()
}

fn reading() -> Report {
    let spend = Metric {
        label: "Gasto no mês".into(),
        used: Some(12.5),
        limit: None,
        remaining: None,
        unit: "USD".into(),
        kind: MetricKind::Spend,
        window: Some(MetricWindow::Monthly),
        period: None,
        resets_at: Some(Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap()),
        billing: Billing::Included,
        scope: None,
        derived_from: None,
        unlimited: false,
    };
    let mut report = Report {
        account: "Work".into(),
        account_id: None,
        provider: Provider::OpenAiApi,
        checked_at: at(21, 12),
        status: Status::Ready,
        metrics: vec![spend],
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
        diagnostic: None,
        sub_reports: Vec::new(),
        as_of: None,
        overage_hint: None,
        breakdown: Vec::new(),
    };
    let budget = |amount, period| Budget {
        kind: Some(BudgetKind::Spend),
        scope: None,
        label_contains: None,
        amount,
        unit: Some("USD".into()),
        period,
        starts: None,
        label: None,
        row: None,
    };
    let account = Account {
        name: "Work".into(),
        provider: Provider::OpenAiApi,
        budgets: vec![
            budget(50.0, BudgetPeriod::Monthly),
            budget(20.0, BudgetPeriod::Weekly),
        ],
        ..Default::default()
    };
    cuotai_core::budget::apply(&account, &mut report, &NoHistory, at(21, 12), &Utc);
    report
}

fn once(locale: &'static str) -> String {
    let report = reading();
    crate::l10n::with_locale(locale, || {
        crate::report_text(
            &report,
            &[],
            None,
            Duration::from_secs(300),
            at(21, 12),
            locale,
            |t| t.format("%H:%M").to_string(),
        )
    })
}

const GOLDEN_DE: &str = "
Work / OpenAI API — Aktuell · jetzt abgefragt
  Ausgaben im Monat: 12,50\u{a0}$
    Zyklus endet: 00:00
  Monatsbudget · Ausgaben im Monat: 12,50\u{a0}$ / 50,00\u{a0}$ (37,50\u{a0}$ übrig)
    Zeitraum: seit 01.10.
    Erneuert: 00:00
    Ihr Budget
    Tempo: 41 Punkte Reserve · reicht bis zur Zurücksetzung (lineare Schätzung)
    Kontingent: ≈ 3,57\u{a0}$/Tag im Budget übrig
  Wochenbudget · Ausgaben im Monat: —\u{a0}USD / 20,00\u{a0}$
    Zeitraum: seit Mo.
    Erneuert: 00:00
    Ihr Budget
  Wochenbudget · Ausgaben im Monat: Die Zählung beginnt mit der ersten Messung dieses Zeitraums.
";

const GOLDEN_JA: &str = "
Work / OpenAI API — 最新 · たった今照会
  今月の支出: $12.50
    サイクル終了：00:00
  月次予算 · 今月の支出: $12.50 / $50.00 （残り $37.50）
    期間：10/01から
    リセット：00:00
    自分の予算
    ペース：41 ポイントの余裕 · リセットまで持ちます（線形推定）
    使用可能量：≈ 1 日 $3.57 予算に残り
  週次予算 · 今月の支出: —\u{a0}USD / $20.00
    期間：月から
    リセット：00:00
    自分の予算
  週次予算 · 今月の支出：この期間の最初の測定から集計を始めます。
";

/// The block of two budgets, golden in German and Japanese.
#[test]
fn once_words_a_budget_period_and_notes_in_german_and_japanese() {
    assert_eq!(once("de"), GOLDEN_DE);
    assert_eq!(once("ja"), GOLDEN_JA);
}
