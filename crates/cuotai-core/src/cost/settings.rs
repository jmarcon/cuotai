//! RF1.1/RF1.2/RF2.3: the cost preferences of this computer, kept in
//! `<OS local data dir>/cuotai/cost/settings.json` (`0600`): per source the
//! consent (the digest of the [`Disclosure`] the user accepted) and the
//! directories chosen instead of the defaults, plus the period shown and
//! the index retention. Everything is off until a source is consented.

use super::source::expand_home;
use super::{CostError, Period, Retention, SourceId};
use crate::hooks::write_atomically;
use crate::models::{create_private_file, digest};
use crate::text::{params, Localized};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Raised whenever the disclosure's wording or meaning changes, so an
/// earlier consent no longer covers it.
const DISCLOSURE_VERSION: u32 = 1;

/// The environment variables the sources' default directories follow.
const VARIABLES: [&str; 2] = ["CLAUDE_CONFIG_DIR", "CODEX_HOME"];

/// The home directory and environment the default directories come from;
/// injectable so tests never read the real ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    home: Option<PathBuf>,
    vars: BTreeMap<String, String>,
}

impl Env {
    /// This process's home directory and the sources' variables.
    pub fn system() -> Self {
        Self::new(
            dirs::home_dir(),
            VARIABLES.into_iter().filter_map(|name| {
                std::env::var(name)
                    .ok()
                    .map(|value| (name.to_owned(), value))
            }),
        )
    }

    pub fn new(home: Option<PathBuf>, vars: impl IntoIterator<Item = (String, String)>) -> Self {
        Self {
            home,
            vars: vars.into_iter().collect(),
        }
    }

    pub fn home(&self) -> Option<&Path> {
        self.home.as_deref()
    }

    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }
}

/// One source's preferences.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSettings {
    /// [`Disclosure::digest`] of what the user accepted; `None` is off.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consented_at: Option<DateTime<Utc>>,
    /// Directories read instead of the defaults (RF1.2); empty means the
    /// defaults. `~/` is expanded.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<PathBuf>,
}

/// The cost preferences of this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostSettings {
    #[serde(default)]
    pub sources: BTreeMap<SourceId, SourceSettings>,
    #[serde(default)]
    pub period: Period,
    #[serde(default = "default_retention")]
    pub retention_days: u32,
}

fn default_retention() -> u32 {
    Retention::DEFAULT_DAYS
}

impl Default for CostSettings {
    fn default() -> Self {
        Self {
            sources: BTreeMap::new(),
            period: Period::default(),
            retention_days: Retention::DEFAULT_DAYS,
        }
    }
}

impl CostSettings {
    /// The retention; a value out of bounds written by hand counts as the
    /// default.
    pub fn retention(&self) -> Retention {
        Retention::days(self.retention_days).unwrap_or_default()
    }

    fn source(&self, source: SourceId) -> Option<&SourceSettings> {
        self.sources.get(&source)
    }

    /// The directories of `source` read when it is on: the chosen ones or
    /// the defaults.
    pub fn roots(&self, source: SourceId, env: &Env) -> Vec<PathBuf> {
        let chosen = self.source(source).map_or(&[][..], |s| s.paths.as_slice());
        roots(source, chosen, env)
    }

    /// What turning `source` on reads now.
    pub fn disclosure(&self, source: SourceId, env: &Env) -> Disclosure {
        let chosen = self.source(source).map_or(&[][..], |s| s.paths.as_slice());
        Disclosure::new(source, chosen, env)
    }

    /// RF1.1: on only while the consent covers exactly the directories it
    /// would read now (a changed list, or a moved home, turns it off).
    pub fn enabled(&self, source: SourceId, env: &Env) -> bool {
        self.source(source)
            .and_then(|s| s.consent.as_deref())
            .is_some_and(|consent| consent == self.disclosure(source, env).digest)
    }

    /// The sources that are on.
    pub fn enabled_sources(&self, env: &Env) -> Vec<SourceId> {
        SourceId::ALL
            .into_iter()
            .filter(|source| self.enabled(*source, env))
            .collect()
    }
}

/// `chosen` (expanded) or the source's defaults.
fn roots(source: SourceId, chosen: &[PathBuf], env: &Env) -> Vec<PathBuf> {
    if chosen.is_empty() {
        source
            .adapter()
            .default_roots(env.home(), &|name| env.var(name))
    } else {
        chosen
            .iter()
            .filter_map(|path| expand_home(&path.to_string_lossy(), env.home()))
            .collect()
    }
}

/// RF1.1: what turning one source on means, shown before the consent:
/// exactly which directories are read, what for, and that only numbers
/// are extracted.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Disclosure {
    pub source: SourceId,
    pub label: &'static str,
    /// Every directory read, in order; nothing outside them.
    pub directories: Vec<PathBuf>,
    /// The directories were chosen by the user, not the defaults.
    pub custom: bool,
    /// The files read inside them.
    pub pattern: &'static str,
    pub purpose: Localized,
    pub extracted: Localized,
    pub never: Localized,
    pub off: Localized,
    /// Identifies this exact disclosure; the consent records it.
    pub digest: String,
}

