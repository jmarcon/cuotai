//! User themes and languages: reads `<config folder>/cuotai/{themes,locales}` and hands them to the UI, which
//! registers them. Read-only; no file is executed.
//!
//! PRD 0050 RF3.1/RF3.2: a theme is parsed by `cuotai_core::user_theme` here; only its canonical CSS, rebuilt
//! from tokens, reaches the webview, and a refused file is reported with the parser's reason.
//!
//! PRD 0050 RF3.3/RF3.4: each folder lists at most [`MAX_FILES`] eligible regular files (symbolic links refused),
//! sorted by name before the cap so the same files always win, and the excess is reported by name. A language
//! pack loses its protected ids (`cuotai_core::user_pack`) here, before the webview sees it; the built-in and
//! core texts keep those words.
//!
//! PRD 0035 re-validation S-02: a language pack is read fail-closed by `cuotai_core::user_pack::load`. Only its
//! typed, filtered messages reach the webview (never the file's text, so the webview never parses it again), and
//! a file the core refuses is reported with the core's reason and sends no message at all.
use cuotai_core::text::Message;
use serde::Serialize;
use std::{collections::BTreeMap, fs, path::PathBuf};

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_FILES: usize = 64;

/// A file read from a user folder; its text stays in the shell.
pub struct UserFile {
    name: String,
    text: String,
}

/// An accepted user theme: its registry entry and the canonical CSS (`text`).
#[derive(Serialize)]
pub struct UserTheme {
    name: String,
    id: String,
    label: String,
    group: String,
    modes: Vec<&'static str>,
    text: String,
}

