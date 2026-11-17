//! "About": project and open-source library data (core list) and opening
//! repositories in the default browser.

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
    /// PRD 0039 RF10: `dev abc1234` for a build between releases (display-only).
    pub build: Option<String>,
    pub license: &'static str,
    pub repository: &'static str,
    pub libraries: &'static [Library],
}

pub fn view() -> AboutView {
    AboutView {
        name: about::PROJECT_NAME,
        version: env!("CARGO_PKG_VERSION"),
        build: cuotai_core::build_info::build_suffix(),
        license: about::PROJECT_LICENSE,
        repository: about::PROJECT_REPOSITORY,
        libraries: about::libraries(),
    }
}

/// PRD 0019 RF7.7 "What's new": the section of the running version from the core's parsed
/// changelog, projected for the window, plus the address of the full changelog. The core owns
/// parsing and the category ids; this only picks the one section and shapes it for rendering
/// (labels stay `Localized`, so the frontend resolves them with the core catalog like every other
/// core text). The release page and the updater's notes keep every section since the last
/// published release (PRD 0019); only this dialog shows one version.
#[derive(Debug, Serialize)]
pub struct WhatsNewView {
    pub current: String,
    pub title: Localized,
    pub entries_language: Localized,
    /// The running version's section, or the newest one when the running version has none
    /// (a build between releases); `None` only for an empty changelog.
    pub release: Option<ReleaseView>,
    /// The full changelog on GitHub (`CHANGELOG_URL`, allow-listed by `is_listed`).
    pub full_changelog: &'static str,
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

fn release_view(release: &Release, running: &str) -> ReleaseView {
    ReleaseView {
        version: release.version.clone(),
        date: release.date.clone(),
        current: release.version == running,
        notes: notes(&release.section),
    }
}

/// The What's new view of `changelog` for the `running` version: only that version's section
/// (or the newest one when it has none), never the whole history.
pub fn whats_new_for(changelog: &Changelog, running: &str) -> WhatsNewView {
    let release = changelog
        .find(running)
        .or_else(|| changelog.releases.first())
        .map(|release| release_view(release, running));
    WhatsNewView {
        current: running.to_owned(),
        title: changelog_title_l10n(),
        entries_language: changelog_entries_language_l10n(),
        release,
        full_changelog: CHANGELOG_URL,
    }
}

pub fn whats_new() -> WhatsNewView {
    whats_new_for(changelog::entries(), env!("CARGO_PKG_VERSION"))
}

/// PRD 0019 RF7.7: the full changelog the What's new dialog links (`CHANGELOG_URL` in
/// `js/lib.js`). The branch file, not a release page: a tag may never be published.
pub const CHANGELOG_URL: &str = "https://github.com/jmarcon/cuotai/blob/main/CHANGELOG.md";

/// Only addresses that Cuotai itself publishes (the project, the releases page the
/// updater offers when something fails — PRD 0018 RF2.3/RF2.5 — and the core list) or,
/// PRD 0011 RF4.1, the public status pages of the core's verified sources may be
/// opened: the UI never chooses the destination, so arbitrary text coming from it never becomes a
/// command.
pub fn is_listed(url: &str) -> bool {
    url == about::PROJECT_REPOSITORY
        || url == NEW_PROVIDER_ISSUE
        || url == EXTENSIONS_GUIDE
        || url == TRANSLATION_GUIDE
        || url == BACKUP_GUIDE
        || url == INSTALL_GUIDE
        || url == CHANGELOG_URL
        || url == crate::updater::MANUAL_URL
        || about::libraries().iter().any(|l| l.repository == url)
        || cuotai_core::incidents::is_status_link(url)
        || cuotai_core::support::is_bare_form_url(url)
        || cuotai_core::user_session::is_open_url(url)
        || cuotai_core::secret_input::is_key_page_url(url)
        || cuotai_core::oauth::bailian::is_install_url(url)
        || cuotai_core::access::is_evidence_url(url)
}

/// PRD 0027 RF3.5: the new-provider issue template, offered by the picker's
/// empty state ("Request a provider"); `NEW_PROVIDER_URL` in `js/lib.js`.
pub const NEW_PROVIDER_ISSUE: &str =
    "https://github.com/jmarcon/cuotai/issues/new?template=new-provider.yml";

/// PRD 0017 RF5: the guide the Extensions dialog links ("How extensions work");
/// `EXTENSIONS_GUIDE_URL` in `js/lib.js`.
pub const EXTENSIONS_GUIDE: &str = "https://github.com/jmarcon/cuotai/blob/main/docs/extensions.md";

/// PRD 0046 RF5.2: the "Move to a new computer" and "Keep two computers
/// aligned" recipes the Files page links (`BACKUP_GUIDE_URL` in `js/lib.js`).
pub const BACKUP_GUIDE: &str =
    "https://github.com/jmarcon/cuotai/blob/main/docs/advanced.md#backup-restore-and-self-check";

/// PRD 0040 re-validation V-07: how to install the terminal version, which the
/// Claude Code snippet runs (`INSTALL_GUIDE_URL` in `js/settings/dashboard.js`).
pub const INSTALL_GUIDE: &str = "https://github.com/jmarcon/cuotai/blob/main/docs/installing.md";

/// PRD 0035 RF11.5: the translation guide the machine-translation notice in
/// Settings › General links (`TRANSLATION_GUIDE_URL` in `js/lib.js`).
pub const TRANSLATION_GUIDE: &str =
    "https://github.com/jmarcon/cuotai/blob/main/apps/desktop/ui/locales/README.md";

/// PRD 0017 RF5: opens the folder discovery reads extensions from (created on
/// demand) in the system file manager. The path is the core's, never the UI's.
pub fn open_extensions_folder() -> Result<(), &'static str> {
    let dir = cuotai_core::extensions::default_dir().ok_or("unavailable")?;
    std::fs::create_dir_all(&dir).map_err(|_| "unavailable")?;
    launch(&dir.to_string_lossy())
}

