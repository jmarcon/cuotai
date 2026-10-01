//! Config file discovery, loading and atomic saving.
//!
//! Save policy: on save we resolve symlinks to the real destination, re-read the
//! current bytes and refuse to write if they differ from the snapshot taken at
//! load time (external edit, unexpected creation or deletion), then re-check
//! again immediately before the atomic rename (`persist_noclobber` when the
//! file should not exist yet) to shrink the TOCTOU window. Bytes are written
//! to a temporary file in the destination directory, fsynced, then atomically
//! renamed over the target, so the file is never partially overwritten. New
//! files get mode 0600 on Unix; existing files keep their permissions (and
//! permission failures abort the save). When the previously loaded document
//! parses, we edit it with `toml_edit`, matching `[[accounts]]` tables
//! positionally so order, comments and formatting survive renames; new
//! accounts are appended and serialized with `toml_edit::ser`, as are brand
//! new files. The rendered document is parsed back and compared to the
//! requested config before writing. Only on success are `config`/`exists`/
//! snapshot updated; on any failure `self` is untouched.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use cuotai_core::{Account, Config};

pub(crate) struct ConfigFile {
    pub config: Config,
    pub path: PathBuf,
    pub exists: bool,
    /// Byte snapshot of the file when loaded/saved; `None` when it did not exist.
    original: Option<Vec<u8>>,
}

impl ConfigFile {
    /// Load precedence: explicit path, `./cuotai.toml`, platform config dir
    /// (`dirs::config_dir()/cuotai/config.toml`), otherwise a new `./cuotai.toml`.
    ///
    /// Missing file: interactive always yields an empty-accounts config with
    /// `exists = false` (so the caller can open the wizard); non-interactive
    /// errors only for an explicit path, otherwise falls back to the default
    /// provider catalog. Errors never include raw TOML contents.
    pub fn load(explicit: Option<PathBuf>, interactive: bool) -> Result<Self, String> {
        let local = PathBuf::from("cuotai.toml");
        let implicit = explicit.is_none();
        let path = match explicit {
            Some(path) => path,
            None if local.is_file() => local.clone(),
            None => match dirs::config_dir() {
                Some(dir) if dir.join("cuotai/config.toml").is_file() => {
                    dir.join("cuotai/config.toml")
                }
                _ => local,
            },
        };
        match fs::read(&path) {
            Ok(bytes) => {
                let text = std::str::from_utf8(&bytes).map_err(|_| {
                    format!("arquivo {} invalido: conteudo nao e UTF-8", path.display())
                })?;
                // ConfigError messages are fixed strings and never echo the body.
                let config = Config::from_toml(text)
                    .map_err(|e| format!("arquivo {} invalido: {}", path.display(), e))?;
                Ok(Self {
                    config,
                    path,
                    exists: true,
                    original: Some(bytes),
                })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if interactive {
                    let config = Config {
                        accounts: Vec::new(),
                        ..Config::default()
                    };
                    Ok(Self {
                        config,
                        path,
                        exists: false,
                        original: None,
                    })
                } else if implicit {
                    Ok(Self {
                        config: Config::default(),
                        path,
                        exists: false,
                        original: None,
                    })
                } else {
                    Err(format!("arquivo {} nao encontrado", path.display()))
                }
            }
            Err(e) => Err(format!("falha ao ler {}: {}", path.display(), e)),
        }
    }

