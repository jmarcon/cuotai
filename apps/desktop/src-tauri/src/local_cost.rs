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
use cuotai_core::Account;
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

/// PRD 0030 RF6: the typed display currency, with its age for the lines.
#[derive(Serialize)]
pub struct DisplayView {
    code: String,
    rate: f64,
    set_on: chrono::NaiveDate,
    age_days: u32,
    stale: bool,
}

/// PRD 0030 RF1/RF6: the plan prices, the display currency and the plan
/// value lines; shown in Settings → Local cost and on the cards. Typed by
/// the person, kept in the machine-local cost settings, never exported.
#[derive(Serialize)]
pub struct PlansView {
    /// The configured accounts a source reads, each with its plan.
    accounts: Vec<cost::PlanAccount>,
    display: Option<DisplayView>,
    /// One line per source with a plan (RF1.2/RF1.3).
    values: Vec<cost::PlanValue>,
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

    /// PRD 0030 RF1/RF6: the plan accounts, the display currency and the
    /// plan values of `accounts` (the configured ones). Plans still saved
    /// under a legacy `provider:name` key move to the account id first.
    /// Reads the index, never a log.
    pub fn plans(&self, accounts: &[Account]) -> Result<PlansView, String> {
        let stores = self.stores()?;
        stores.settings.migrate_plans(accounts).map_err(l10n)?;
        let saved = stores.settings.load().map_err(l10n)?;
        let now = chrono::Utc::now();
        let scanner = Self::scanner(stores, &self.env);
        let today = scanner.zone().day(now);
        let values = scanner
            .plan_values(&stores.pricing, accounts, now)
            .map_err(l10n)?;
        Ok(PlansView {
            accounts: cost::plan_accounts(&saved.plans, accounts),
            display: saved.display_currency.as_ref().map(|d| DisplayView {
                code: d.code.clone(),
                rate: d.rate,
                set_on: d.set_on,
                age_days: d.age_days(today),
                stale: d.stale(today),
            }),
            values,
        })
    }

    /// PRD 0030 RF1.1: save (`price` given) or clear the plan of the
    /// account `id`; refused unless a configured account has that id.
    /// Error: `cost.error.invalid_plan` as JSON.
    pub fn set_plan(
        &self,
        accounts: &[Account],
        id: &str,
        plan: Option<(f64, String, Option<u8>)>,
    ) -> Result<PlansView, String> {
        let stores = self.stores()?;
        if !accounts.iter().any(|account| account.state_key() == id) {
            return Err(l10n(CostError::InvalidPlan));
        }
        let plan = plan
            .map(|(price, currency, day)| cost::PlanSetting::new(price, &currency, day))
            .transpose()
            .map_err(l10n)?;
        stores.settings.set_plan(id, plan).map_err(l10n)?;
        self.plans(accounts)
    }

    /// PRD 0030 RF6.1: save the rate typed ("1 USD = `rate` `code`"), or
    /// with `None` clear it. Error: `cost.error.invalid_rate` as JSON.
    pub fn set_display(
        &self,
        accounts: &[Account],
        display: Option<(String, f64)>,
    ) -> Result<PlansView, String> {
        let stores = self.stores()?;
        let today = Self::scanner(stores, &self.env)
            .zone()
            .day(chrono::Utc::now());
        let display = display
            .map(|(code, rate)| cost::DisplayCurrency::new(&code, rate, today))
            .transpose()
            .map_err(l10n)?;
        stores
            .settings
            .set_display_currency(display)
            .map_err(l10n)?;
        self.plans(accounts)
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

#[cfg(test)]
mod tests {
    use super::*;
    use cuotai_core::Provider;

    fn local(dir: &std::path::Path) -> LocalCost {
        LocalCost {
            stores: Some(Stores {
                settings: SettingsStore::at(dir.join("settings.json")),
                index: CostIndex::new(dir.join("index.sqlite3")),
                pricing: PricingFile::at(dir.join("pricing.json")),
            }),
            env: Env::new(Some(dir.join("home")), []),
            worker: Mutex::new(None),
        }
    }

    fn account(name: &str, provider: Provider) -> Account {
        Account {
            id: None,
            name: name.into(),
            provider,
            ..Account::default()
        }
    }

    /// PRD 0030 RF1.1: a plan is saved under the account id, a legacy key
    /// moves to it, an unknown account is refused and clearing removes it.
    #[test]
    fn plans_follow_the_account_id() {
        let dir = tempfile::tempdir().unwrap();
        let cost = local(dir.path());
        let work = account("Work", Provider::Claude);
        let other = account("Api", Provider::AnthropicApi);
        let accounts = [work.clone(), other.clone()];

        let view = cost.plans(&accounts).unwrap();
        assert_eq!(
            view.accounts.len(),
            1,
            "only the tool's accounts are listed"
        );

        let plan = cost::PlanSetting::new(100.0, "USD", None).unwrap();
        let settings = &cost.stores().unwrap().settings;
        settings.set_plan(&work.key(), Some(plan)).unwrap();
        cost.plans(&accounts).unwrap();
        let saved = settings.load().unwrap();
        assert_eq!(saved.plans.keys().collect::<Vec<_>>(), [&work.state_key()]);

        let id = work.state_key();
        assert!(cost
            .set_plan(&accounts, "nobody", Some((10.0, "USD".into(), None)))
            .is_err());
        assert!(cost
            .set_plan(&accounts, &id, Some((0.0, "USD".into(), None)))
            .is_err());
        let view = cost
            .set_plan(&accounts, &id, Some((20.0, "brl".into(), Some(15))))
            .unwrap();
        let saved = view.accounts[0].plan.as_ref().unwrap();
        assert_eq!((saved.price, saved.currency.as_str()), (20.0, "BRL"));
        let view = cost.set_plan(&accounts, &id, None).unwrap();
        assert!(view.accounts[0].plan.is_none());
    }

    /// PRD 0030 RF6.1: the rate is validated and saved; USD is refused.
    #[test]
    fn the_display_currency_is_validated_and_saved() {
        let dir = tempfile::tempdir().unwrap();
        let cost = local(dir.path());
        assert!(cost.set_display(&[], Some(("USD".into(), 5.0))).is_err());
        assert!(cost.set_display(&[], Some(("BRL".into(), 0.0))).is_err());
        let view = cost.set_display(&[], Some(("brl".into(), 5.42))).unwrap();
        let display = view.display.unwrap();
        assert_eq!((display.code.as_str(), display.rate), ("BRL", 5.42));
        assert_eq!(display.age_days, 0);
        assert!(!display.stale);
        assert!(cost.set_display(&[], None).unwrap().display.is_none());
    }
}
