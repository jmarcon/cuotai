//! PRD 0044 RF3.1 (re-validation S-04): a failing channel frees its
//! delivery slot while it waits to retry. Local wiremock servers only.

use super::delivery::{Dispatcher, Timing, MAX_IN_FLIGHT};
use super::log::{DeliveryLog, LogOutcome};
use super::payload::Source;
use super::*;
use crate::account_id::AccountId;
use crate::alerts::{AlertEvent, AlertKind};
use crate::oauth::OAuthError;
use crate::{Account, MetricWindow, Provider};
use chrono::{TimeDelta, TimeZone};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use wiremock::matchers::path;
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Default)]
struct MemoryStore(Mutex<HashMap<String, OAuthToken>>);

impl TokenStore for MemoryStore {
    fn load(&self, id: &str) -> Result<Option<OAuthToken>, OAuthError> {
        Ok(self.0.lock().unwrap().get(id).cloned())
    }
    fn save(&self, id: &str, token: &OAuthToken) -> Result<(), OAuthError> {
        self.0.lock().unwrap().insert(id.into(), token.clone());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), OAuthError> {
        self.0.lock().unwrap().remove(id);
        Ok(())
    }
}

fn t0() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 2, 14, 0, 0).unwrap()
}

fn account(id: &str) -> Account {
    Account {
        name: "Work".into(),
        provider: Provider::Claude,
        id: AccountId::parse(id),
        ..Account::default()
    }
}

fn threshold(account: &Account) -> Source {
    Source::Alerts {
        account: account.clone(),
        events: vec![AlertEvent {
            account_key: account.state_key(),
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
    }
}

/// A webhook channel posting to `url`, limited to the account `only`.
fn channel(settings: &std::path::Path, vault: &Vault, url: &str, only: &str) -> Channel {
    let channel = Channel {
        id: new_id(),
        kind: ChannelKind::Webhook,
        label: "Mock".into(),
        host: "127.0.0.1".into(),
        filters: Filters {
            accounts: [only.to_owned()].into(),
            ..Filters::default()
        },
        enabled: true,
        created_at: t0(),
        signed: false,
    };
    vault
        .put(&Vault::url_key(&channel.id), url, settings)
        .unwrap();
    let stored = channel.clone();
    mutate(settings, move |channels| {
        channels.push(stored);
        Ok(())
    })
    .unwrap();
    channel
}

#[tokio::test]
async fn a_channel_waiting_to_retry_does_not_hold_a_delivery_slot() {
    assert_eq!(MAX_IN_FLIGHT, 2, "two failing channels fill every slot");
    let server = MockServer::start().await;
    Mock::given(path("/down"))
        .respond_with(ResponseTemplate::new(500))
        .expect(6)
        .mount(&server)
        .await;
    Mock::given(path("/up"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let settings = dir.path().join("desktop.json");
    let log = DeliveryLog::at(dir.path().join("channels").join("log.jsonl"));
    let vault = Vault::new(std::sync::Arc::new(MemoryStore::default()));
    let url = |route: &str| format!("{}{route}", server.uri());
    let first = channel(&settings, &vault, &url("/down"), "acc_failing");
    let second = channel(&settings, &vault, &url("/down"), "acc_failing");
    let healthy = channel(&settings, &vault, &url("/up"), "acc_healthy");
    let backoff = Duration::from_millis(1500);
    let timing = Timing {
        timeout: Duration::from_secs(2),
        backoff: [backoff, backoff],
        startup_grace: TimeDelta::zero(),
        ..Timing::default()
    };
    let dispatcher = Dispatcher::with(
        settings.clone(),
        vault.clone(),
        Some(log.clone()),
        t0() - TimeDelta::hours(1),
        timing,
    )
    .unwrap()
    .loose();
    // The two failing channels take both slots and get their first 500.
    assert_eq!(
        dispatcher.dispatch(&threshold(&account("acc_failing")), true, "en-US", t0()),
        2
    );
    for _ in 0..200 {
        if server.received_requests().await.unwrap_or_default().len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // While both wait to retry, the healthy channel is delivered at once.
    let asked = Instant::now();
    assert_eq!(
        dispatcher.dispatch(&threshold(&account("acc_healthy")), true, "en-US", t0()),
        1
    );
    let delivered = loop {
        if let Some(position) = log.read().iter().position(|e| e.channel == healthy.id) {
            break position;
        }
        assert!(
            asked.elapsed() < backoff,
            "the healthy channel waited for a failing channel's backoff"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    assert_eq!(delivered, 0, "delivered before the failing channels end");
    // The failing channels still retry and fail after three attempts.
    for _ in 0..2000 {
        if log.read().len() >= 3 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let entries = log.read();
    for failing in [&first, &second] {
        let entry = entries.iter().find(|e| e.channel == failing.id).unwrap();
        assert_eq!(entry.outcome, LogOutcome::Failed);
        assert_eq!(entry.attempt, 3);
    }
    assert_eq!(
        entries
            .iter()
            .find(|e| e.channel == healthy.id)
            .unwrap()
            .outcome,
        LogOutcome::Delivered
    );
}