    /// Validate and durably persist `config`. See module docs for the policy.
    pub fn save(&mut self, config: Config) -> Result<(), String> {
        config.validate().map_err(|e| e.to_string())?;
        if config.refresh_seconds > i64::MAX as u64
            || config.concurrency > i64::MAX as usize
            || config.timeout_seconds > i64::MAX as u64
        {
            return Err("valores numericos grandes demais para TOML".into());
        }
        // Resolve symlinks so we replace the destination, never the link.
        let dest = match fs::canonicalize(&self.path) {
            Ok(real) => real,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => self.path.clone(),
            Err(e) => return Err(format!("falha ao resolver {}: {}", self.path.display(), e)),
        };
        // Refuse stale/external edits and unexpected creation/deletion.
        self.check_fresh(&dest)?;
        let bytes = render(&config, self.original.as_deref())?;
        let dir = dest
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let mut tmp = tempfile::NamedTempFile::new_in(&dir)
            .map_err(|e| format!("falha ao criar arquivo temporario: {}", e))?;
        if let Some(perms) = current_permissions(&dest) {
            fs::set_permissions(tmp.path(), perms)
                .map_err(|e| format!("falha ao ajustar permissoes: {}", e))?;
        } else {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o600))
                    .map_err(|e| format!("falha ao ajustar permissoes: {}", e))?;
            }
        }
        tmp.write_all(&bytes)
            .map_err(|e| format!("falha ao escrever arquivo temporario: {}", e))?;
        tmp.as_file()
            .sync_all()
            .map_err(|e| format!("falha ao sincronizar arquivo temporario: {}", e))?;
        // Last freshness check right before the rename shrinks the TOCTOU window.
        self.check_fresh(&dest)?;
        let result = if self.original.is_none() {
            tmp.persist_noclobber(&dest).map_err(|e| e.error)
        } else {
            tmp.persist(&dest).map_err(|e| e.error)
        };
        result.map_err(|e| format!("falha ao gravar {}: {}", dest.display(), e))?;
        self.config = config;
        self.exists = true;
        self.original = Some(bytes);
        Ok(())
    }

    fn check_fresh(&self, dest: &Path) -> Result<(), String> {
        let current = match fs::read(dest) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("falha ao reler {}: {}", dest.display(), e)),
        };
        if current != self.original {
            return Err(format!(
                "arquivo {} mudou desde o carregamento; feche e reabra o Cuotai antes de salvar",
                dest.display()
            ));
        }
        Ok(())
    }
}

fn current_permissions(path: &Path) -> Option<fs::Permissions> {
    fs::metadata(path).ok().map(|m| m.permissions())
}

/// Serialize `config`, editing the previous document when it parses so
/// comments and formatting of retained entries survive. The result is parsed
/// back and must be equivalent to `config`, otherwise the save is refused.
fn render(config: &Config, previous: Option<&[u8]>) -> Result<Vec<u8>, String> {
    let text = match previous
        .and_then(|b| std::str::from_utf8(b).ok())
        .and_then(|s| s.parse::<toml_edit::DocumentMut>().ok())
    {
        Some(mut doc) => {
            set_int(&mut doc, "refresh_seconds", config.refresh_seconds as i64);
            set_int(&mut doc, "concurrency", config.concurrency as i64);
            set_int(&mut doc, "timeout_seconds", config.timeout_seconds as i64);
            let editable = doc
                .get("accounts")
                .map(|item| item.as_array_of_tables().is_some())
                .unwrap_or(true);
            if !editable {
                if config.accounts.is_empty() {
                    doc["accounts"] =
                        toml_edit::Item::Value(toml_edit::Value::Array(toml_edit::Array::new()));
                    doc.to_string()
                } else {
                    serialize_fresh(config)?
                }
            } else {
                update_accounts(&mut doc, &config.accounts)?;
                doc.to_string()
            }
        }
        None => serialize_fresh(config)?,
    };
    // Round-trip guard: the rendered document must describe the same config.
    let parsed =
        Config::from_toml(&text).map_err(|e| format!("documento gerado invalido: {}", e))?;
    let want =
        toml::to_string(config).map_err(|e| format!("falha ao serializar configuracao: {}", e))?;
    let got =
        toml::to_string(&parsed).map_err(|e| format!("falha ao serializar configuracao: {}", e))?;
    if want != got {
        return Err("documento gerado diverge da configuracao".into());
    }
    Ok(text.into_bytes())
}

fn serialize_fresh(config: &Config) -> Result<String, String> {
    toml_edit::ser::to_document(config)
        .map(|doc| doc.to_string())
        .map_err(|e| format!("falha ao serializar configuracao: {}", e))
}

/// Replace a scalar while keeping the existing value decor (e.g. trailing comments).
fn set_int(doc: &mut toml_edit::DocumentMut, key: &str, val: i64) {
    if let Some(existing) = doc.get_mut(key).and_then(|i| i.as_value_mut()) {
        let decor = existing.decor().clone();
        let mut value = toml_edit::Value::from(val);
        *value.decor_mut() = decor;
        *existing = value;
    } else {
        doc[key] = toml_edit::value(val);
    }
}

