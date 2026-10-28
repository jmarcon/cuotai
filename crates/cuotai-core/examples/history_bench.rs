//! Timing harness for the history store on a large synthetic file: no
//! network, no credentials, no real account. Run with
//! `cargo run --release -p cuotai-core --example history_bench [days]`.

use chrono::{TimeDelta, Utc};
use cuotai_core::config_file::ConfigIdentity;
use cuotai_core::history::{ExportFormat, HistoryStore};
use cuotai_core::stats::{self, StatsRange};
use cuotai_core::{Config, Metric, MetricKind, MetricWindow};
use std::time::Instant;

const ACCOUNTS: usize = 4;
const METRICS: usize = 6;
const STEP_MINUTES: i64 = 5;

fn config() -> Config {
    let mut toml = String::new();
    for i in 0..ACCOUNTS {
        toml.push_str(&format!(
            "[[accounts]]\nname = \"acct{i}\"\nprovider = \"groq\"\ncredential_env = \"GROQ_KEY_{i}\"\n"
        ));
    }
    Config::from_toml(&toml).expect("valid config")
}

fn metrics(tick: i64) -> Vec<Metric> {
    (0..METRICS)
        .map(|m| {
            let window = if m % 2 == 0 {
                MetricWindow::Daily
            } else {
                MetricWindow::Monthly
            };
            let period = if m % 2 == 0 { 288 } else { 288 * 30 };
            let used = (tick % period) as f64 / period as f64 * 100.0;
            let start = tick - tick % period;
            Metric {
                label: format!("metric {m}"),
                used: Some(used),
                limit: Some(100.0),
                remaining: Some(100.0 - used),
                unit: "%".into(),
                kind: MetricKind::Quota,
                window: Some(window),
                period: None,
                resets_at: Some(base() + TimeDelta::minutes((start + period) * STEP_MINUTES)),
                billing: Default::default(),
                scope: None,
            }
        })
        .collect()
}

fn base() -> chrono::DateTime<Utc> {
    let days: i64 = std::env::args()
        .nth(1)
        .and_then(|d| d.parse().ok())
        .unwrap_or(30);
    // Fixed at first use so every tick shares one origin.
    static BASE: std::sync::OnceLock<chrono::DateTime<Utc>> = std::sync::OnceLock::new();
    *BASE.get_or_init(|| Utc::now() - TimeDelta::days(days) + TimeDelta::minutes(1))
}

fn main() {
    let days: i64 = std::env::args()
        .nth(1)
        .and_then(|d| d.parse().ok())
        .unwrap_or(30);
    // `HISTORY_BENCH_DIR` keeps the file between runs (the recording is
    // skipped when it exists); otherwise a temporary directory is used.
    let tmp = tempfile::tempdir().expect("tempdir");
    let dir = std::env::var_os("HISTORY_BENCH_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| tmp.path().to_path_buf());
    std::fs::create_dir_all(&dir).expect("bench dir");
    let path = dir.join("history.sqlite3");
    let reuse = path.exists();
    let store = HistoryStore::new(path, &ConfigIdentity::Explicit("bench".into()));
    let config = config();
    let ticks = days * 24 * 60 / STEP_MINUTES;
    if reuse {
        println!("reusing the recorded store");
    } else {
        let t = Instant::now();
        let mut rows = 0;
        for tick in 0..ticks {
            let at = base() + TimeDelta::minutes(tick * STEP_MINUTES);
            let metrics = metrics(tick);
            for account in &config.accounts {
                rows += store.record(account, at, &metrics).expect("record");
            }
        }
        let record = t.elapsed();
        let calls = ticks as usize * ACCOUNTS;
        println!(
            "record: {calls} calls, {rows} rows, {:?} total, {:.1} us/call",
            record,
            record.as_secs_f64() * 1e6 / calls as f64
        );
    }
    let skip_prune = std::env::var_os("HISTORY_BENCH_NO_PRUNE").is_some();
    println!("size on disk: {} bytes", store.size_on_disk().unwrap());

    let t = Instant::now();
    let latest = store.latest_reports(&config).expect("latest");
    println!(
        "latest_reports: {:?} ({} accounts)",
        t.elapsed(),
        latest.iter().flatten().count()
    );

    let now = Utc::now();
    // `HISTORY_BENCH_LOOP=n` repeats the statistics, long enough to sample.
    let loops: usize = std::env::var("HISTORY_BENCH_LOOP")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    for _ in 0..loops {
        stats::summarize(&store, &config.accounts, StatsRange::Days30, now, &Utc)
            .expect("summarize");
    }
    let t = Instant::now();
    // Only timings: the summary holds per-account history.
    let _summary = stats::summarize(&store, &config.accounts, StatsRange::Days30, now, &Utc)
        .expect("summarize");
    println!("summarize(30d): {:?}", t.elapsed());

    let t = Instant::now();
    let mut points = 0;
    for account in &config.accounts {
        let h = store
            .account_history(account, now - TimeDelta::days(30)..=now)
            .expect("history");
        points += h.iter().map(|m| m.points.len()).sum::<usize>();
    }
    println!(
        "account_history(30d, all accounts): {:?} ({points} points)",
        t.elapsed()
    );

    let t = Instant::now();
    let mut out = Vec::new();
    let n = store
        .export(&config.accounts, None, .., ExportFormat::Csv, &mut out)
        .expect("export");
    println!(
        "export csv: {:?} ({n} rows, {} bytes)",
        t.elapsed(),
        out.len()
    );

    let t = Instant::now();
    let bounds = store.bounds().expect("bounds");
    println!("bounds: {:?} {bounds:?}", t.elapsed());

    if skip_prune {
        return;
    }
    let t = Instant::now();
    let report = store.prune(now).expect("prune");
    println!("prune: {:?} {report:?}", t.elapsed());

    let t = Instant::now();
    let _summary = stats::summarize(&store, &config.accounts, StatsRange::Days30, now, &Utc)
        .expect("summarize");
    println!("summarize(30d) after prune: {:?}", t.elapsed());
}
