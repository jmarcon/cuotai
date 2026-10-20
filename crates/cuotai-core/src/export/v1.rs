//! `export::v1` (PRD 0012 RF1): the last known state of every account, built
//! from a [`RefreshCycle`] without querying anything, with the derived fields
//! the front ends already show (class, age, freshness, the preserved reading,
//! binding metric, pace and usage access).
//!
//! The contract is additive within `schema: 1` and documented in
//! `docs/export-v1.md` with `docs/export-v1.schema.json`; the reference
//! `docs/export-v1.example.json` is checked by a test. It never carries
//! credentials, credential variable names, authenticated URLs or (unless
//! [`Options::include_identity`]) e-mails: notes and `source` go through
//! [`REDACTED`] substitution. A missing value stays `null`, never zero.

use std::borrow::Cow;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::access::{self, UsageAccess};
use crate::config_file::ConfigIdentity;
use crate::freshness::{self, Freshness};
use crate::metrics::{self, AccountClass, Binding};
use crate::pace::{self, Pace};
use crate::refresh::RefreshCycle;
use crate::text::{Localized, ParamValue};
use crate::{account_edit, identity, Account, Config, Metric, Provider, Report};

/// Version of this contract; removing a field or changing its meaning needs 2.
pub const SCHEMA: u32 = 1;

/// What replaces a credential variable name, an e-mail or an unparsable URL
/// in notes and `source`.
pub const REDACTED: &str = "[redacted]";

/// Choices of whoever produces the export.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Options {
    /// Publish each account's e-mail (`email`). Off by default: the field is
    /// then absent and the e-mail is redacted from notes.
    pub include_identity: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Export {
    /// Always [`SCHEMA`].
    pub schema: u32,
    pub generated_at: DateTime<Utc>,
    /// [`ConfigIdentity::digest`]: tells configurations apart without
    /// publishing the file path.
    pub config_identity: String,
    /// In configuration order.
    pub accounts: Vec<AccountExport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AccountExport {
    /// Position in the configuration.
    pub index: usize,
    /// [`Account::key`]: `<provider slug>:<name>`, stable across reordering.
    pub key: String,
    pub name: String,
    pub provider: Provider,
    /// Present only with [`Options::include_identity`]; `null` when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<Option<String>>,
    /// [`metrics::classify_report`] of `report`.
    pub class: AccountClass,
    /// The current report, `null` while the account has none yet (first
    /// query or an edit pending). Notes and `source` are redacted.
    pub report: Option<Report>,
    /// Age of `report.checked_at`; `null` without a report.
    pub age_seconds: Option<u64>,
    /// [`freshness::assess`] of `report.checked_at` against the account's
    /// effective interval; `null` without a report.
    pub freshness: Option<Freshness>,
    /// `report.checked_at` lies in the future (the clock went backwards);
    /// the age then reads zero.
    pub clock_adjusted: bool,
    /// `true` exactly when `last_good` is present: what is known of the
    /// account comes from an earlier reading, not from `report`.
    pub stale: bool,
    /// The last `Ready` reading, offered only while `report` is missing or
    /// failed ([`RefreshCycle::last_good_report`]). Never current data.
    pub last_good: Option<LastGood>,
    /// [`metrics::binding`] of `report`.
    pub binding: Option<Binding>,
    /// [`pace::assess`] per metric of `report`, same length and order;
    /// `null` for a metric without a pace. Empty without a report.
    pub pace: Vec<Option<Pace>>,
    /// [`access::describe`] of the account.
    pub access: UsageAccess,
}

/// The preserved reading of [`AccountExport::last_good`].
#[derive(Debug, Clone, Serialize)]
pub struct LastGood {
    /// Original measurement time, never refreshed.
    pub checked_at: DateTime<Utc>,
    pub age_seconds: u64,
    pub metrics: Vec<Metric>,
}

impl Export {
    /// The export of `config` with the reports `cycle` holds at `now`. Pure:
    /// reads the cycle's state and never starts a query.
    pub fn from_cycle(
        config: &Config,
        identity: &ConfigIdentity,
        cycle: &RefreshCycle,
        now: DateTime<Utc>,
        options: Options,
    ) -> Self {
        let accounts = config
            .accounts
            .iter()
            .enumerate()
            .map(|(index, account)| {
                let interval =
                    Duration::from_secs(account.effective_refresh_seconds(config.refresh_seconds));
                account_export(
                    index,
                    account,
                    cycle.account_report(index),
                    cycle.last_good_report(index),
                    interval,
                    now,
                    options,
                )
            })
            .collect();
        Self {
            schema: SCHEMA,
            generated_at: now,
            config_identity: identity.digest(),
            accounts,
        }
    }
}

fn account_export(
    index: usize,
    account: &Account,
    report: Option<&Report>,
    last_good: Option<&Report>,
    interval: Duration,
    now: DateTime<Utc>,
    options: Options,
) -> AccountExport {
    let scrub = Scrub::new(account, options.include_identity);
    let age = |at| freshness::age(at, now).as_secs();
    AccountExport {
        index,
        key: account.key(),
        name: account.name.clone(),
        provider: account.provider,
        email: options
            .include_identity
            .then(|| identity::email(account).map(str::to_owned)),
        class: metrics::classify_report(report),
        report: report.map(|report| scrub.report(report)),
        age_seconds: report.map(|report| age(report.checked_at)),
        freshness: report.map(|report| freshness::assess(report.checked_at, interval, now)),
        clock_adjusted: report
            .is_some_and(|report| freshness::clock_adjusted(report.checked_at, now)),
        stale: last_good.is_some(),
        last_good: last_good.map(|reading| LastGood {
            checked_at: reading.checked_at,
            age_seconds: age(reading.checked_at),
            metrics: reading.metrics.clone(),
        }),
        binding: report.and_then(metrics::binding),
        pace: report.map_or_else(Vec::new, |report| {
            report
                .metrics
                .iter()
                .map(|metric| pace::assess(metric, now, None))
                .collect()
        }),
        access: access::describe(account),
    }
}

/// Removes from free text what RF1.3 keeps out of an export: the account's
/// credential variable names (and any `UPPER_SNAKE` name, the shape of an
/// environment variable), e-mails (the account's and any address a provider
/// put in a note) unless identity is included, and the credentials, query
/// and fragment of URLs.
struct Scrub<'a> {
    names: Vec<&'a str>,
    email: Option<&'a str>,
    hide_emails: bool,
}