impl Disclosure {
    /// The disclosure of `source` reading `chosen` (empty: the defaults).
    pub fn new(source: SourceId, chosen: &[PathBuf], env: &Env) -> Self {
        let directories = roots(source, chosen, env);
        let pattern = source.adapter().pattern();
        let identity = serde_json::to_vec(&(DISCLOSURE_VERSION, source, &directories, pattern))
            .expect("disclosure identity serializes");
        let label = source.label();
        Self {
            source,
            label,
            custom: !chosen.is_empty(),
            pattern,
            purpose: Localized::with_param(
                "cost.disclosure.purpose",
                "source",
                label,
                format!(
                    "Para estimar, só neste computador, os tokens e o custo a preços de API do \
                     uso do {label} por dia e por modelo."
                ),
            ),
            extracted: Localized::fixed(
                "cost.disclosure.extracted",
                "Só números são extraídos: o horário, o modelo, as contagens de tokens (entrada, \
                 saída, leitura e gravação de cache) e um resumo anônimo da sessão.",
            ),
            never: Localized::fixed(
                "cost.disclosure.never",
                "O conteúdo das conversas, prompts, respostas, nomes de arquivos e caminhos de \
                 projetos nunca é lido para o índice, gravado nem exibido. Nada sai do computador.",
            ),
            off: Localized::fixed(
                "cost.disclosure.off",
                "Desligar a fonte interrompe qualquer leitura e apaga tudo o que foi derivado dela.",
            ),
            digest: digest(&identity),
            directories,
        }
    }

    /// The disclosure as lines for a terminal: a header, the purpose, each
    /// directory with the pattern, what is extracted, what never is, and
    /// what turning it off does.
    pub fn lines(&self) -> Vec<Localized> {
        let mut lines = vec![
            Localized::with_param(
                "cost.disclosure.header",
                "source",
                self.label,
                format!("Ligar a leitura dos logs locais do {}.", self.label),
            ),
            self.purpose.clone(),
            Localized::with_param(
                "cost.disclosure.directories",
                "pattern",
                self.pattern,
                format!("Lê apenas os arquivos {} destes diretórios:", self.pattern),
            ),
        ];
        lines.extend(self.directories.iter().map(|dir| {
            let dir = dir.display().to_string();
            Localized::with_params(
                "cost.disclosure.directory",
                params([("path", dir.as_str().into())]),
                dir.clone(),
            )
        }));
        lines.extend([self.extracted.clone(), self.never.clone(), self.off.clone()]);
        lines
    }
}

/// Where the preferences live; injectable so tests never touch OS dirs.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
}