/// Match `[[accounts]]` tables positionally: the wizard edits in place or
/// appends, so table order and comments survive renames. Extra tables are
/// dropped, missing ones appended.
fn update_accounts(doc: &mut toml_edit::DocumentMut, accounts: &[Account]) -> Result<(), String> {
    if accounts.is_empty() {
        doc["accounts"] = toml_edit::value(toml_edit::Array::new());
        return Ok(());
    }
    if !doc.contains_key("accounts") {
        doc["accounts"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    }
    let array = doc["accounts"]
        .as_array_of_tables_mut()
        .ok_or_else(|| "chave 'accounts' existente nao e uma lista de tabelas".to_string())?;
    // Deletion preserves the original table/comment belonging to each survivor.
    // Renames still use positional matching when the account count is unchanged.
    if accounts.len() < array.len() {
        let mut index = 0;
        while index < array.len() {
            let keep = array
                .get(index)
                .and_then(|table| table.get("name"))
                .and_then(|name| name.as_str())
                .is_some_and(|name| accounts.iter().any(|account| account.name == name));
            if keep {
                index += 1;
            } else {
                array.remove(index);
            }
        }
    }
    for (index, account) in accounts.iter().enumerate() {
        if index < array.len() {
            edit_account(array.get_mut(index).expect("index checked"), account)?;
        } else {
            let doc = toml_edit::ser::to_document(account)
                .map_err(|e| format!("falha ao serializar conta {}: {}", account.name, e))?;
            let mut table = toml_edit::Table::new();
            for (key, item) in doc.iter() {
                table[key] = item.clone();
            }
            array.push(table);
        }
    }
    while array.len() > accounts.len() {
        array.remove(array.len() - 1);
    }
    Ok(())
}

/// Overwrite a table's fields, preserving value decor (trailing comments) and
/// the key/prefix comments that stay attached to the table itself.
fn edit_account(table: &mut toml_edit::Table, account: &Account) -> Result<(), String> {
    let doc = toml_edit::ser::to_document(account)
        .map_err(|e| format!("falha ao serializar conta {}: {}", account.name, e))?;
    for (key, item) in doc.iter() {
        if let (Some(old), Some(new_value)) = (
            table.get_mut(key).and_then(|i| i.as_value_mut()),
            item.as_value(),
        ) {
            let decor = old.decor().clone();
            let mut value = new_value.clone();
            *value.decor_mut() = decor;
            *old = value;
        } else {
            table[key] = item.clone();
        }
    }
    if account.credential_env.is_none() {
        table.remove("credential_env");
    }
    if account.oauth.is_none() {
        table.remove("oauth");
    }
    // Override individual de refresh_seconds: só persiste quando setado;
    // limpar o campo no assistente remove a chave (herda o global).
    if account.refresh_seconds.is_none() {
        table.remove("refresh_seconds");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn account(name: &str, env: Option<&str>) -> Account {
        Account {
            name: name.into(),
            provider: cuotai_core::Provider::OpenAiApi,
            credential_env: env.map(str::to_string),
            oauth: None,
            params: BTreeMap::new(),
            refresh_seconds: None,
        }
    }

    fn config(accounts: Vec<Account>) -> Config {
        Config {
            refresh_seconds: 60,
            concurrency: 2,
            timeout_seconds: 10,
            accounts,
        }
    }

    fn load_at(path: &Path, interactive: bool) -> Result<ConfigFile, String> {
        ConfigFile::load(Some(path.to_path_buf()), interactive)
    }

    #[test]
    fn new_file_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file = load_at(&path, true).unwrap();
        assert!(!file.exists);
        assert!(file.config.accounts.is_empty());
        let cfg = config(vec![account("OpenAI", Some("OPENAI_ADMIN_KEY"))]);
        file.save(cfg.clone()).unwrap();
        assert!(file.exists);
        assert_eq!(file.path, path);
        let reloaded = load_at(&path, true).unwrap();
        assert_eq!(
            toml::to_string(&reloaded.config).unwrap(),
            toml::to_string(&cfg).unwrap()
        );
    }

    #[test]
    fn save_preserves_comments_params_and_other_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        fs::write(
            &path,
            "# cabecalho\nrefresh_seconds = 300 # rodape\nconcurrency = 4\ntimeout_seconds = 30\n\n[[accounts]]\nname = \"OpenAI\"\nprovider = \"open_ai_api\"\ncredential_env = \"OPENAI_ADMIN_KEY\"\n# parametros extras\nparams = { project_ids = \"proj_1\", models = \"gpt-4.1\" }\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        let mut cfg = file.config.clone();
        cfg.refresh_seconds = 60;
        cfg.accounts.push(account("Nova", None));
        file.save(cfg).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# cabecalho"));
        assert!(text.contains("# rodape"));
        assert!(text.contains("# parametros extras"));
        assert!(text.contains("project_ids = \"proj_1\""));
        assert!(text.contains("refresh_seconds = 60"));
        assert!(text.contains("name = \"Nova\""));
        // Saved document must still parse to the same config.
        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(reloaded.config.accounts.len(), 2);
        assert_eq!(
            reloaded.config.accounts[0]
                .params
                .get("project_ids")
                .map(String::as_str),
            Some("proj_1")
        );
    }

    #[test]
    fn rename_first_of_two_keeps_order_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\n\n[[accounts]]\n# primeira\nname = \"A\"\nprovider = \"open_ai_api\"\n\n[[accounts]]\n# segunda\nname = \"B\"\nprovider = \"anthropic_api\"\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        let mut cfg = file.config.clone();
        cfg.accounts[0].name = "Renomeada".into();
        file.save(cfg.clone()).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# primeira"));
        assert!(text.contains("# segunda"));
        let first = text.find("Renomeada").unwrap();
        let second = text.find("\"B\"").unwrap();
        assert!(first < second, "{text}");
        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(
            toml::to_string(&reloaded.config).unwrap(),
            toml::to_string(&cfg).unwrap()
        );
    }

    #[test]
    fn conflict_refuses_external_edit_and_keeps_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        fs::write(
            &path,
            "refresh_seconds = 100\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let err = file.save(config(vec![])).unwrap_err();
        assert!(err.contains("feche e reabra"), "{err}");
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "refresh_seconds = 100\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n"
        );
    }

    #[test]
    fn unexpected_creation_refused() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file = load_at(&path, true).unwrap();
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        assert!(file.save(config(vec![])).is_err());
    }

    #[test]
    fn write_failure_leaves_object_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gone").join("cuotai.toml");
        let mut file = load_at(&path, true).unwrap();
        let before = file.config.accounts.len();
        let err = file.save(config(vec![account("X", None)])).unwrap_err();
        assert!(!err.is_empty());
        assert!(!file.exists);
        assert_eq!(file.config.accounts.len(), before);
    }

    #[test]
    fn missing_explicit_interactive_vs_headless() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let headless = load_at(&path, false);
        assert!(headless.is_err());
        let interactive = load_at(&path, true).unwrap();
        assert!(!interactive.exists);
        assert!(interactive.config.accounts.is_empty());
    }

    #[test]
    fn invalid_toml_error_has_no_body() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        fs::write(&path, "segredo = [").unwrap();
        let err = load_at(&path, false)
            .err()
            .expect("TOML inválido deve falhar");
        assert!(!err.contains("segredo"), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn new_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cuotai.toml");
        let mut file = load_at(&path, true).unwrap();
        file.save(config(vec![])).unwrap();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn save_follows_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.toml");
        fs::write(
            &real,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\naccounts = []\n",
        )
        .unwrap();
        let link = dir.path().join("link.toml");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let mut file = load_at(&link, false).unwrap();
        file.save(config(vec![])).unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert!(fs::read_to_string(&real)
            .unwrap()
            .contains("refresh_seconds = 60"));
    }
    #[test]
    fn switching_oauth_to_environment_removes_saved_oauth_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("auth.toml");
        let mut file = load_at(&path, true).unwrap();
        let mut account = account("Google", None);
        account.provider = cuotai_core::Provider::GeminiApi;
        account.oauth = Some(cuotai_core::oauth::OAuthConfig::new("own-client".into()));
        file.save(config(vec![account.clone()])).unwrap();
        assert_eq!(
            load_at(&path, false).unwrap().config.accounts[0].oauth,
            account.oauth
        );
        account.oauth = None;
        account.credential_env = Some("GOOGLE_CLOUD_ACCESS_TOKEN".into());
        file.save(config(vec![account])).unwrap();
        let loaded = load_at(&path, false).unwrap();
        assert!(loaded.config.accounts[0].oauth.is_none());
        assert_eq!(
            loaded.config.accounts[0].credential_env.as_deref(),
            Some("GOOGLE_CLOUD_ACCESS_TOKEN")
        );
    }
    #[test]
    fn native_login_options_roundtrip_without_credentials_in_toml() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("native.toml");
        let mut file = load_at(&path, true).unwrap();
        let mut account = account("Kiro", None);
        account.provider = cuotai_core::Provider::Kiro;
        let mut oauth = cuotai_core::oauth::OAuthConfig::new(String::new());
        oauth
            .options
            .insert("auth_method".into(), "builder-id".into());
        oauth.options.insert("region".into(), "us-east-1".into());
        account.oauth = Some(oauth.clone());
        file.save(config(vec![account])).unwrap();
        let loaded = load_at(&path, false).unwrap();
        assert_eq!(loaded.config.accounts[0].oauth, Some(oauth));
        assert!(loaded.config.accounts[0].credential_env.is_none());
    }
    #[test]
    fn deleting_accounts_preserves_survivors_and_last_deletion_reloads_empty() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("delete.toml");
        fs::write(&path, "[[accounts]]\nname='first'\nprovider='groq'\n[[accounts]]\nname='middle'\nprovider='kimi'\n[[accounts]]\n# survivor comment\nname='last'\nprovider='ollama_cloud'\ncredential_env='LAST_KEY'\n").unwrap();
        let mut file = load_at(&path, false).unwrap();
        let mut edited = file.config.clone();
        edited.accounts.remove(1);
        file.save(edited).unwrap();
        let loaded = load_at(&path, false).unwrap();
        assert_eq!(
            loaded
                .config
                .accounts
                .iter()
                .map(|a| a.name.as_str())
                .collect::<Vec<_>>(),
            ["first", "last"]
        );
        assert_eq!(
            loaded.config.accounts[1].credential_env.as_deref(),
            Some("LAST_KEY")
        );
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("# survivor comment"));
        let mut empty = file.config.clone();
        empty.accounts.clear();
        file.save(empty).unwrap();
        assert!(load_at(&path, false).unwrap().config.accounts.is_empty());
    }

    #[test]
    fn account_refresh_override_round_trips_and_clears_when_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("refresh.toml");
        let mut file = load_at(&path, true).unwrap();
        let mut overridden = account("OpenAI", Some("OPENAI_ADMIN_KEY"));
        overridden.refresh_seconds = Some(120);
        file.save(config(vec![overridden])).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("refresh_seconds = 120"),
            "override não persistido: {text}"
        );
        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(reloaded.config.accounts[0].refresh_seconds, Some(120));
        // Limpar o override remove a chave do arquivo (sem campo fantasma).
        let mut cleared = reloaded.config.clone();
        cleared.accounts[0].refresh_seconds = None;
        file.save(cleared).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("refresh_seconds = 120"),
            "override não foi removido: {text}"
        );
        let final_loaded = load_at(&path, false).unwrap();
        assert!(final_loaded.config.accounts[0].refresh_seconds.is_none());
    }

    #[test]
    fn account_refresh_override_does_not_touch_global_interval() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("refresh-global.toml");
        let mut file = load_at(&path, true).unwrap();
        let mut config = file.config.clone();
        config.refresh_seconds = 240;
        let mut overridden = account("OpenAI", Some("OPENAI_ADMIN_KEY"));
        overridden.refresh_seconds = Some(45);
        config.accounts.push(overridden);
        file.save(config.clone()).unwrap();
        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(reloaded.config.refresh_seconds, 240);
        assert_eq!(reloaded.config.accounts[0].refresh_seconds, Some(45));
    }

    #[test]
    fn editing_account_preserves_unrelated_comments_and_other_accounts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mixed.toml");
        fs::write(
            &path,
            "refresh_seconds = 300\nconcurrency = 4\ntimeout_seconds = 30\n\n[[accounts]]\nname = \"Vizinha\"\nprovider = \"open_ai_api\"\ncredential_env = \"VIZINHA_KEY\"\n# comentário da vizinha\nparams = { foo = \"bar\" }\n\n[[accounts]]\nname = \"OpenAI\"\nprovider = \"open_ai_api\"\ncredential_env = \"OPENAI_ADMIN_KEY\"\nparams = { project_ids = \"proj_1\" }\n",
        )
        .unwrap();
        let mut file = load_at(&path, false).unwrap();
        let mut cfg = file.config.clone();
        cfg.accounts[1].refresh_seconds = Some(90);
        file.save(cfg).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# comentário da vizinha"));
        assert!(text.contains("project_ids = \"proj_1\""));
        assert!(text.contains("refresh_seconds = 90"));
        let reloaded = load_at(&path, false).unwrap();
        assert_eq!(reloaded.config.accounts[1].refresh_seconds, Some(90));
        assert!(reloaded.config.accounts[0].refresh_seconds.is_none());
    }
}
