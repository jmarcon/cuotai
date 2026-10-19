//! Configurações do app desktop (não confundir com o `cuotai.toml` de contas, que é do core).
//! Arquivo `desktop.json` na pasta padrão de configuração: pasta de temas e idiomas e o
//! `cuotai.toml` escolhido pelo usuário (só o caminho; nunca dentro do TOML).
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Serialize, Deserialize, Default, Clone)]
struct Stored {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user_dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    config_file: Option<String>,
    /// Animações (PRD 0004 RF1.2); ausente = `system`. Texto livre na leitura: um valor
    /// desconhecido vira `system` sem invalidar os demais campos do arquivo.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    motion: Option<String>,
}

/// Valores aceitos para `motion`, na ordem exibida pela UI.
pub const MOTIONS: [&str; 4] = ["system", "always", "reduced", "off"];

fn motion_of(stored: &Stored) -> &'static str {
    MOTIONS
        .into_iter()
        .find(|m| stored.motion.as_deref() == Some(m))
        .unwrap_or("system")
}

/// Pasta padrão de configuração do Cuotai (`<config do SO>/cuotai`).
pub fn default_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("cuotai"))
}

fn settings_file() -> Option<PathBuf> {
    default_dir().map(|d| d.join("desktop.json"))
}

fn read(file: &Path) -> Stored {
    fs::read_to_string(file)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn load() -> Stored {
    settings_file().map(|f| read(&f)).unwrap_or_default()
}

/// Altera um campo preservando os demais. Escrita atômica (temporário + rename).
fn update(file: &Path, change: impl FnOnce(&mut Stored)) -> Result<(), &'static str> {
    let parent = file.parent().ok_or("unwritable")?;
    fs::create_dir_all(parent).map_err(|_| "unwritable")?;
    let mut stored = read(file);
    change(&mut stored);
    let tmp = parent.join(".desktop.json.tmp");
    let text = serde_json::to_string_pretty(&stored).map_err(|_| "unwritable")?;
    fs::write(&tmp, text).map_err(|_| "unwritable")?;
    fs::rename(&tmp, file).map_err(|_| "unwritable")
}

/// Pasta personalizada salva pelo usuário (sem considerar `CUOTAI_USER_DIR`).
pub fn custom_user_dir() -> Option<PathBuf> {
    load()
        .user_dir
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// A variável de ambiente tem prioridade e bloqueia a edição pela interface.
pub fn env_user_dir() -> Option<PathBuf> {
    std::env::var_os("CUOTAI_USER_DIR").map(PathBuf::from)
}

/// Pasta efetiva: variável de ambiente → configuração do usuário → padrão.
pub fn effective_user_dir() -> Option<PathBuf> {
    env_user_dir().or_else(custom_user_dir).or_else(default_dir)
}

/// Erros com código estável (a UI traduz): `invalid_path`, `not_dir`, `unwritable`.
pub fn validate_dir(path: &Path) -> Result<(), &'static str> {
    if !path.is_absolute() || path.parent().is_none() || path.components().count() < 3 {
        return Err("invalid_path"); // relativo ou raiz do disco / pasta de topo
    }
    if path.exists() && !path.is_dir() {
        return Err("not_dir");
    }
    fs::create_dir_all(path).map_err(|_| "unwritable")?;
    let probe = path.join(".cuotai-write-test");
    fs::File::create(&probe)
        .and_then(|mut f| f.write_all(b"ok"))
        .map_err(|_| "unwritable")?;
    let _ = fs::remove_file(probe);
    Ok(())
}

/// Grava (ou, com `None`, remove) a pasta personalizada.
pub fn save_user_dir(dir: Option<&Path>) -> Result<(), &'static str> {
    if let Some(d) = dir {
        validate_dir(d)?;
    }
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| s.user_dir = dir.map(|d| d.display().to_string()))
}

/// `cuotai.toml` escolhido nas configurações (sem considerar `CUOTAI_CONFIG`).
pub fn chosen_config() -> Option<PathBuf> {
    load()
        .config_file
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// Erro com código estável (a UI traduz): `not_file` (relativo, ausente ou pasta).
pub fn validate_config_file(path: &Path) -> Result<(), &'static str> {
    if path.is_absolute() && path.is_file() {
        Ok(())
    } else {
        Err("not_file")
    }
}

/// Grava (ou, com `None`, remove) o arquivo escolhido; o chamador já validou e carregou.
pub fn save_config_file(path: Option<&Path>) -> Result<(), &'static str> {
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| {
        s.config_file = path.map(|p| p.display().to_string())
    })
}