impl<'a> Scrub<'a> {
    fn new(account: &'a Account, include_identity: bool) -> Self {
        let params = account
            .params
            .iter()
            .filter(|(key, _)| key.ends_with("_env"))
            .map(|(_, name)| name.as_str());
        let names = account
            .credential_env
            .as_deref()
            .into_iter()
            .chain(params)
            .chain([account_edit::default_env(account.provider)])
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .collect();
        Self {
            names,
            email: identity::email(account).filter(|_| !include_identity),
            hide_emails: !include_identity,
        }
    }

    fn report(&self, report: &Report) -> Report {
        let mut report = report.clone();
        for note in &mut report.notes {
            *note = self.text(note);
        }
        for note in &mut report.notes_l10n {
            self.localized(note);
        }
        report.source = self.text(&report.source);
        report
    }

    fn localized(&self, note: &mut Localized) {
        note.fallback = Cow::Owned(self.text(&note.fallback));
        for value in note.params.values_mut() {
            match value {
                ParamValue::Text(text) => *text = self.text(text),
                ParamValue::L10n(inner) => self.localized(inner),
            }
        }
    }

    fn text(&self, text: &str) -> String {
        let text = match self.email {
            Some(email) if text.contains(email) => Cow::Owned(text.replace(email, REDACTED)),
            _ => Cow::Borrowed(text),
        };
        let mut out = String::with_capacity(text.len());
        let mut rest = &*text;
        while let Some(start) = url_start(rest) {
            self.plain(&rest[..start], &mut out);
            let url = &rest[start..];
            let end = url
                .find(|c: char| c.is_whitespace() || "\"'<>;()[]{}".contains(c))
                .unwrap_or(url.len());
            let url = url[..end].trim_end_matches(['.', ',', ':']);
            out.push_str(&clean_url(url));
            rest = &rest[start + url.len()..];
        }
        self.plain(rest, &mut out);
        out
    }

    /// [`Self::words`], with every e-mail address replaced unless identity
    /// is included.
    fn plain(&self, text: &str, out: &mut String) {
        let mut rest = text;
        while let Some((start, end)) = self.hide_emails.then(|| email_span(rest)).flatten() {
            self.words(&rest[..start], out);
            out.push_str(REDACTED);
            rest = &rest[end..];
        }
        self.words(rest, out);
    }

