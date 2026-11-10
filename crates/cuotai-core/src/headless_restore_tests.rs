//! PRD 0010 RF2.1/RF2.3 (re-validation S-01): every front end restores the
//! kept readings through [`Monitor::restored_readings`] (TUI launch and
//! reload, desktop install, `--json-v1`, headless launch and `SIGHUP`
//! reload); a reading taken with other account settings never comes back.

use super::*;
use crate::history::HistoryStore;
use crate::{Account, Metric, MetricKind};

const TOML: &str = "[[accounts]]\nid = \"acc_0123456789ab\"\nname = \"alpha\"\nprovider = \"groq\"\ncredential_env = \"GROQ_KEY_A\"\n";

fn store(dir: &tempfile::TempDir) -> HistoryStore {
    HistoryStore::new(
        dir.path().join("history.sqlite3"),
        &ConfigIdentity::Explicit("restore".into()),
    )
}

fn monitor(config: &Config, history: &HistoryStore) -> Arc<Monitor> {
    Arc::new(
        Monitor::new(config.clone())
            .unwrap()
            .with_history(history.clone()),
    )
}

fn edited(config: &Config, edit: impl FnOnce(&mut Account)) -> Config {
    let mut config = config.clone();
    edit(&mut config.accounts[0]);
    config
}

fn kept(driver: &Driver) -> bool {
    lock(driver.cycle()).last_good_report(0).is_some()
}

#[test]
fn every_restore_path_drops_a_reading_taken_with_other_settings() {
    let dir = tempfile::tempdir().unwrap();
    let history = store(&dir);
    let config = Config::from_toml(TOML).unwrap();
    let metric = Metric {
        billing: Default::default(),
        scope: None,
        label: "5h".into(),
        used: Some(42.0),
        limit: Some(100.0),
        remaining: None,
        unit: "%".into(),
        kind: MetricKind::Quota,
        window: None,
        period: None,
        resets_at: None,
        derived_from: None,
    };
    history
        .record(
            &config.accounts[0],
            Utc::now() - chrono::TimeDelta::minutes(1),
            &[metric],
        )
        .unwrap();
    let renamed = edited(&config, |account| account.name = "renamed".into());
    let rekeyed = edited(&config, |account| {
        account.credential_env = Some("GROQ_KEY_B".into());
    });

    // The funnel every front end reads.
    assert!(monitor(&config, &history).restored_readings()[0].is_some());
    assert!(monitor(&renamed, &history).restored_readings()[0].is_some());
    assert!(monitor(&rekeyed, &history).restored_readings()[0].is_none());

    // Headless launch.
    let now = Instant::now();
    assert!(kept(&Driver::new(&config, monitor(&config, &history), now)));
    let mut driver = Driver::new(&rekeyed, monitor(&rekeyed, &history), now);
    assert!(!kept(&driver), "launch with another key");

    // `SIGHUP` reload: back to the original key restores, a new key drops.
    driver.reload(&rekeyed, &config, monitor(&config, &history), now);
    assert!(kept(&driver));
    driver.reload(&config, &renamed, monitor(&renamed, &history), now);
    assert!(kept(&driver), "a rename keeps it");
    driver.reload(&renamed, &rekeyed, monitor(&rekeyed, &history), now);
    assert!(!kept(&driver), "reload with another key");
}
