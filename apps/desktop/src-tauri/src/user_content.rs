//! Temas e idiomas do usuário: lê `<pasta de configuração>/cuotai/{themes,locales}` e entrega os textos à UI,
//! que valida e registra. Somente leitura; nenhum arquivo é executado e a UI aplica as regras de segurança.
use serde::Serialize;
use std::{fs, path::PathBuf};

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_FILES: usize = 64;

#[derive(Serialize)]
pub struct UserFile {
    name: String,
    text: String,
}

#[derive(Serialize)]
pub struct UserError {
    kind: &'static str,
    name: String,
    reason: &'static str,
}

#[derive(Serialize)]
pub struct UserContent {
    dir: String,
    themes: Vec<UserFile>,
    locales: Vec<UserFile>,
    errors: Vec<UserError>,
}

/// Pasta efetiva (variável de ambiente → configuração do usuário → padrão); ver `settings.rs`.
pub fn user_dir() -> Option<PathBuf> {
    crate::settings::effective_user_dir()
}

fn read_dir(
    dir: PathBuf,
    ext: &str,
    kind: &'static str,
    errors: &mut Vec<UserError>,
) -> Vec<UserFile> {
    let _ = fs::create_dir_all(&dir); // facilita achar a pasta; falha silenciosa (somente leitura)
    let Ok(entries) = fs::read_dir(&dir) else {
        return vec![];
    };
    let mut files = Vec::new();
    for entry in entries.flatten().take(MAX_FILES) {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_owned) else {
            continue;
        };
        let too_big = entry
            .metadata()
            .map(|m| m.len() > MAX_FILE_BYTES)
            .unwrap_or(true);
        if too_big {
            errors.push(UserError {
                kind,
                name,
                reason: "tooLarge",
            });
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(text) => files.push(UserFile { name, text }),
            Err(_) => errors.push(UserError {
                kind,
                name,
                reason: "unreadable",
            }),
        }
    }
    files.sort_by(|a, b| a.name.cmp(&b.name));
    files
}

pub fn load() -> Option<UserContent> {
    let base = user_dir()?;
    let mut errors = Vec::new();
    let themes = read_dir(base.join("themes"), "css", "theme", &mut errors);
    let locales = read_dir(base.join("locales"), "json", "locale", &mut errors);
    Some(UserContent {
        dir: base.display().to_string(),
        themes,
        locales,
        errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_user_files_and_flags_oversized_ones() {
        let base = std::env::temp_dir().join(format!("cuotai-user-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("themes")).unwrap();
        fs::create_dir_all(base.join("locales")).unwrap();
        fs::write(
            base.join("themes/ocean.css"),
            ":root[data-theme=\"ocean\"]{}",
        )
        .unwrap();
        fs::write(base.join("themes/ignored.txt"), "x").unwrap();
        fs::write(
            base.join("themes/huge.css"),
            vec![b'a'; (MAX_FILE_BYTES + 1) as usize],
        )
        .unwrap();
        fs::write(base.join("locales/fr.json"), "{}").unwrap();
        std::env::set_var("CUOTAI_USER_DIR", &base);

        let c = load().expect("pasta de usuário");
        assert_eq!(
            c.themes.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["ocean.css"]
        );
        assert_eq!(c.locales.len(), 1);
        assert!(c
            .errors
            .iter()
            .any(|e| e.name == "huge.css" && e.reason == "tooLarge"));
        let _ = fs::remove_dir_all(&base);
    }
}
