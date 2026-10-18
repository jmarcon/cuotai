//! "Sobre": dados do projeto e das bibliotecas de código aberto (lista do core) e abertura de
//! repositórios no navegador padrão.

use cuotai_core::about::{self, Library};
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

/// Só endereços que o próprio Cuotai publica (o projeto e a lista do core) podem ser abertos:
/// a UI nunca escolhe o destino, então um texto qualquer vindo dela não vira um comando.
pub fn is_listed(url: &str) -> bool {
    url == about::PROJECT_REPOSITORY || about::libraries().iter().any(|l| l.repository == url)
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