    /// Copy `text`, replacing each identifier-like word that names a
    /// credential variable.
    fn words(&self, text: &str, out: &mut String) {
        let mut word_start = None;
        for (at, c) in text.char_indices() {
            match (c.is_ascii_alphanumeric() || c == '_', word_start) {
                (true, None) => word_start = Some(at),
                (true, Some(_)) => {}
                (false, start) => {
                    if let Some(start) = start {
                        self.word(&text[start..at], out);
                        word_start = None;
                    }
                    out.push(c);
                }
            }
        }
        if let Some(start) = word_start {
            self.word(&text[start..], out);
        }
    }

    fn word(&self, word: &str, out: &mut String) {
        if self.names.contains(&word) || env_like(word) {
            out.push_str(REDACTED);
        } else {
            out.push_str(word);
        }
    }
}

/// `UPPER_SNAKE` with at least one `_` between characters, e.g. `GROQ_API_KEY`.
fn env_like(word: &str) -> bool {
    let bytes = word.as_bytes();
    bytes.first().is_some_and(u8::is_ascii_uppercase)
        && bytes.last().is_some_and(|b| *b != b'_')
        && bytes.contains(&b'_')
        && bytes
            .iter()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || *b == b'_')
}

/// Byte range of the first e-mail address (`local@domain.tld`) in `text`.
fn email_span(text: &str) -> Option<(usize, usize)> {
    let local = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    let host = |c: char| c.is_ascii_alphanumeric() || ".-".contains(c);
    text.match_indices('@').find_map(|(at, _)| {
        let start = text[..at]
            .char_indices()
            .rev()
            .find(|&(_, c)| !local(c))
            .map_or(0, |(i, c)| i + c.len_utf8());
        let after = &text[at + 1..];
        let domain = after[..after.find(|c: char| !host(c)).unwrap_or(after.len())]
            .trim_end_matches(['.', '-']);
        let labels = domain.split('.').count();
        (start < at && labels >= 2 && domain.split('.').all(|label| !label.is_empty()))
            .then_some((start, at + 1 + domain.len()))
    })
}

fn url_start(text: &str) -> Option<usize> {
    text.match_indices("http")
        .map(|(at, _)| at)
        .find(|&at| text[at..].starts_with("https://") || text[at..].starts_with("http://"))
}