/// An accepted language pack, as the core parsed and filtered it. There is no `text` field: the webview receives
/// the messages, never the file.
#[derive(Serialize)]
pub struct UserLocale {
    name: String,
    code: String,
    label: String,
    flag: Option<String>,
    digest: String,
    messages: BTreeMap<String, Message>,
    ignored: Vec<String>,
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
    themes: Vec<UserTheme>,
    locales: Vec<UserLocale>,
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

/// PRD 0050 RF3.1/RF3.2: parse each theme in the core; a refused file never reaches the webview.
fn parse_themes(files: Vec<UserFile>, errors: &mut Vec<UserError>) -> Vec<UserTheme> {
    files
        .into_iter()
        .filter_map(|file| {
            let id = file.name.strip_suffix(".css").unwrap_or(&file.name);
            match cuotai_core::user_theme::parse(id, &file.text) {
                Ok(theme) => Some(UserTheme {
                    name: file.name,
                    id: theme.id,
                    label: theme.label,
                    group: theme.group,
                    modes: theme.modes,
                    text: theme.css,
                }),
                Err(error) => {
                    errors.push(UserError::new("theme", file.name, error.reason()));
                    None
                }
            }
        })
        .collect()
}

/// PRD 0050 RF3.3 and PRD 0035 re-validation S-02: load each pack in the core. A refused pack is reported with
/// the core's reason and sends nothing; an accepted one without its protected ids, whose count is reported (the
/// pack itself still loads).
fn parse_locales(files: Vec<UserFile>, errors: &mut Vec<UserError>) -> Vec<UserLocale> {
    files
        .into_iter()
        .filter_map(
            |file| match cuotai_core::user_pack::load(&file.name, &file.text) {
                Ok(pack) => {
                    if !pack.ignored.is_empty() {
                        errors.push(UserError {
                            count: Some(pack.ignored.len()),
                            ..UserError::new("locale", file.name.clone(), "protected")
                        });
                    }
                    Some(UserLocale {
                        name: file.name,
                        code: pack.code,
                        label: pack.label,
                        flag: pack.flag,
                        digest: pack.digest,
                        messages: pack.messages,
                        ignored: pack.ignored,
                    })
                }
                Err(refusal) => {
                    errors.push(UserError::new("locale", file.name, refusal.reason()));
                    None
                }
            },
        )
        .collect()
}

pub fn load() -> Option<UserContent> {
    let base = user_dir()?;
    let mut errors = Vec::new();
    let themes = read_dir(base.join("themes"), "css", "theme", &mut errors);
    let themes = parse_themes(themes, &mut errors);
    let locales = read_dir(base.join("locales"), "json", "locale", &mut errors);
    let locales = parse_locales(locales, &mut errors);
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

    /// PRD 0050 RF3.1/RF3.2: only parsed, canonical theme CSS reaches the
    /// webview; a refused theme is reported with the parser's reason.
    #[test]
    fn a_theme_reaches_the_webview_only_as_canonical_css() {
        let ok = ":root[data-theme=\"ocean\"][data-mode=\"dark\"] { --bg: #000; /* note */ }\n\
                  :root[data-theme=\"ocean\"][data-mode=\"light\"] { --bg: #FFF; }";
        let files = vec![
            UserFile {
                name: "ocean.css".into(),
                text: ok.into(),
            },
            UserFile {
                name: "pixel.css".into(),
                text: ok
                    .replace("ocean", "pixel")
                    .replace("#000", "url(https://example.com/pixel)"),
            },
        ];
        let mut errors = Vec::new();
        let themes = parse_themes(files, &mut errors);
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].id, "ocean");
        assert!(!themes[0].text.contains("note"));
        assert!(themes[0].text.contains("--bg: #fff;"));
        assert_eq!(errors.len(), 1);
        assert_eq!(
            (errors[0].name.as_str(), errors[0].reason),
            ("pixel.css", "externalUrl")
        );
    }

    /// PRD 0050 RF3.3: a pack keeps its ordinary texts, loses its
    /// protected ones before the webview, and the loss is reported.
    // security-invariant: SEC-USER-PACK
    #[test]
    fn a_language_pack_never_reaches_the_webview_with_protected_ids() {
        let files = vec![
            UserFile {
                name: "fr.json".into(),
                text: r#"{"_meta":{"name":"Français"},"tray.ok":"OK","serve.allow_browsers.warning":"Sans risque","dialog.delete":"Supprimer"}"#.into(),
            },
            UserFile {
                name: "de.json".into(),
                text: r#"{"_meta":{"name":"Deutsch","flag":"AT"},"tray.ok":"OK"}"#.into(),
            },
        ];
        let mut errors = Vec::new();
        let locales = parse_locales(files, &mut errors);
        assert_eq!(locales.len(), 2);
        assert_eq!(locales[0].messages.keys().collect::<Vec<_>>(), ["tray.ok"]);
        assert_eq!(
            locales[0].ignored,
            ["dialog.delete", "serve.allow_browsers.warning"]
        );
        assert_eq!(
            (locales[1].code.as_str(), locales[1].label.as_str()),
            ("de", "Deutsch")
        );
        assert_eq!(locales[1].flag.as_deref(), Some("AT"));
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].name, "fr.json");
        assert_eq!(errors[0].reason, "protected");
        assert_eq!(errors[0].count, Some(2));
    }

    /// PRD 0035 re-validation S-02: a pack the core cannot read safely is
    /// refused whole with the core's reason, and what the webview receives
    /// for a locale never carries the file's text.
    // security-invariant: SEC-USER-PACK
    #[test]
    fn a_refused_pack_sends_no_message_and_no_locale_carries_text() {
        let deep = format!("{}1{}", "[".repeat(200), "]".repeat(200));
        let files = vec![
            UserFile {
                name: "ja.json".into(),
                text: r#"{"_meta":{"name":"日本語","n":1e400},"tray.ok":"x","serve.banner":"x"}"#
                    .into(),
            },
            UserFile {
                name: "ko.json".into(),
                text: format!(r#"{{"_meta":{{"name":"한국어","p":{deep}}},"tray.ok":"x"}}"#),
            },
            UserFile {
                name: "de.json".into(),
                text: r#"{"_meta":{"name":"Deutsch\ud800"},"tray.ok":"x"}"#.into(),
            },
            UserFile {
                name: "en-US-US.json".into(),
                text: r#"{"_meta":{"name":"X"},"tray.ok":"x"}"#.into(),
            },
            UserFile {
                name: "fr.json".into(),
                text: r#"{"_meta":{"name":"Français"},"tray.ok":"Tout va bien"}"#.into(),
            },
        ];
        let mut errors = Vec::new();
        let locales = parse_locales(files, &mut errors);
        assert_eq!(
            errors
                .iter()
                .map(|e| (e.kind, e.name.as_str(), e.reason))
                .collect::<Vec<_>>(),
            [
                ("locale", "ja.json", "invalidJson"),
                ("locale", "ko.json", "invalidMeta"),
                ("locale", "de.json", "invalidJson"),
                ("locale", "en-US-US.json", "invalidCode"),
            ]
        );
        assert_eq!(locales.len(), 1);
        let content = UserContent {
            dir: String::new(),
            themes: vec![],
            locales,
            errors,
        };
        let sent = serde_json::to_value(&content).unwrap();
        let locale = &sent["locales"][0];
        assert!(locale.get("text").is_none());
        assert_eq!(locale["name"], "fr.json");
        assert_eq!(locale["code"], "fr");
        assert_eq!(locale["label"], "Français");
        assert_eq!(locale["messages"]["tray.ok"], "Tout va bien");
        assert_eq!(locale["ignored"], serde_json::json!([]));
        assert!(locale["flag"].is_null());
        assert_eq!(locale["digest"].as_str().map(str::len), Some(64));
        for locale in sent["locales"].as_array().unwrap() {
            assert!(locale.get("text").is_none());
        }
    }
}