/// Grava a preferência de animações. Erro com código estável: `invalid_motion`.
pub fn save_motion(motion: &str) -> Result<(), &'static str> {
    if !MOTIONS.contains(&motion) {
        return Err("invalid_motion");
    }
    let file = settings_file().ok_or("unwritable")?;
    update(&file, |s| {
        s.motion = (motion != "system").then(|| motion.to_owned())
    })
}

#[derive(Serialize)]
pub struct SettingsView {
    /// Pasta em uso agora.
    user_dir: Option<String>,
    default_dir: Option<String>,
    /// Pasta escolhida pelo usuário (vazia = usa a padrão).
    custom_dir: Option<String>,
    /// `CUOTAI_USER_DIR` definida: a escolha pela interface não tem efeito.
    env_locked: bool,
    /// `cuotai.toml` escolhido (vazio = padrão do SO).
    config_file: Option<String>,
    /// `CUOTAI_CONFIG` definida: tem prioridade sobre o arquivo escolhido.
    config_env_locked: bool,
    /// Animações: `system`, `always`, `reduced` ou `off`.
    motion: &'static str,
    version: &'static str,
}

pub fn view() -> SettingsView {
    SettingsView {
        user_dir: effective_user_dir().map(|p| p.display().to_string()),
        default_dir: default_dir().map(|p| p.display().to_string()),
        custom_dir: custom_user_dir().map(|p| p.display().to_string()),
        env_locked: env_user_dir().is_some(),
        config_file: chosen_config().map(|p| p.display().to_string()),
        config_env_locked: cuotai_core::config_file::Locations::from_process()
            .env
            .is_some(),
        motion: motion_of(&load()),
        version: env!("CARGO_PKG_VERSION"),
    }
}

/// Abre a pasta no gerenciador de arquivos do sistema. Só a pasta efetiva, nunca um caminho vindo da UI.
pub fn open_user_dir() -> Result<(), &'static str> {
    let dir = effective_user_dir().ok_or("invalid_path")?;
    fs::create_dir_all(&dir).map_err(|_| "unwritable")?;
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "linux")]
    let mut cmd = std::process::Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut cmd = std::process::Command::new("explorer");
    cmd.arg(&dir).spawn().map(|_| ()).map_err(|_| "unwritable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_relative_root_and_files() {
        assert_eq!(
            validate_dir(Path::new("relativo/pasta")),
            Err("invalid_path")
        );
        assert_eq!(validate_dir(Path::new("/")), Err("invalid_path"));
        assert_eq!(validate_dir(Path::new("/tmp")), Err("invalid_path"));
        let file = std::env::temp_dir().join(format!("cuotai-file-{}", std::process::id()));
        fs::write(&file, "x").unwrap();
        assert_eq!(validate_dir(&file), Err("not_dir"));
        let _ = fs::remove_file(file);
    }

    #[test]
    fn accepts_writable_dir() {
        let dir = std::env::temp_dir().join(format!("cuotai-ok-{}", std::process::id()));
        assert_eq!(validate_dir(&dir), Ok(()));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn choices_are_updated_independently_and_reset() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        update(&file, |s| s.user_dir = Some("/u".into())).unwrap();
        update(&file, |s| s.config_file = Some("/c/cuotai.toml".into())).unwrap();
        let stored = read(&file);
        assert_eq!(stored.user_dir.as_deref(), Some("/u"));
        assert_eq!(stored.config_file.as_deref(), Some("/c/cuotai.toml"));
        update(&file, |s| s.config_file = None).unwrap();
        let stored = read(&file);
        assert_eq!(stored.user_dir.as_deref(), Some("/u"));
        assert_eq!(stored.config_file, None);
    }

    #[test]
    fn motion_defaults_to_system_and_round_trips_without_touching_other_fields() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("desktop.json");
        assert_eq!(motion_of(&read(&file)), "system");
        update(&file, |s| s.user_dir = Some("/u".into())).unwrap();
        update(&file, |s| s.motion = Some("off".into())).unwrap();
        let stored = read(&file);
        assert_eq!(motion_of(&stored), "off");
        assert_eq!(stored.user_dir.as_deref(), Some("/u"));
        // Valor desconhecido (edição manual) cai no padrão e não apaga o resto.
        fs::write(&file, r#"{"user_dir":"/u","motion":"fast"}"#).unwrap();
        let stored = read(&file);
        assert_eq!(motion_of(&stored), "system");
        assert_eq!(stored.user_dir.as_deref(), Some("/u"));
        assert_eq!(save_motion("fast"), Err("invalid_motion"));
    }
}
