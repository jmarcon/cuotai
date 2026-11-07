//! PRD 0044 RF5.3 and RF3.1 for `--notify`: delivery to a notification
//! channel on a local endpoint (never a real service), private texts and
//! the shared ledger across two sessions.

use super::*;
use chrono::TimeZone;
use cuotai_core::channels::{self, delivery::Timing, log::DeliveryLog, Draft, Vault};
use cuotai_core::oauth::{OAuthError, OAuthToken, TokenStore};
use cuotai_core::{Metric, MetricKind, MetricWindow, Provider, Status};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::{Arc, Mutex};

fn at(h: u32, m: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 3, 2, h, m, 0).unwrap()
}

fn config() -> Config {
    Config {
        accounts: vec![Account {
            id: None,
            name: "Secret Corp".into(),
            provider: Provider::Claude,
            ..Account::default()
        }],
        ..Config::default()
    }
}

fn reading(checked_at: DateTime<Utc>, used: f64) -> Report {
    Report {
        as_of: Default::default(),
        overage_hint: Default::default(),
        sub_reports: Vec::new(),
        account_id: None,
        diagnostic: None,
        account: "Secret Corp".into(),
        provider: Provider::Claude,
        checked_at,
        status: Status::Ready,
        metrics: vec![Metric {
            billing: Default::default(),
            scope: None,
            label: "5h".into(),
            used: Some(used),
            limit: Some(100.0),
            remaining: None,
            unit: "%".into(),
            kind: MetricKind::Quota,
            window: Some(MetricWindow::Window5h),
            period: None,
            resets_at: Some(at(20, 0)),
            derived_from: None,
        }],
        notes: Vec::new(),
        notes_l10n: Vec::new(),
        source: String::new(),
        session_expires_at: None,
        breakdown: Vec::new(),
    }
}

#[derive(Default)]
struct Memory(Mutex<HashMap<String, OAuthToken>>);

impl TokenStore for Memory {
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

/// A local HTTP endpoint standing in for a self-hosted ntfy server: it
/// answers 200 to every request and keeps the bodies.
struct LocalNtfy {
    port: u16,
    bodies: Arc<Mutex<Vec<String>>>,
}

impl LocalNtfy {
    fn start() -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let kept = Arc::clone(&bodies);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream);
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 {
                        break;
                    }
                    if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = value.trim().parse().unwrap_or(0);
                    }
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                kept.lock()
                    .unwrap()
                    .push(String::from_utf8_lossy(&body).into_owned());
                let _ = reader.get_mut().write_all(
                    b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                );
            }
        });
        Self { port, bodies }
    }

    fn count(&self) -> usize {
        self.bodies.lock().unwrap().len()
    }
}

fn session(dir: &std::path::Path, dispatcher: Option<channels::Dispatcher>) -> Notices {
    let mut notices = Notices::new(at(9, 0), false)
        .shared(
            notify_prefs::Source::new(Some(dir.join("desktop.json"))),
            SharedLedger::open(Some(dir.join("notified.json"))),
        )
        .with_channels(dispatcher);
    notices.zone = chrono::FixedOffset::east_opt(0);
    notices
}

fn observe(notices: &mut Notices, prev: Option<&Report>, next: &Report, now: DateTime<Utc>) {
    let events = notices.assess(prev, next, now, None);
    notices.record(next, events, now, None);
    notices.tick(&config(), &[Some(next)], now, "en-US");
}

/// The 90% crossing reaches the channel once although two sessions (the
/// desktop and `--notify`) share the ledger, and the message hides the
/// account name by default; neither the preferences nor the log hold the
/// address.
#[tokio::test]
async fn one_crossing_reaches_the_channel_once_with_names_hidden() {
    let dir = tempfile::tempdir().unwrap();
    let server = LocalNtfy::start();
    let settings = dir.path().join("desktop.json");
    let vault = Vault::new(Arc::new(Memory::default()));
    let draft = Draft {
        kind: channels::ChannelKind::Ntfy,
        label: "Phone".into(),
        filters: channels::Filters::default(),
        sign: false,
    };
    let url = format!("http://127.0.0.1:{}/cuotai-test", server.port);
    channels::add(&settings, &vault, &draft, &url, at(8, 0)).unwrap();
    let dispatcher = || {
        channels::Dispatcher::with(
            settings.clone(),
            vault.clone(),
            Some(DeliveryLog::at(dir.path().join("log.jsonl"))),
            at(8, 0),
            Timing::default(),
        )
    };
    let mut desktop = session(dir.path(), dispatcher());
    let mut tui = session(dir.path(), dispatcher());
    let before = reading(at(10, 0), 70.0);
    let after = reading(at(10, 5), 92.0);
    for notices in [&mut desktop, &mut tui] {
        observe(notices, Some(&before), &after, at(10, 5));
    }
    for _ in 0..300 {
        if server.count() > 0 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert_eq!(
        server.count(),
        1,
        "one crossing, one message: {:?}",
        server.bodies.lock().unwrap()
    );
    let body = server.bodies.lock().unwrap()[0].clone();
    assert!(
        !body.contains("Secret Corp"),
        "names hidden by default: {body}"
    );
    let saved = std::fs::read_to_string(&settings).unwrap();
    let log = std::fs::read_to_string(dir.path().join("log.jsonl")).unwrap();
    assert!(!saved.contains("cuotai-test") && !log.contains("cuotai-test"));
    assert!(log.contains("\"delivered\""));
}
