//! User themes and languages: reads `<config folder>/cuotai/{themes,locales}` and hands the texts to the UI,
//! which validates and registers them. Read-only; no file is executed.
//!
//! PRD 0050 RF3.3/RF3.4: each folder lists at most [`MAX_FILES`] eligible regular files (symbolic links refused),
//! sorted by name before the cap so the same files always win, and the excess is reported by name. A language
//! pack loses its protected ids (`cuotai_core::user_pack`) here, before the webview sees it; the built-in and
//! core texts keep those words.
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
    /// `protected`: how many ids the pack may not replace were ignored.
    #[serde(skip_serializing_if = "Option::is_none")]
    count: Option<usize>,
}

impl UserError {
    fn new(kind: &'static str, name: String, reason: &'static str) -> Self {
        Self {
            kind,
            name,
            reason,
            count: None,
        }
    }
}

#[derive(Serialize)]
pub struct UserContent {
    dir: String,
    themes: Vec<UserFile>,
    locales: Vec<UserFile>,
    errors: Vec<UserError>,
}

/// Effective folder (environment variable → user setting → default); see `settings.rs`.
pub fn user_dir() -> Option<PathBuf> {
    crate::settings::effective_user_dir()
}

fn read_dir(
    dir: PathBuf,
    ext: &str,
    kind: &'static str,
    errors: &mut Vec<UserError>,
) -> Vec<UserFile> {
    let _ = fs::create_dir_all(&dir); // makes the folder easier to find; silent failure (read-only)
    let Ok(entries) = fs::read_dir(&dir) else {
        return vec![];
    };
    // Eligible names only (the extension, valid UTF-8), sorted before anything is read or capped.
    let mut names: Vec<String> = entries
        .flatten()
        .filter_map(|entry| entry.file_name().to_str().map(str::to_owned))
        .filter(|name| {
            std::path::Path::new(name)
                .extension()
                .and_then(|e| e.to_str())
                == Some(ext)
        })
        .collect();
    names.sort();
    let mut files = Vec::new();
    let mut eligible = 0;
    for name in names {
        let path = dir.join(&name);
        // A symbolic link could point anywhere; only regular files of the folder are read.
        let Ok(meta) = fs::symlink_metadata(&path) else {
            errors.push(UserError::new(kind, name, "unreadable"));
            continue;
        };
        if meta.file_type().is_symlink() {
            errors.push(UserError::new(kind, name, "symlink"));
            continue;
        }
        if !meta.is_file() {
            continue;
        }
        eligible += 1;
        if eligible > MAX_FILES {
            errors.push(UserError::new(kind, name, "tooMany"));
            continue;
        }
        if meta.len() > MAX_FILE_BYTES {
            errors.push(UserError::new(kind, name, "tooLarge"));
            continue;
        }
        match fs::read_to_string(&path) {
            Ok(text) => files.push(UserFile { name, text }),
            Err(_) => errors.push(UserError::new(kind, name, "unreadable")),
        }
    }
    files
}

/// PRD 0050 RF3.3: drop the protected ids of each pack and report them (the pack itself still loads).
fn strip_locales(files: Vec<UserFile>, errors: &mut Vec<UserError>) -> Vec<UserFile> {
    files
        .into_iter()
        .map(
            |file| match cuotai_core::user_pack::strip_protected(&file.text) {
                Some((text, removed)) if !removed.is_empty() => {
                    errors.push(UserError {
                        count: Some(removed.len()),
                        ..UserError::new("locale", file.name.clone(), "protected")
                    });
                    UserFile { text, ..file }
                }
                // Not a JSON object: the UI reports it as invalid, as before.
                _ => file,
            },
        )
        .collect()
}