impl SettingsStore {
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// `<OS local data dir>/cuotai/cost/settings.json`.
    pub fn in_data_dir() -> Result<Self, CostError> {
        dirs::data_local_dir()
            .map(|dir| Self::at(dir.join("cuotai").join("cost").join("settings.json")))
            .ok_or(CostError::Settings)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The saved preferences; everything off when none were saved. A file
    /// that cannot be read or parsed is an error, never "on".
    pub fn load(&self) -> Result<CostSettings, CostError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|_| CostError::Settings),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(CostSettings::default()),
            Err(_) => Err(CostError::Settings),
        }
    }

    fn update(
        &self,
        change: impl FnOnce(&mut CostSettings) -> Result<(), CostError>,
    ) -> Result<CostSettings, CostError> {
        let mut settings = self.load()?;
        change(&mut settings)?;
        let bytes = serde_json::to_vec_pretty(&settings).map_err(|_| CostError::Settings)?;
        create_private_file(&self.path).map_err(|_| CostError::Settings)?;
        write_atomically(&self.path, &bytes).map_err(|_| CostError::Settings)?;
        Ok(settings)
    }

    /// RF1.1: the user accepted the disclosure `digest` of `source` reading
    /// `chosen` (empty: the defaults). Refused with
    /// [`CostError::ConsentChanged`] unless it is exactly what would be
    /// read now.
    pub fn enable(
        &self,
        source: SourceId,
        chosen: Vec<PathBuf>,
        digest: &str,
        env: &Env,
        now: DateTime<Utc>,
    ) -> Result<CostSettings, CostError> {
        if Disclosure::new(source, &chosen, env).digest != digest {
            return Err(CostError::ConsentChanged);
        }
        self.update(|settings| {
            settings.sources.insert(
                source,
                SourceSettings {
                    consent: Some(digest.to_owned()),
                    consented_at: Some(now),
                    paths: chosen,
                },
            );
            Ok(())
        })
    }

    /// Turn `source` off, keeping its chosen directories. Use
    /// [`super::turn_off`] to also erase what was derived from it.
    pub fn disable(&self, source: SourceId) -> Result<CostSettings, CostError> {
        self.update(|settings| {
            if let Some(entry) = settings.sources.get_mut(&source) {
                entry.consent = None;
                entry.consented_at = None;
            }
            Ok(())
        })
    }

    /// RF2.3: the period front ends show.
    pub fn set_period(&self, period: Period) -> Result<CostSettings, CostError> {
        self.update(|settings| {
            settings.period = period;
            Ok(())
        })
    }

    /// RF2.3: how many days the index keeps.
    pub fn set_retention(&self, retention: Retention) -> Result<CostSettings, CostError> {
        self.update(|settings| {
            settings.retention_days = retention.keep_days();
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(home: &str) -> Env {
        Env::new(Some(PathBuf::from(home)), [])
    }

    #[test]
    fn nothing_is_on_until_a_disclosure_is_accepted() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(dir.path().join("cost/settings.json"));
        let env = env("/home/ana");
        let settings = store.load().unwrap();
        assert!(settings.enabled_sources(&env).is_empty());
        assert_eq!(settings.retention(), Retention::default());
        assert_eq!(settings.period, Period::Last7Days);

        let disclosure = settings.disclosure(SourceId::CodexCli, &env);
        assert_eq!(
            disclosure.directories,
            [
                PathBuf::from("/home/ana/.codex/sessions"),
                PathBuf::from("/home/ana/.codex/archived_sessions")
            ]
        );
        let stale = Disclosure::new(SourceId::CodexCli, &[PathBuf::from("/elsewhere")], &env);
        assert_eq!(
            store.enable(
                SourceId::CodexCli,
                Vec::new(),
                &stale.digest,
                &env,
                Utc::now()
            ),
            Err(CostError::ConsentChanged)
        );
        let on = store
            .enable(
                SourceId::CodexCli,
                Vec::new(),
                &disclosure.digest,
                &env,
                Utc::now(),
            )
            .unwrap();
        assert_eq!(on.enabled_sources(&env), [SourceId::CodexCli]);
        assert_eq!(store.load().unwrap(), on);
        // Consent is per source.
        assert!(!on.enabled(SourceId::ClaudeCode, &env));
        // Another home means other directories: the consent no longer holds.
        assert!(!on.enabled(SourceId::CodexCli, &self::env("/home/bia")));

        let off = store.disable(SourceId::CodexCli).unwrap();
        assert!(off.enabled_sources(&env).is_empty());
    }

    #[test]
    fn chosen_directories_replace_the_defaults_and_are_listed() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(dir.path().join("settings.json"));
        let env = env("/home/ana");
        let chosen = vec![PathBuf::from("~/archive/claude")];
        let disclosure = Disclosure::new(SourceId::ClaudeCode, &chosen, &env);
        assert!(disclosure.custom);
        assert_eq!(
            disclosure.directories,
            [PathBuf::from("/home/ana/archive/claude")]
        );
        let lines: Vec<String> = disclosure
            .lines()
            .iter()
            .map(|l| l.resolve("en-US"))
            .collect();
        assert!(
            lines.iter().any(|line| line == "/home/ana/archive/claude"),
            "{lines:?}"
        );
        let on = store
            .enable(
                SourceId::ClaudeCode,
                chosen,
                &disclosure.digest,
                &env,
                Utc::now(),
            )
            .unwrap();
        assert_eq!(
            on.roots(SourceId::ClaudeCode, &env),
            [PathBuf::from("/home/ana/archive/claude")]
        );
        assert!(on.enabled(SourceId::ClaudeCode, &env));
    }

    #[test]
    fn a_damaged_file_is_an_error_never_on() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, b"{ not json").unwrap();
        let store = SettingsStore::at(&path);
        assert_eq!(store.load(), Err(CostError::Settings));
        assert_eq!(
            store.disable(SourceId::ClaudeCode),
            Err(CostError::Settings)
        );
    }

    #[test]
    fn period_and_retention_are_saved() {
        let dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::at(dir.path().join("settings.json"));
        store.set_period(Period::CurrentMonth).unwrap();
        store.set_retention(Retention::days(30).unwrap()).unwrap();
        let settings = store.load().unwrap();
        assert_eq!(settings.period, Period::CurrentMonth);
        assert_eq!(settings.retention().keep_days(), 30);
        assert_eq!(Retention::days(6), Err(CostError::RetentionBounds));
        assert_eq!(Retention::days(731), Err(CostError::RetentionBounds));
        // The settings file and the desktop use the documented slugs.
        for period in Period::ALL {
            assert_eq!(serde_json::json!(period), period.slug());
        }
        std::fs::write(store.path(), r#"{"period":"last_30_days"}"#).unwrap();
        assert_eq!(store.load().unwrap().period, Period::Last30Days);
    }

    #[cfg(unix)]
    #[test]
    fn the_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cost/settings.json");
        SettingsStore::at(&path).set_period(Period::Today).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir_mode = std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(dir_mode & 0o777, 0o700);
    }
}