/// PRD 0028 RF2.3: opens a problem-report link the shell built itself
/// (`open_problem_report`): only this project's issue form, prefilled by
/// the core's draft, never a URL from the webview.
pub fn open_problem_link(url: &str) -> Result<(), &'static str> {
    let form = format!("{}/issues/new?", cuotai_core::support::REPOSITORY);
    if !url.starts_with(&form) || url.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err("not_listed");
    }
    launch(url)
}

/// Opens `url` in the system default browser, if it is on the list.
pub fn open_external(url: &str) -> Result<(), &'static str> {
    if !is_listed(url) {
        return Err("not_listed");
    }
    launch(url)
}

/// PRD 0036 RF8.2: open the usage page of the user's own aggregator
/// instance. The address comes from the core (`aggregators::dashboard_url`,
/// built from the validated base address), never from the interface.
pub fn open_dashboard(url: &str) -> Result<(), &'static str> {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("not_listed");
    }
    launch(url)
}

fn launch(url: &str) -> Result<(), &'static str> {
    #[cfg(target_os = "macos")]
    let mut cmd = std::process::Command::new("open");
    #[cfg(target_os = "linux")]
    let mut cmd = std::process::Command::new("xdg-open");
    #[cfg(target_os = "windows")]
    let mut cmd = {
        // `explorer` opens the URL in the default browser without going through the command interpreter.
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
        assert!(is_listed(NEW_PROVIDER_ISSUE));
        assert!(is_listed(EXTENSIONS_GUIDE));
        assert!(is_listed(TRANSLATION_GUIDE));
        assert!(is_listed(BACKUP_GUIDE));
        assert!(is_listed(CHANGELOG_URL));
        assert!(is_listed(
            "https://platform.xiaomimimo.com/console/plan-manage"
        ));
        for library in about::libraries() {
            assert!(is_listed(library.repository), "{}", library.name);
        }
        // PRD 0005 RF4.2 (DS-MSG-9): the reference contract the account
        // details link; any other address on the same host stays closed.
        let reference = cuotai_core::access::describe(&cuotai_core::Account {
            name: "zed".into(),
            provider: cuotai_core::Provider::Zed,
            ..Default::default()
        })
        .evidence
        .url();
        assert!(is_listed(reference), "{reference}");
        for url in [
            "https://github.com/decolua/9router/blob/main/README.md",
            "https://github.com/decolua/9router/issues/new",
            &format!("{reference}/x"),
        ] {
            assert!(!is_listed(url), "{url}");
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

    /// Owner request: the `bl` install page the core declares opens; a look-alike does not.
    #[test]
    fn bl_install_page_opens_and_look_alikes_do_not() {
        assert!(is_listed(cuotai_core::oauth::bailian::CLI_INSTALL_URL));
        for url in [
            "https://www.alibabacloud.com/help/en/model-studio/cli/installation/../x",
            "http://www.alibabacloud.com/help/en/model-studio/cli/installation",
            "https://www.alibabacloud.com/help/en/model-studio/cli/installationx",
        ] {
            assert!(!is_listed(url), "{url}");
        }
    }

    /// Owner request: the account dialog opens the key-creation page the
    /// core declares for each typed key, and nothing that only looks like one.
    #[test]
    fn key_creation_pages_open_and_look_alikes_do_not() {
        let pages: Vec<_> = cuotai_core::Provider::ALL
            .iter()
            .flat_map(|p| cuotai_core::secret_input::secret_inputs(*p))
            .filter_map(|input| input.key_page)
            .collect();
        assert!(!pages.is_empty());
        for url in &pages {
            assert!(is_listed(url), "{url}");
        }
        for url in [
            "https://console.groq.com/keys/../evil",
            "https://console.groq.com/keysx",
            "http://console.groq.com/keys",
            "https://console.groq.com.example.com/keys",
            "https://platform.claude.com/settings/admin-keys?next=https://example.com",
        ] {
            assert_eq!(open_external(url), Err("not_listed"), "{url}");
        }
    }

    /// PRD 0028 re-validation (S-01): the webview may open only the two bare
    /// issue forms; a prefilled URL with any text it chose is refused, and
    /// the shell's own opener takes only this project's issue form.
    #[test]
    fn prefilled_issue_urls_from_the_webview_are_refused() {
        assert!(is_listed(cuotai_core::support::FORM_URL));
        assert!(is_listed(cuotai_core::support::BUG_FORM_URL));
        for url in [
            "https://github.com/jmarcon/cuotai/issues/new?template=provider-problem.yml&title=x&body=run+this",
            "https://github.com/jmarcon/cuotai/issues/new?title=Phish",
        ] {
            assert_eq!(open_external(url), Err("not_listed"), "{url}");
        }
        for url in [
            "https://github.com/other/repo/issues/new?x=1",
            "https://github.com/jmarcon/cuotai/pulls",
            "https://github.com/jmarcon/cuotai/issues/new?x=1 y",
            "javascript:alert(1)",
        ] {
            assert_eq!(open_problem_link(url), Err("not_listed"), "{url}");
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
    fn whats_new_shows_only_the_running_version() {
        let core = changelog::parse(
            "# Changelog\n\n## [Unreleased]\n\n## [0.3.0] - 2026-11-03\n\n### Added\n\n- Third.\n- Third again.\n\n\
             ## [0.2.0] - 2026-11-02\n\n### Fixed\n\n- Second.\n\n## [0.1.0] - 2026-11-01\n\n### Added\n\n- First.\n",
        )
        .unwrap();
        let view = whats_new_for(&core, "0.2.0");
        let release = view.release.as_ref().unwrap();
        assert_eq!(release.version, "0.2.0");
        assert!(release.current);
        assert_eq!(release.notes.categories.len(), 1);
        assert_eq!(release.notes.categories[0].entries, ["Second."]);
        assert_eq!(view.full_changelog, CHANGELOG_URL);
        let json = serde_json::to_value(&view).unwrap();
        assert!(json.get("releases").is_none() && json.get("history").is_none());
        assert!(json["release"]["categories"][0]["label"]["id"]
            .as_str()
            .unwrap()
            .starts_with("changelog.category."));
        // A build between releases (no section of its own) shows the newest section, not marked current.
        let between = whats_new_for(&core, "0.3.1");
        let newest = between.release.unwrap();
        assert_eq!(newest.version, "0.3.0");
        assert!(!newest.current);
        assert_eq!(newest.notes.categories[0].entries.len(), 2);
    }

    #[test]
    fn whats_new_mirrors_the_embedded_changelog() {
        let core = changelog::entries();
        let view = whats_new();
        let release = view.release.expect("the embedded changelog has a release");
        let source = core
            .find(env!("CARGO_PKG_VERSION"))
            .unwrap_or(&core.releases[0]);
        assert_eq!(release.version, source.version);
        assert_eq!(release.date, source.date);
        let shown: usize = release
            .notes
            .categories
            .iter()
            .map(|c| c.entries.len())
            .sum();
        let expected: usize = source
            .section
            .categories
            .iter()
            .map(|b| b.entries.len())
            .sum();
        assert_eq!(shown, expected);
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