pub fn load() -> Option<UserContent> {
    let base = user_dir()?;
    let mut errors = Vec::new();
    let themes = read_dir(base.join("themes"), "css", "theme", &mut errors);
    let locales = read_dir(base.join("locales"), "json", "locale", &mut errors);
    let locales = strip_locales(locales, &mut errors);
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

    fn read(dir: &std::path::Path, ext: &str) -> (Vec<UserFile>, Vec<UserError>) {
        let mut errors = Vec::new();
        let files = read_dir(dir.to_path_buf(), ext, "theme", &mut errors);
        (files, errors)
    }

    #[test]
    fn loads_user_files_and_flags_oversized_ones() {
        let base = tempfile::tempdir().unwrap();
        let themes = base.path().join("themes");
        fs::create_dir_all(&themes).unwrap();
        fs::write(themes.join("ocean.css"), ":root[data-theme=\"ocean\"]{}").unwrap();
        fs::write(themes.join("ignored.txt"), "x").unwrap();
        fs::write(
            themes.join("huge.css"),
            vec![b'a'; (MAX_FILE_BYTES + 1) as usize],
        )
        .unwrap();
        fs::create_dir_all(themes.join("folder.css")).unwrap();
        let (files, errors) = read(&themes, "css");
        assert_eq!(
            files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["ocean.css"]
        );
        assert!(errors
            .iter()
            .any(|e| e.name == "huge.css" && e.reason == "tooLarge"));
        assert_eq!(errors.len(), 1, "a folder is not a file and not an error");
    }

    /// PRD 0050 RF3.4: 64 eligible files per folder, chosen by name, the
    /// rest reported; other extensions do not count.
    #[test]
    fn the_cap_counts_eligible_files_sorted_and_reports_the_rest() {
        let base = tempfile::tempdir().unwrap();
        for n in 0..70 {
            fs::write(base.path().join(format!("t{n:02}.css")), "x").unwrap();
            fs::write(base.path().join(format!("a{n:02}.txt")), "x").unwrap();
        }
        let (files, errors) = read(base.path(), "css");
        assert_eq!(files.len(), MAX_FILES);
        assert_eq!(files[0].name, "t00.css");
        assert_eq!(files[63].name, "t63.css");
        let excess: Vec<&str> = errors
            .iter()
            .filter(|e| e.reason == "tooMany")
            .map(|e| e.name.as_str())
            .collect();
        assert_eq!(
            excess,
            ["t64.css", "t65.css", "t66.css", "t67.css", "t68.css", "t69.css"]
        );
    }

    /// PRD 0050 RF3.4: a symbolic link is refused, never followed.
    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_is_refused() {
        let base = tempfile::tempdir().unwrap();
        let outside = base.path().join("outside.css");
        fs::write(&outside, ":root{}").unwrap();
        let themes = base.path().join("themes");
        fs::create_dir_all(&themes).unwrap();
        std::os::unix::fs::symlink(&outside, themes.join("linked.css")).unwrap();
        let (files, errors) = read(&themes, "css");
        assert!(files.is_empty());
        assert_eq!(errors[0].name, "linked.css");
        assert_eq!(errors[0].reason, "symlink");
    }

    /// PRD 0050 RF3.3: a pack keeps its ordinary texts, loses its
    /// protected ones before the webview, and the loss is reported.
    // security-invariant: SEC-USER-PACK
    #[test]
    fn a_language_pack_never_reaches_the_webview_with_protected_ids() {
        let files = vec![
            UserFile {
                name: "fr.json".into(),
                text: r#"{"_meta":{"name":"Français"},"tray.ok":"OK","serve.allow_browsers.warning":"Sans risque","dialog.delete.title":"Supprimer"}"#.into(),
            },
            UserFile {
                name: "de.json".into(),
                text: r#"{"_meta":{"name":"Deutsch"},"tray.ok":"OK"}"#.into(),
            },
        ];
        let mut errors = Vec::new();
        let files = strip_locales(files, &mut errors);
        assert!(!files[0].text.contains("serve.allow_browsers"));
        assert!(!files[0].text.contains("dialog.delete"));
        assert!(files[0].text.contains("tray.ok"));
        assert_eq!(
            files[1].text,
            r#"{"_meta":{"name":"Deutsch"},"tray.ok":"OK"}"#
        );
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].reason, "protected");
        assert_eq!(errors[0].count, Some(2));
    }
}
