//! "Sobre": dados do projeto e das bibliotecas de código aberto (lista do core) e abertura de
//! repositórios no navegador padrão.

use cuotai_core::about::{self, Library};
use cuotai_core::changelog::{self, Changelog, Release, Section};
use cuotai_core::text::{
    changelog_category_l10n, changelog_entries_language_l10n, changelog_title_l10n, Localized,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct AboutView {
    pub name: &'static str,
    pub version: &'static str,
    pub license: &'static str,
    pub repository: &'static str,
    pub libraries: &'static [Library],
}

pub fn view() -> AboutView {
    AboutView {
        name: about::PROJECT_NAME,
        version: env!("CARGO_PKG_VERSION"),
        license: about::PROJECT_LICENSE,
        repository: about::PROJECT_REPOSITORY,
        libraries: about::libraries(),
    }
}

/// PRD 0019 RF7.7 "What's new": the core's parsed changelog, projected for the window. The
/// core owns parsing and the category ids; this only shapes them for rendering (labels stay
/// `Localized`, so the frontend resolves them with the core catalog like every other core text).
#[derive(Debug, Serialize)]
pub struct WhatsNewView {
    pub current: &'static str,
    pub title: Localized,
    pub entries_language: Localized,
    pub releases: Vec<ReleaseView>,
    /// The internal builds (never published, PRD 0039 D11), shown last when the changelog has them.
    pub history: Option<HistoryView>,
}

#[derive(Debug, Serialize)]
pub struct ReleaseView {
    pub version: String,
    pub date: String,
    /// Marks the section of the running version.
    pub current: bool,
    #[serde(flatten)]
    pub notes: NotesView,
}

#[derive(Debug, Serialize)]
pub struct HistoryView {
    pub title: String,
    #[serde(flatten)]
    pub notes: NotesView,
}

#[derive(Debug, Serialize)]
pub struct NotesView {
    pub paragraphs: Vec<String>,
    pub categories: Vec<CategoryView>,
}

#[derive(Debug, Serialize)]
pub struct CategoryView {
    pub label: Localized,
    pub entries: Vec<String>,
}

fn notes(section: &Section) -> NotesView {
    NotesView {
        paragraphs: section.paragraphs.clone(),
        categories: section
            .categories
            .iter()
            .map(|block| CategoryView {
                label: changelog_category_l10n(block.category),
                entries: block
                    .entries
                    .iter()
                    .map(|entry| entry.text.clone())
                    .collect(),
            })
            .collect(),
    }
}

fn release_view(release: &Release) -> ReleaseView {
    ReleaseView {
        version: release.version.clone(),
        date: release.date.clone(),
        current: release.version == env!("CARGO_PKG_VERSION"),
        notes: notes(&release.section),
    }
}

/// The What's new view of `changelog` (the embedded one in production).
pub fn whats_new_from(changelog: &Changelog) -> WhatsNewView {
    WhatsNewView {
        current: env!("CARGO_PKG_VERSION"),
        title: changelog_title_l10n(),
        entries_language: changelog_entries_language_l10n(),
        releases: changelog.releases.iter().map(release_view).collect(),
        history: changelog.pre_release.as_ref().map(|history| HistoryView {
            title: history.title.clone(),
            notes: notes(&history.section),
        }),
    }
}

pub fn whats_new() -> WhatsNewView {
    whats_new_from(changelog::entries())
}

/// Só endereços que o próprio Cuotai publica (o projeto, a página de lançamentos que o
/// atualizador oferece quando algo falha — PRD 0018 RF2.3/RF2.5 — e a lista do core) ou,
/// PRD 0011 RF4.1, as páginas públicas de status das fontes verificadas do core podem ser
/// abertos: a UI nunca escolhe o destino, então um texto qualquer vindo dela não vira um
/// comando.
pub fn is_listed(url: &str) -> bool {
    url == about::PROJECT_REPOSITORY
        || url == crate::updater::MANUAL_URL
        || about::libraries().iter().any(|l| l.repository == url)
        || cuotai_core::incidents::is_status_link(url)
}

/// Abre `url` no navegador padrão do sistema, se estiver na lista.
pub fn open_external(url: &str) -> Result<(), &'static str> {
    if !is_listed(url) {
        return Err("not_listed");
    }
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "linux")]
    let mut cmd = std::process::Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        // `explorer` abre a URL no navegador padrão sem passar pelo interpretador de comandos.
        std::process::Command::new("explorer")
    };
    cmd.arg(url).spawn().map(|_| ()).map_err(|_| "unavailable")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_published_addresses_open() {
        assert!(is_listed(about::PROJECT_REPOSITORY));
        for library in about::libraries() {
            assert!(is_listed(library.repository), "{}", library.name);
        }
        for url in [
            "",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "https://example.com",
            "https://github.com/tauri-apps/tauri; rm -rf /",
            "https://github.com/tauri-apps/tauri/",
        ] {
            assert!(!is_listed(url), "{url}");
            assert_eq!(open_external(url), Err("not_listed"));
        }
    }

    #[test]
    fn status_pages_of_verified_sources_open_and_look_alikes_do_not() {
        assert!(is_listed("https://status.claude.com/"));
        assert!(is_listed("https://status.claude.com/incidents/abc123xyz"));
        for url in [
            "https://status.claude.com.example.com/",
            "http://status.claude.com/",
            "https://status.claude.com/?next=https://example.com",
        ] {
            assert_eq!(open_external(url), Err("not_listed"), "{url}");
        }
    }

    #[test]
    fn whats_new_mirrors_the_core_changelog() {
        let core = changelog::entries();
        let view = whats_new();
        assert_eq!(view.releases.len(), core.releases.len());
        assert_eq!(
            view.releases.iter().filter(|r| r.current).count(),
            usize::from(core.knows(env!("CARGO_PKG_VERSION")))
        );
        for (shown, source) in view.releases.iter().zip(&core.releases) {
            assert_eq!(shown.version, source.version);
            assert_eq!(shown.date, source.date);
            let shown_entries: usize = shown.notes.categories.iter().map(|c| c.entries.len()).sum();
            let source_entries: usize = source
                .section
                .categories
                .iter()
                .map(|b| b.entries.len())
                .sum();
            assert_eq!(shown_entries, source_entries, "{}", source.version);
        }
        assert_eq!(view.history.is_some(), core.pre_release.is_some());
        let json = serde_json::to_value(&view).unwrap();
        let first = &json["releases"][0];
        assert!(first["categories"][0]["label"]["id"]
            .as_str()
            .unwrap()
            .starts_with("changelog.category."));
        assert!(first.get("current").is_some());
        // Category labels stay the core's ids, one per fixed category.
        assert!(changelog::Category::ALL
            .iter()
            .all(|c| c.l10n_id().starts_with("changelog.category.")));
    }

    #[test]
    fn view_exposes_core_list_without_gaps() {
        let view = view();
        assert_eq!(view.libraries.len(), about::libraries().len());
        let json = serde_json::to_value(&view).unwrap();
        let first = &json["libraries"][0];
        for key in ["name", "license", "repository", "role", "used_by"] {
            assert!(first.get(key).is_some(), "{key}");
        }
        assert!(json["libraries"]
            .as_array()
            .unwrap()
            .iter()
            .any(|l| l["role"] == "reference" && l["used_by"][0] == "core"));
    }
}