/// `url` without user, password, query or fragment; [`REDACTED`] when it
/// does not parse. Kept verbatim when there is nothing to remove.
fn clean_url(url: &str) -> Cow<'_, str> {
    let Ok(mut parsed) = url::Url::parse(url) else {
        return Cow::Borrowed(REDACTED);
    };
    if parsed.username().is_empty()
        && parsed.password().is_none()
        && parsed.query().is_none()
        && parsed.fragment().is_none()
    {
        return Cow::Borrowed(url);
    }
    // A URL with a host always accepts an empty user and password.
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);
    parsed.set_query(None);
    parsed.set_fragment(None);
    Cow::Owned(parsed.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::params;
    use crate::{MetricKind, MetricWindow, Status};
    use chrono::TimeZone;
    use serde_json::Value;
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    const REFERENCE: &str = "docs/export-v1.example.json";
    const SCHEMA_DOC: &str = "docs/export-v1.schema.json";
    const REGENERATE: &str = "CUOTAI_REGENERATE_DOCS";

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 20, 12, 0, 0).unwrap()
    }

    fn minutes_ago(minutes: i64) -> DateTime<Utc> {
        now() - chrono::TimeDelta::minutes(minutes)
    }

    fn account(name: &str, provider: Provider) -> Account {
        Account {
            name: name.into(),
            provider,
            ..Account::default()
        }
    }

    fn quota(label: &str, used: f64, window: MetricWindow, resets_in_hours: i64) -> Metric {
        Metric {
            limit: Some(100.0),
            resets_at: Some(now() + chrono::TimeDelta::hours(resets_in_hours)),
            ..Metric::usage(label, used, "%", MetricKind::Quota, Some(window))
        }
    }

    fn report(account: &Account, status: Status, checked_at: DateTime<Utc>) -> Report {
        Report {
            account: account.name.clone(),
            provider: account.provider,
            checked_at,
            status,
            metrics: Vec::new(),
            notes: Vec::new(),
            notes_l10n: Vec::new(),
            source: String::new(),
        }
    }

    fn note(report: &mut Report, note: Localized) {
        report.notes.push(note.fallback.to_string());
        report.notes_l10n.push(note);
    }

    fn finish(cycle: &mut RefreshCycle, index: usize, report: Report) {
        let token = cycle.begin(index).unwrap();
        cycle.finish_account(index, Some(report), token, Instant::now());
    }

    fn build(config: &Config, cycle: &RefreshCycle, options: Options) -> Export {
        let identity = ConfigIdentity::Explicit("reference".into());
        Export::from_cycle(config, &identity, cycle, now(), options)
    }

    fn export(config: &Config, cycle: &RefreshCycle, options: Options) -> Value {
        serde_json::to_value(build(config, cycle, options)).unwrap()
    }

    /// One account per case the contract distinguishes: a healthy one with a
    /// binding metric, a pace and a missing value; a failure with the
    /// preserved reading; a failure without one whose note names the
    /// credential variable; and one still waiting for its first report.
    fn reference() -> (Config, RefreshCycle) {
        let mut personal = account("Personal", Provider::Claude);
        personal
            .params
            .insert("email".into(), "person@example.com".into());
        let mut work = account("Work", Provider::OpenAiApi);
        work.credential_env = Some("OPENAI_ADMIN_KEY".into());
        let groq = account("Groq", Provider::Groq);
        let pending = account("Kimi", Provider::Kimi);
        let config = Config {
            accounts: vec![personal, work, groq, pending],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let accounts = &config.accounts;

        let mut ready = report(&accounts[0], Status::Ready, minutes_ago(2));
        ready.metrics = vec![
            quota("Sessão (5h)", 30.0, MetricWindow::Window5h, 2),
            quota("Semana", 80.0, MetricWindow::Weekly, 48),
            Metric {
                used: None,
                ..Metric::usage("Créditos extras", 0.0, "USD", MetricKind::Credits, None)
            },
        ];
        ready.source = "https://api.anthropic.com/api/oauth/usage".into();
        finish(&mut cycle, 0, ready);

        let mut good = report(&accounts[1], Status::Ready, minutes_ago(42));
        good.metrics = vec![Metric {
            period: Some("2026-10".into()),
            ..Metric::usage(
                "Gasto do mês",
                12.5,
                "USD",
                MetricKind::Spend,
                Some(MetricWindow::Monthly),
            )
        }];
        finish(&mut cycle, 1, good);
        let mut failed = report(&accounts[1], Status::Error, minutes_ago(1));
        note(
            &mut failed,
            Localized::with_params(
                "query.http",
                params([("code", ParamValue::from(503u16))]),
                "Consulta retornou HTTP 503. Verifique acesso e permissões.",
            ),
        );
        finish(&mut cycle, 1, failed);

        let mut missing = report(&accounts[2], Status::NeedsConfiguration, minutes_ago(1));
        note(
            &mut missing,
            Localized::with_param(
                "query.missing_credential",
                "name",
                "GROQ_API_KEY",
                "Credencial ausente: GROQ_API_KEY",
            ),
        );
        finish(&mut cycle, 2, missing);
        (config, cycle)
    }

    fn doc_path(relative: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative)
    }

    /// RF1.2: the output matches the reference file; any change to the
    /// contract shows up as a diff to review (regenerate it with
    /// `CUOTAI_REGENERATE_DOCS=1` only for an additive change).
    #[test]
    fn export_v1_reference_matches_the_document() {
        let (config, cycle) = reference();
        // The struct itself, so the document keeps the declared field order.
        let rendered = format!(
            "{}\n",
            serde_json::to_string_pretty(&build(&config, &cycle, Options::default())).unwrap()
        );
        let path = doc_path(REFERENCE);
        if std::env::var_os(REGENERATE).is_some() {
            std::fs::write(&path, &rendered).unwrap();
            return;
        }
        // Windows checkouts may convert the document to CRLF.
        let current = std::fs::read_to_string(&path)
            .expect("docs/export-v1.example.json")
            .replace("\r\n", "\n");
        assert!(
            current == rendered,
            "{REFERENCE} differs from export::v1; review the change (removing or redefining a \
             field needs schema 2) and regenerate with \
             `{REGENERATE}=1 cargo test -p cuotai-core export_v1_reference`"
        );
    }

    #[test]
    fn reference_values_follow_the_cycle_without_inventing_data() {
        let (config, cycle) = reference();
        let json = export(&config, &cycle, Options::default());
        assert_eq!(json["schema"], 1);
        assert_eq!(
            json["config_identity"],
            ConfigIdentity::Explicit("reference".into()).digest()
        );
        let [personal, work, groq, pending] = json["accounts"].as_array().unwrap().as_slice()
        else {
            panic!("four accounts in configuration order");
        };
        assert_eq!(personal["key"], "claude:Personal");
        assert_eq!(personal["class"], "available");
        assert_eq!(personal["age_seconds"], 120);
        assert_eq!(personal["freshness"], "fresh");
        assert_eq!(personal["stale"], false);
        assert_eq!(personal["binding"]["label"], "Semana");
        assert_eq!(personal["pace"].as_array().unwrap().len(), 3);
        assert_eq!(personal["pace"][0]["stage"], "reserve");
        assert!(personal["pace"][2].is_null());
        assert_eq!(personal["access"]["level"], "undocumented");

        // The failure keeps its own report and age; the old values travel
        // apart, with their original time, and never feed binding or pace.
        assert_eq!(work["report"]["status"], "error");
        assert_eq!(work["age_seconds"], 60);
        assert_eq!(work["stale"], true);
        assert_eq!(work["last_good"]["age_seconds"], 42 * 60);
        assert_eq!(work["last_good"]["metrics"][0]["used"], 12.5);
        assert!(work["binding"].is_null());
        assert_eq!(work["class"], "unmeasured");

        assert_eq!(groq["stale"], false);
        assert!(groq["last_good"].is_null());

        assert_eq!(pending["key"], "kimi:Kimi");
        assert_eq!(pending["class"], "unmeasured");
        for field in ["report", "age_seconds", "freshness", "last_good", "binding"] {
            assert!(pending[field].is_null(), "{field}");
        }
        assert_eq!(pending["pace"], serde_json::json!([]));
    }

    /// RF1.4: unknown values are `null`, never `0`, including non-finite
    /// numbers an adapter might leave behind.
    #[test]
    fn missing_values_stay_null() {
        let (config, cycle) = reference();
        let json = export(&config, &cycle, Options::default());
        let credits = &json["accounts"][0]["report"]["metrics"][2];
        for field in [
            "used",
            "limit",
            "remaining",
            "resets_at",
            "window",
            "period",
        ] {
            assert!(credits[field].is_null(), "{field}: {}", credits[field]);
        }

        let config = Config {
            accounts: vec![account("Kimi", Provider::Kimi)],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let mut ready = report(&config.accounts[0], Status::Ready, minutes_ago(1));
        ready.metrics = vec![Metric {
            limit: Some(f64::NAN),
            remaining: Some(f64::INFINITY),
            ..Metric::usage("Saldo", f64::NAN, "CNY", MetricKind::Balance, None)
        }];
        finish(&mut cycle, 0, ready);
        let json = export(&config, &cycle, Options::default());
        let metric = &json["accounts"][0]["report"]["metrics"][0];
        for field in ["used", "limit", "remaining"] {
            assert!(metric[field].is_null(), "{field}: {}", metric[field]);
        }
    }

    /// RF1.3: a fixture account whose notes and source carry every kind of
    /// secret a provider could leak; none of them reaches the export.
    #[test]
    fn export_carries_no_secrets() {
        const TOKEN: &str = "sk-live-0123456789abcdef";
        let mut account = account("Leaky", Provider::GeminiAi);
        account.credential_env = Some("my_private_key".into());
        account
            .params
            .insert("session_env".into(), "GEMINI_SESSION".into());
        account
            .params
            .insert("email".into(), "leaky@example.com".into());
        let config = Config {
            accounts: vec![account.clone()],
            ..Config::default()
        };
        let mut cycle = RefreshCycle::new(&config, Instant::now());
        let mut failed = report(&account, Status::Error, minutes_ago(1));
        failed.source = format!(
            "https://user:{TOKEN}@api.example.com/v1/usage?key={TOKEN}#frag ; https://docs.example.com/usage"
        );
        note(
            &mut failed,
            Localized::with_params(
                "query.router_existing.missing_gemini_session",
                params([
                    ("env", ParamValue::from("GEMINI_SESSION")),
                    (
                        "nested",
                        ParamValue::l10n(Localized::with_param(
                            "query.missing_credential",
                            "name",
                            "my_private_key",
                            "Credencial ausente: my_private_key",
                        )),
                    ),
                ]),
                format!(
                    "Token ausente em GEMINI_SESSION (my_private_key); GEMINI_API_KEY não \
                     autentica leaky@example.com. Veja https://api.example.com/x?access_token={TOKEN}."
                ),
            ),
        );
        // An address the provider's response put in a note (Cursor's
        // "Conta: …", Perplexity's member filter), not the account's own.
        note(
            &mut failed,
            Localized::with_param(
                "note.cursor_personal.account_email",
                "email",
                "owner.name+cuotai@corp.example.com",
                "Conta: owner.name+cuotai@corp.example.com",
            ),
        );
        finish(&mut cycle, 0, failed);

        let json = export(&config, &cycle, Options::default());
        let text = json.to_string();
        for secret in [
            TOKEN,
            "user:",
            "key=",
            "access_token",
            "#frag",
            "my_private_key",
            "GEMINI_SESSION",
            "GEMINI_API_KEY",
            "leaky@example.com",
            "corp.example.com",
            "credential_env",
            "session_env",
            "Bearer",
        ] {
            assert!(!text.contains(secret), "{secret} leaked: {text}");
        }
        assert!(json["accounts"][0].get("email").is_none());
        let report = &json["accounts"][0]["report"];
        assert_eq!(
            report["source"],
            "https://api.example.com/v1/usage ; https://docs.example.com/usage"
        );
        assert_eq!(report["notes_l10n"][0]["params"]["env"], REDACTED);
        assert_eq!(report["notes"][0], report["notes_l10n"][0]["fallback"]);
        // Ordinary text and public URLs survive the redaction.
        assert!(report["notes"][0]
            .as_str()
            .unwrap()
            .contains("Veja https://api.example.com/x."));
    }

    #[test]
    fn identity_appears_only_when_included() {
        let (config, cycle) = reference();
        let options = Options {
            include_identity: true,
        };
        let json = export(&config, &cycle, options);
        assert_eq!(json["accounts"][0]["email"], "person@example.com");
        // Included but unknown is `null`, not absent.
        assert!(json["accounts"][1]["email"].is_null());
        assert!(json["accounts"][1]
            .as_object()
            .unwrap()
            .contains_key("email"));

        let without = export(&config, &cycle, Options::default()).to_string();
        assert!(!without.contains("person@example.com"));
        assert!(!without.contains("\"email\""));
    }

    #[test]
    fn redaction_keeps_ordinary_words_and_names_the_account_uses() {
        let mut account = account("A", Provider::Kimi);
        account.credential_env = Some("work".into());
        let scrub = Scrub::new(&account, false);
        assert_eq!(
            scrub.text("HTTP 429 at work, network down; KIMI_API_KEY or _X_ missing"),
            "HTTP 429 at [redacted], network down; [redacted] or _X_ missing"
        );
        assert_eq!(scrub.text("teamwork"), "teamwork");
        assert_eq!(
            scrub.text("http://:80/x"),
            "[redacted]",
            "an unparsable URL is dropped"
        );
        assert_eq!(
            scrub.text("mailto:a.b+c@x.io, at @home or a@b and me@host.dev."),
            "mailto:[redacted], at @home or a@b and [redacted].",
            "every e-mail address goes, other @ words stay"
        );
        let identity = Scrub::new(&account, true);
        assert_eq!(identity.text("Conta: a@x.io"), "Conta: a@x.io");
    }

    /// RF1.2: the documented JSON Schema declares every field the export
    /// emits (an undeclared field fails here) and accepts the reference and
    /// the identity variant.
    #[test]
    fn json_schema_describes_the_export() {
        let schema: Value =
            serde_json::from_str(&std::fs::read_to_string(doc_path(SCHEMA_DOC)).unwrap())
                .expect("docs/export-v1.schema.json is JSON");
        let (config, cycle) = reference();
        for options in [
            Options::default(),
            Options {
                include_identity: true,
            },
        ] {
            let json = export(&config, &cycle, options);
            let mut errors = Vec::new();
            validate(&schema, &schema, &json, "$", &mut errors);
            assert!(errors.is_empty(), "{errors:#?}");
        }

        // The validator itself rejects what the schema forbids.
        let mut broken = export(&config, &cycle, Options::default());
        broken["accounts"][0]["undeclared"] = Value::Bool(true);
        broken["accounts"][1]["stale"] = Value::from(1);
        broken["accounts"][2]
            .as_object_mut()
            .unwrap()
            .remove("access");
        let mut errors = Vec::new();
        validate(&schema, &schema, &broken, "$", &mut errors);
        assert_eq!(errors.len(), 3, "{errors:#?}");
    }

    /// The subset of JSON Schema 2020-12 the document uses, strict about
    /// undeclared properties so the schema cannot fall behind the code.
    fn validate(root: &Value, schema: &Value, value: &Value, at: &str, errors: &mut Vec<String>) {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let name = reference
                .strip_prefix("#/$defs/")
                .unwrap_or_else(|| panic!("unsupported $ref {reference}"));
            let target = &root["$defs"][name];
            assert!(!target.is_null(), "missing $defs/{name}");
            return validate(root, target, value, at, errors);
        }
        const KEYWORDS: [&str; 16] = [
            "$schema",
            "$id",
            "$ref",
            "$defs",
            "title",
            "description",
            "type",
            "properties",
            "additionalProperties",
            "required",
            "items",
            "enum",
            "const",
            "anyOf",
            "format",
            "minimum",
        ];
        for key in schema.as_object().into_iter().flatten().map(|(k, _)| k) {
            assert!(
                KEYWORDS.contains(&key.as_str()),
                "unsupported keyword {key} at {at}"
            );
        }
        if let Some(options) = schema.get("anyOf").and_then(Value::as_array) {
            let matches = options.iter().any(|option| {
                let mut inner = Vec::new();
                validate(root, option, value, at, &mut inner);
                inner.is_empty()
            });
            if !matches {
                errors.push(format!("{at}: matches no anyOf branch: {value}"));
            }
        }
        if let Some(types) = schema.get("type") {
            let types: Vec<&str> = match types {
                Value::String(one) => vec![one.as_str()],
                Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
                _ => panic!("bad type at {at}"),
            };
            let actual = match value {
                Value::Null => "null",
                Value::Bool(_) => "boolean",
                Value::Number(n) if n.is_u64() || n.is_i64() => "integer",
                Value::Number(_) => "number",
                Value::String(_) => "string",
                Value::Array(_) => "array",
                Value::Object(_) => "object",
            };
            let ok = types.contains(&actual) || (actual == "integer" && types.contains(&"number"));
            if !ok {
                errors.push(format!("{at}: {actual} is not {types:?}"));
                return;
            }
        }
        if let Some(allowed) = schema.get("enum").and_then(Value::as_array) {
            if !allowed.contains(value) {
                errors.push(format!("{at}: {value} not in enum"));
            }
        }
        if let Some(expected) = schema.get("const") {
            if expected != value {
                errors.push(format!("{at}: {value} is not {expected}"));
            }
        }
        if let (Some(minimum), Some(number)) = (
            schema.get("minimum").and_then(Value::as_f64),
            value.as_f64(),
        ) {
            if number < minimum {
                errors.push(format!("{at}: {number} < {minimum}"));
            }
        }
        if let (Some(items), Some(array)) = (schema.get("items"), value.as_array()) {
            for (i, item) in array.iter().enumerate() {
                validate(root, items, item, &format!("{at}[{i}]"), errors);
            }
        }
        if let Some(object) = value.as_object() {
            for name in schema
                .get("required")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
            {
                if !object.contains_key(name) {
                    errors.push(format!("{at}: missing required {name}"));
                }
            }
            // An object schema declares each property by name or through
            // `additionalProperties` (maps); anything else is undeclared.
            if schema.get("type").is_some() {
                let properties = schema.get("properties").and_then(Value::as_object);
                let extra = schema.get("additionalProperties");
                for (name, item) in object {
                    match properties.and_then(|p| p.get(name)).or(extra) {
                        Some(property) => {
                            validate(root, property, item, &format!("{at}.{name}"), errors)
                        }
                        None => errors.push(format!("{at}: undeclared property {name}")),
                    }
                }
            }
        }
    }
}
