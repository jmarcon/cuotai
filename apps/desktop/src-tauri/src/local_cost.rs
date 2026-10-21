//! PRD 0015 RF1/RF2 in the desktop app: Settings → Local cost turns each
//! source on only after its disclosure was shown and accepted (the core's
//! `Disclosure`, matched by digest), off with everything derived from it,
//! sets the period and the index retention, erases the index, and shows
//! what the core's background `CostWorker` last found. The worker runs on
//! a thread of its own, never on the refresh cycle.

use cuotai_core::cost::{
    self, CostError, CostIndex, CostStatus, CostWorker, Disclosure, Env, Indexed, Period,
    Retention, Scanner, SettingsStore, SourceId, WorkerState,
};
use cuotai_core::text::Localized;
use serde::Serialize;
use std::sync::Mutex;

/// The local cost state of the app.
pub struct LocalCost {
    stores: Option<(SettingsStore, CostIndex)>,
    env: Env,
    worker: Mutex<Option<CostWorker>>,
}

/// One source in Settings → Local cost.
#[derive(Serialize)]
pub struct SourceView {
    source: SourceId,
    label: &'static str,
    enabled: bool,
    consented_at: Option<chrono::DateTime<chrono::Utc>>,
    /// What turning it on reads now; shown before the consent.
    disclosure: Disclosure,
    indexed: Indexed,
    /// The last scan's sentence about the source, while it is on.
    summary: Option<Localized>,
}

#[derive(Serialize)]
pub struct PeriodView {
    id: Period,
    label: Localized,
}

/// Settings → Local cost.
#[derive(Serialize)]
pub struct CostView {
    sources: Vec<SourceView>,
    period: Period,
    periods: Vec<PeriodView>,
    retention_days: u32,
    retention_min: u32,
    retention_max: u32,
    state: WorkerState,
    error: Option<Localized>,
    size: u64,
}

fn l10n(error: CostError) -> String {
    serde_json::to_string(&error.l10n()).unwrap_or_else(|_| error.to_string())
}

impl LocalCost {
    /// The stores of this computer; without a local data directory every
    /// command fails with the core's storage error and nothing is read.
    pub fn new() -> Self {
        let stores = SettingsStore::in_data_dir()
            .and_then(|settings| Ok((settings, CostIndex::in_data_dir()?)))
            .ok();
        Self {
            stores,
            env: Env::system(),
            worker: Mutex::new(None),
        }
    }

    /// RF2.2: start the background scan (it reads only the preferences
    /// while every source is off).
    pub fn start(&self) {
        let Some((settings, index)) = self.stores.clone() else {
            return;
        };
        let worker = CostWorker::spawn(Scanner::new(index, settings, self.env.clone()));
        *self.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
    }

    fn stores(&self) -> Result<&(SettingsStore, CostIndex), String> {
        self.stores
            .as_ref()
            .ok_or_else(|| l10n(CostError::Settings))
    }

    fn worker_status(&self) -> CostStatus {
        self.worker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(CostWorker::status)
            .unwrap_or_default()
    }

    fn wake(&self) {
        if let Some(worker) = self
            .worker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
        {
            worker.wake();
        }
    }

    pub fn view(&self) -> Result<CostView, String> {
        let (settings, index) = self.stores()?;
        let saved = settings.load().map_err(l10n)?;
        let status = self.worker_status();
        let mut sources = Vec::new();
        for source in SourceId::ALL {
            let enabled = saved.enabled(source, &self.env);
            let summary = status
                .last
                .as_ref()
                .filter(|_| enabled)
                .and_then(|report| report.sources.iter().find(|s| s.source == source))
                .map(cost::SourceScan::summary);
            sources.push(SourceView {
                source,
                label: source.label(),
                enabled,
                consented_at: saved
                    .sources
                    .get(&source)
                    .and_then(|s| s.consented_at)
                    .filter(|_| enabled),
                disclosure: saved.disclosure(source, &self.env),
                indexed: index.indexed(source).map_err(l10n)?,
                summary,
            });
        }
        Ok(CostView {
            sources,
            period: saved.period,
            periods: Period::ALL
                .into_iter()
                .map(|id| PeriodView {
                    id,
                    label: id.label(),
                })
                .collect(),
            retention_days: saved.retention().keep_days(),
            retention_min: Retention::MIN_DAYS,
            retention_max: Retention::MAX_DAYS,
            state: status.state,
            error: status.error,
            size: index.size_on_disk().map_err(l10n)?,
        })
    }

    /// RF1.1: the user accepted the disclosure `digest` the UI showed for
    /// `source`; refused when the directories changed since.
    pub fn enable(&self, source: SourceId, digest: &str) -> Result<CostView, String> {
        let (settings, _) = self.stores()?;
        let chosen = settings
            .load()
            .map_err(l10n)?
            .sources
            .get(&source)
            .map(|s| s.paths.clone())
            .unwrap_or_default();
        settings
            .enable(source, chosen, digest, &self.env, chrono::Utc::now())
            .map_err(l10n)?;
        self.wake();
        self.view()
    }

    /// RF2.4: off, and everything derived from `source` erased.
    pub fn disable(&self, source: SourceId) -> Result<CostView, String> {
        let (settings, index) = self.stores()?;
        cost::turn_off(settings, index, source).map_err(l10n)?;
        self.wake();
        self.view()
    }

    /// RF2.3: the period shown and the days the index keeps.
    pub fn set_options(&self, period: Period, retention_days: u32) -> Result<CostView, String> {
        let (settings, _) = self.stores()?;
        let retention = Retention::days(retention_days).map_err(l10n)?;
        settings.set_period(period).map_err(l10n)?;
        settings.set_retention(retention).map_err(l10n)?;
        self.wake();
        self.view()
    }

    /// RF2.4: erase the whole index; returns the rows removed.
    pub fn erase(&self) -> Result<usize, String> {
        let (_, index) = self.stores()?;
        let removed = index.erase().map_err(l10n)?;
        self.wake();
        Ok(removed)
    }
}
