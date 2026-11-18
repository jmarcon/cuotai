//! PRD 0044 RF1.3 (review R-01): an aggregator account in a channel's
//! account filter covers its connections, whose ids are `<parent>/<sub>`.

use super::payload::{prepare, Source};
use super::*;
use crate::account_id::AccountId;
use crate::alerts::{AlertEvent, AlertKind};
use crate::{Account, MetricWindow, Provider};
use chrono::TimeZone;

fn only(ids: &[&str]) -> Filters {
    Filters {
        accounts: ids.iter().map(|id| (*id).to_owned()).collect(),
        ..Filters::default()
    }
}

#[test]
fn the_parent_of_a_connection_id_is_its_aggregator() {
    assert_eq!(AccountId::parent("acc_x/c1"), Some("acc_x"));
    assert_eq!(
        AccountId::parent(AccountId::connection("acc_x", "c1").as_str()),
        Some("acc_x")
    );
    assert_eq!(AccountId::parent("acc_x"), None);
}

#[test]
fn an_aggregator_in_the_filter_covers_its_connections_only() {
    assert!(only(&["acc_x"]).admits_account("acc_x/c1"));
    assert!(only(&["acc_x"]).admits_account("acc_x"));
    assert!(!only(&["acc_x"]).admits_account("acc_y/c1"));
    assert!(!only(&["acc_x"]).admits_account("acc_xy/c1"));
    assert!(only(&["acc_x/c1"]).admits_account("acc_x/c1"));
    assert!(!only(&["acc_x/c1"]).admits_account("acc_x"));
    assert!(!only(&["acc_x/c1"]).admits_account("acc_x/c2"));
    assert!(Filters::default().admits_account("acc_y/c1"));
}

#[test]
fn a_connection_alert_reaches_a_channel_limited_to_its_aggregator() {
    let connection = Account {
        name: "Team key".into(),
        provider: Provider::Claude,
        id: Some(AccountId::connection("acc_x", "c1")),
        ..Account::default()
    };
    let now = Utc.with_ymd_and_hms(2026, 5, 4, 12, 0, 0).unwrap();
    let source = Source::Alerts {
        account: connection.clone(),
        events: vec![AlertEvent {
            account_key: connection.state_key(),
            label: "Session (5h)".into(),
            window: Some(MetricWindow::Window5h),
            kind: AlertKind::Threshold,
            threshold: 90,
            consumed: 0.9,
            resets_at: None,
            projection: None,
            overage: None,
            limit_change: None,
            row: None,
            mark_key: None,
        }],
    };
    let utc = chrono::FixedOffset::east_opt(0).unwrap();
    let out = prepare(&source, &only(&["acc_x"]), true, "en-US", now, &utc)
        .expect("the aggregator covers its connection");
    assert_eq!(out.account_id.as_deref(), Some("acc_x/c1"));
    assert!(prepare(&source, &only(&["acc_y"]), true, "en-US", now, &utc).is_none());
}
