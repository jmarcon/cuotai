//! PRD 0015 RF1/RF2/RF4 in the desktop app: Settings → Local cost turns
//! each source on only after its disclosure was shown and accepted (the
//! core's `Disclosure`, matched by digest), off with everything derived
//! from it, chooses its directories (a new list needs a new consent), sets
//! the period and the index retention, shows the prices in effect, erases
//! the index, and shows what the core's background `CostWorker` last found.
//! The cards and details read the core's `CostSummary` (RF4.1) and the
//! local server publishes it as `local_cost` (RF4.3). The worker runs on a
//! thread of its own, never on the refresh cycle.

use cuotai_core::cost::{
    self, CostError, CostIndex, CostStatus, CostSummary, CostWorker, Disclosure, Env, Indexed,
    Period, PriceRow, Prices, PricingFile, Retention, Scanner, SettingsStore, SourceId, TableInfo,
    WorkerState,
};
use cuotai_core::text::Localized;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Mutex;

/// The files of this computer's local cost.
#[derive(Clone)]
struct Stores {
    settings: SettingsStore,
    index: CostIndex,
    pricing: PricingFile,
}

/// The local cost state of the app.
pub struct LocalCost {
    stores: Option<Stores>,
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
    /// The directories chosen instead of the defaults, as saved (`~/` kept);
    /// empty: the defaults.
    paths: Vec<PathBuf>,
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

/// The user's `pricing.json` in Settings → Local cost.
#[derive(Serialize)]
pub struct PricingFileView {
    path: PathBuf,
    /// It exists, is valid and prices models over the embedded table.
    used: bool,
    /// Why it is ignored as a whole.
    error: Option<Localized>,
}

/// RF4.1/RF3: the prices in effect, model by model, in Settings → Local
/// cost; reading them reads no log.
#[derive(Serialize)]
pub struct PricesView {
    currency: &'static str,
    notice: Localized,
    /// The embedded table first, then the user's.
    tables: Vec<TableInfo>,
    pricing_file: PricingFileView,
    models: Vec<PriceRow>,
}

fn l10n(error: CostError) -> String {
    serde_json::to_string(&error.l10n()).unwrap_or_else(|_| error.to_string())
}

impl LocalCost {
    /// The stores of this computer; without a local data directory every
    /// command fails with the core's storage error and nothing is read.
    pub fn new() -> Self {
        let stores = (|| {
            Ok::<_, CostError>(Stores {
                settings: SettingsStore::in_data_dir()?,
                index: CostIndex::in_data_dir()?,
                pricing: PricingFile::in_data_dir()?,
            })
        })()
        .ok();
        Self {
            stores,
            env: Env::system(),
            worker: Mutex::new(None),
        }
    }

    fn scanner(stores: &Stores, env: &Env) -> Scanner {
        Scanner::new(stores.index.clone(), stores.settings.clone(), env.clone())
    }

    /// RF2.2: start the background scan (it reads only the preferences
    /// while every source is off).
    pub fn start(&self) {
        let Some(stores) = &self.stores else {
            return;
        };
        let worker = CostWorker::spawn(Self::scanner(stores, &self.env));
        *self.worker.lock().unwrap_or_else(|e| e.into_inner()) = Some(worker);
    }

    fn stores(&self) -> Result<&Stores, String> {
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
        let stores = self.stores()?;
        let saved = stores.settings.load().map_err(l10n)?;
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
            let chosen = saved.sources.get(&source);
            sources.push(SourceView {
                source,
                label: source.label(),
                enabled,
                consented_at: chosen.and_then(|s| s.consented_at).filter(|_| enabled),
                paths: chosen.map(|s| s.paths.clone()).unwrap_or_default(),
                disclosure: saved.disclosure(source, &self.env),
                indexed: stores.index.indexed(source).map_err(l10n)?,
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
            size: stores.index.size_on_disk().map_err(l10n)?,
        })
    }

    /// RF4.1: the cards' and details' estimate: per source today, yesterday
    /// and the saved period, by day and by model, over the background
    /// scan's last report. Reads the index, never a log.
    pub fn usage(&self) -> Result<CostSummary, String> {
        let stores = self.stores()?;
        let last = self.worker_status().last;
        Self::scanner(stores, &self.env)
            .summary(&stores.pricing, last.as_ref(), None, chrono::Utc::now())
            .map_err(l10n)
    }

    /// RF4.3: the local server's `local_cost`, `None` while every source
    /// is off or the index cannot be read.
    pub fn export(&self) -> Option<CostSummary> {
        self.usage().ok().filter(CostSummary::any_on)
    }

    /// RF1.1: the user accepted the disclosure `digest` the UI showed for
    /// `source`; refused when the directories changed since.
    pub fn enable(&self, source: SourceId, digest: &str) -> Result<CostView, String> {
        let settings = &self.stores()?.settings;
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
        let stores = self.stores()?;
        cost::turn_off(&stores.settings, &stores.index, source).map_err(l10n)?;
        self.wake();
        self.view()
    }

    /// RF4.1: read `paths` (blank lines dropped; none: the defaults) for
    /// `source`. A different list turns a source that is on off, erasing
    /// what was derived; Settings then shows the new list's disclosure.
    pub fn set_paths(&self, source: SourceId, paths: Vec<String>) -> Result<CostView, String> {
        let stores = self.stores()?;
        let paths = paths
            .iter()
            .map(|path| path.trim())
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .collect();
        cost::choose_paths(&stores.settings, &stores.index, source, paths, &self.env)
            .map_err(l10n)?;
        self.wake();
        self.view()
    }

    /// RF2.3: the period shown and the days the index keeps.
    pub fn set_options(&self, period: Period, retention_days: u32) -> Result<CostView, String> {
        let settings = &self.stores()?.settings;
        let retention = Retention::days(retention_days).map_err(l10n)?;
        settings.set_period(period).map_err(l10n)?;
        settings.set_retention(retention).map_err(l10n)?;
        self.wake();
        self.view()
    }

    /// RF4.1: the prices in effect: the embedded table under the user's
    /// `pricing.json`, or the embedded one alone when that file is invalid.
    pub fn prices(&self) -> Result<PricesView, String> {
        let pricing = &self.stores()?.pricing;
        let (prices, error) = Prices::with_user_file(pricing);
        Ok(PricesView {
            currency: cost::CURRENCY,
            notice: cost::estimate_notice(),
            tables: prices.info(),
            pricing_file: PricingFileView {
                path: pricing.path().to_path_buf(),
                used: prices.user().is_some(),
                error: error.as_ref().map(cost::pricing_ignored),
            },
            models: prices.rows(),
        })
    }

    /// RF2.4: erase the whole index; returns the rows removed.
    pub fn erase(&self) -> Result<usize, String> {
        let removed = self.stores()?.index.erase().map_err(l10n)?;
        self.wake();
        Ok(removed)
    }
}
