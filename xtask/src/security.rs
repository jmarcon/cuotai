//! `cargo xtask security-baseline` (PRD 0050 RF1.4, RF7.1, RF7.3, RF7.5):
//! the desktop webview policy, the HTML pages it loads, and the registry of
//! security invariants in `docs/security-baseline.md`, each tied to a test.
//!
//! Every check is a pure function over text so the fixture tests can feed it
//! a broken policy, page or registry; [`run`] only reads the repository.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::{err, Res};

/// The exact desktop policy (RF1.1). `connect-src 'self'` stays because the
/// UI reads its bundled catalogs (`locales/*.json`) with `fetch` from the
/// app's own origin; it reaches nothing outside the bundle.
pub const EXPECTED_CSP: &[(&str, &[&str])] = &[
    ("default-src", &["'self'"]),
    ("script-src", &["'self'"]),
    ("style-src", &["'self'", "'unsafe-inline'"]),
    ("img-src", &["'self'"]),
    ("font-src", &["'self'"]),
    ("connect-src", &["'self'", "ipc:", "http://ipc.localhost"]),
    ("object-src", &["'none'"]),
    ("base-uri", &["'none'"]),
    ("form-action", &["'none'"]),
    ("frame-src", &["'none'"]),
    ("frame-ancestors", &["'none'"]),
];

/// The platforms a registry row may declare.
const PLATFORMS: &[&str] = &["Linux", "macOS", "Windows"];

/// The source marker that ties a test to an invariant id.
const MARKER: &str = "security-invariant:";

/// Problems of `tauri.conf.json` (RF1.1/RF1.3/RF1.4).
pub fn check_tauri_config(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let config: serde_json::Value = match serde_json::from_str(text) {
        Ok(config) => config,
        Err(e) => return vec![format!("tauri.conf.json is not JSON: {e}")],
    };
    let security = &config["app"]["security"];
    let Some(csp) = security["csp"].as_str() else {
        return vec!["app.security.csp must be a string, not null or missing".into()];
    };
    if security
        .get("dangerousDisableAssetCspModification")
        .is_some()
    {
        problems.push("dangerousDisableAssetCspModification must stay unset".into());
    }
    if security.get("devCsp").is_some() {
        problems.push("devCsp must stay unset: development runs the release policy".into());
    }
    problems.extend(check_csp(csp));
    problems
}

/// Problems of one CSP string against [`EXPECTED_CSP`] and the forbidden
/// sources (no `*`, `unsafe-eval`, `https:`, `data:`, `blob:`, remote URL;
/// `http:` only as the exact IPC source, both IPC sources only in
/// `connect-src`; `'unsafe-inline'` only in `style-src`).
pub fn check_csp(csp: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut directives: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for directive in csp.split(';').map(str::trim).filter(|d| !d.is_empty()) {
        let mut parts = directive.split_whitespace();
        let name = parts.next().unwrap_or_default();
        if directives.insert(name, parts.collect()).is_some() {
            problems.push(format!("CSP directive {name} appears twice"));
        }
    }
    for (name, sources) in &directives {
        for source in sources {
            let lower = source.to_ascii_lowercase();
            let ipc = lower == "ipc:" || lower == "http://ipc.localhost";
            let bad = if lower.contains('*') {
                Some("a wildcard")
            } else if lower == "'unsafe-eval'" || lower == "'wasm-unsafe-eval'" {
                Some("eval")
            } else if lower == "'unsafe-inline'" && *name != "style-src" {
                Some("inline code")
            } else if lower.starts_with("data:") || lower.starts_with("blob:") {
                Some("data or blob URLs")
            } else if lower.starts_with("https:")
                || lower.starts_with("http:") && !ipc
                || lower.starts_with("ws:")
                || lower.starts_with("wss:")
                || lower.contains("://") && !ipc
            {
                Some("a remote source")
            } else if ipc && *name != "connect-src" {
                Some("the IPC sources outside connect-src")
            } else {
                None
            };
            if let Some(what) = bad {
                problems.push(format!("CSP {name} must not allow {what}: {source}"));
            }
        }
    }
    for (name, want) in EXPECTED_CSP {
        match directives.get(name) {
            None => problems.push(format!("CSP directive {name} is missing")),
            Some(have) if have.as_slice() != *want => problems.push(format!(
                "CSP {name} must be exactly {:?}, found {have:?}",
                want.join(" ")
            )),
            Some(_) => {}
        }
    }
    for name in directives.keys() {
        if !EXPECTED_CSP.iter().any(|(expected, _)| expected == name) {
            problems.push(format!("CSP directive {name} is not part of the baseline"));
        }
    }
    problems
}

/// Problems of one desktop HTML page (RF1.2/RF1.4): inline scripts, inline
/// event handlers, `javascript:` URLs, remote resources and frames.
pub fn check_html(name: &str, text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        if rest.starts_with("!--") {
            rest = rest.find("-->").map_or("", |end| &rest[end + 3..]);
            continue;
        }
        let Some(end) = rest.find('>') else { break };
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        let lower = tag.to_ascii_lowercase();
        let tag_name = lower
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or_default();
        let attrs = attributes(&lower);
        match tag_name {
            "script" => {
                match attrs.iter().find(|(key, _)| key == "src") {
                    None => problems.push(format!("{name}: inline <script>")),
                    Some((_, src)) if remote(src) => {
                        problems.push(format!("{name}: remote script {src}"))
                    }
                    Some(_) => {}
                }
                // An external script with a body still runs nothing, but the
                // body would be a hidden inline script for a lax parser.
                let body_end = rest.to_ascii_lowercase().find("</script").unwrap_or(0);
                if !rest[..body_end].trim().is_empty() {
                    problems.push(format!("{name}: <script> with a body"));
                }
            }
            "iframe" | "frame" | "object" | "embed" | "base" => {
                problems.push(format!("{name}: <{tag_name}> is not allowed"))
            }
            "meta" if attrs.iter().any(|(key, _)| key == "http-equiv") => {
                problems.push(format!("{name}: <meta http-equiv> overrides the policy"))
            }
            _ => {}
        }
        for (key, value) in &attrs {
            if key.starts_with("on") && key.len() > 2 {
                problems.push(format!("{name}: inline event handler {key}"));
            }
            if matches!(
                key.as_str(),
                "src" | "href" | "action" | "srcset" | "poster"
            ) {
                if value.trim_start().starts_with("javascript:") {
                    problems.push(format!("{name}: javascript: URL in {key}"));
                } else if remote(value) {
                    problems.push(format!("{name}: remote {key} {value}"));
                }
            }
        }
    }
    problems
}

/// Whether a URL leaves the bundle.
fn remote(url: &str) -> bool {
    let url = url.trim().to_ascii_lowercase();
    url.starts_with("//") || url.contains("://") || url.starts_with("data:")
}

/// The lowercase `(name, value)` attributes of one tag body.
fn attributes(tag: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars: Vec<char> = tag.chars().collect();
    let mut i = chars
        .iter()
        .position(|c| c.is_whitespace())
        .unwrap_or(chars.len());
    while i < chars.len() {
        while i < chars.len() && (chars[i].is_whitespace() || chars[i] == '/') {
            i += 1;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '=' && chars[i] != '/' {
            i += 1;
        }
        let key: String = chars[start..i].iter().collect();
        if key.is_empty() {
            i += 1;
            continue;
        }
        let mut value = String::new();
        if i < chars.len() && chars[i] == '=' {
            i += 1;
            if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
                let quote = chars[i];
                i += 1;
                let begin = i;
                while i < chars.len() && chars[i] != quote {
                    i += 1;
                }
                value = chars[begin..i].iter().collect();
                i += 1;
            } else {
                let begin = i;
                while i < chars.len() && !chars[i].is_whitespace() {
                    i += 1;
                }
                value = chars[begin..i].iter().collect();
            }
        }
        out.push((key, value));
    }
    out
}

/// One row of `docs/security-baseline.md`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invariant {
    pub id: String,
    pub owners: Vec<String>,
    pub platforms: Vec<String>,
}

/// Parses the registry table: the rows whose first cell is a `SEC-…` id.
pub fn parse_registry(doc: &str) -> Res<Vec<Invariant>> {
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for line in doc.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.trim_matches('|').split('|').map(str::trim).collect();
        let id = cells[0].trim_matches('`');
        if !id.starts_with("SEC-") {
            continue;
        }
        if cells.len() != 5 {
            return err(format!(
                "{id}: a registry row has 5 cells (id, boundary, fail-closed result, owning test, platforms)"
            ));
        }
        if !seen.insert(id.to_string()) {
            return err(format!("{id} appears twice in the registry"));
        }
        let owners = backticked(cells[3]);
        if owners.is_empty() {
            return err(format!("{id}: no owning test or command"));
        }
        let platforms: Vec<String> = cells[4]
            .split(',')
            .map(|p| p.split('(').next().unwrap_or_default().trim().to_string())
            .filter(|p| !p.is_empty())
            .collect();
        if platforms.is_empty() {
            return err(format!("{id}: missing platform declaration"));
        }
        for platform in &platforms {
            if !PLATFORMS.contains(&platform.as_str()) {
                return err(format!(
                    "{id}: unknown platform {platform:?} (use {})",
                    PLATFORMS.join(", ")
                ));
            }
        }
        rows.push(Invariant {
            id: id.to_string(),
            owners,
            platforms,
        });
    }
    if rows.is_empty() {
        return err("the security baseline registry lists no SEC- invariant");
    }
    Ok(rows)
}

fn backticked(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .filter(|s| !s.is_empty())
        .collect()
}

/// The invariant ids marked in one source file (`security-invariant: SEC-…`).
pub fn markers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (at, _) in text.match_indices(MARKER) {
        let id: String = text[at + MARKER.len()..]
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || *c == '-')
            .collect();
        if id.starts_with("SEC-") && id.len() > 4 {
            out.push(id);
        }
    }
    out
}

/// Cross-checks the registry with the markers found in `sources` (path and
/// text) (RF7.5): each invariant has at least one marker, each marker is
/// documented, and each `file::test` owner names a test that exists.
pub fn check_registry(rows: &[Invariant], sources: &[(String, String)]) -> Vec<String> {
    let mut problems = Vec::new();
    let mut marked: BTreeMap<String, Vec<&str>> = BTreeMap::new();
    for (path, text) in sources {
        for id in markers(text) {
            marked.entry(id).or_default().push(path);
        }
    }
    let documented: BTreeSet<&str> = rows.iter().map(|row| row.id.as_str()).collect();
    for (id, paths) in &marked {
        if !documented.contains(id.as_str()) {
            problems.push(format!(
                "undocumented invariant marker {id} in {}",
                paths.join(", ")
            ));
        }
    }
    for row in rows {
        if !marked.contains_key(&row.id) {
            problems.push(format!("{}: no test carries its marker", row.id));
        }
        for owner in &row.owners {
            // Commands (`cargo xtask …`) are not test names.
            let Some((file, test)) = owner.split_once("::").filter(|_| !owner.contains(' ')) else {
                continue;
            };
            match sources.iter().find(|(path, _)| path == file) {
                None => problems.push(format!("{}: owning file {file} not found", row.id)),
                Some((_, text)) => {
                    let defined = [format!("fn {test}("), format!("fn {test}<")]
                        .iter()
                        .any(|needle| text.contains(needle.as_str()))
                        || text.contains(&format!("\"{test}\""));
                    if !defined {
                        problems.push(format!("{}: stale test name {owner}", row.id));
                    }
                }
            }
        }
    }
    problems
}

/// `.gitignore` keeps the plaintext development store out of git (RF7.3).
pub fn check_gitignore(text: &str) -> Vec<String> {
    if text.lines().any(|line| {
        let line = line.trim();
        line == "cuotai-dev.sqlite3" || line == "/cuotai-dev.sqlite3" || line == "*.sqlite3"
    }) {
        Vec::new()
    } else {
        vec![".gitignore must keep cuotai-dev.sqlite3 ignored".into()]
    }
}

/// The source roots scanned for markers.
const SOURCE_ROOTS: &[&str] = &[
    "crates",
    "apps/desktop/src-tauri/src",
    "apps/desktop/tools",
    "xtask/src",
    "xtask/tests",
    "scripts",
];

fn collect_sources(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) -> Res<()> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name == "target" || name == "node_modules" || name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect_sources(root, &path, out)?;
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rs" | "mjs" | "js" | "sh" | "py")
        ) {
            let text = std::fs::read_to_string(&path)?;
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, text));
        }
    }
    Ok(())
}

/// Runs every check over the repository at `root`; returns the number of
/// invariants on success.
pub fn run(root: &Path) -> Res<usize> {
    let read = |relative: &str| {
        std::fs::read_to_string(root.join(relative))
            .map_err(|e| crate::Error::Msg(format!("cannot read {relative}: {e}")))
    };
    let mut problems = check_tauri_config(&read("apps/desktop/src-tauri/tauri.conf.json")?);
    let ui = root.join("apps/desktop/ui");
    let mut pages: Vec<PathBuf> = std::fs::read_dir(&ui)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("html"))
        .collect();
    pages.sort();
    if pages.is_empty() {
        problems.push("no desktop HTML page found".into());
    }
    for page in &pages {
        let name = page.file_name().unwrap().to_string_lossy().into_owned();
        problems.extend(check_html(&name, &std::fs::read_to_string(page)?));
    }
    problems.extend(check_gitignore(&read(".gitignore")?));
    let rows = parse_registry(&read("docs/security-baseline.md")?)?;
    let mut sources = Vec::new();
    for dir in SOURCE_ROOTS {
        collect_sources(root, &root.join(dir), &mut sources)?;
    }
    problems.extend(check_registry(&rows, &sources));
    if problems.is_empty() {
        Ok(rows.len())
    } else {
        err(format!(
            "security baseline failed:\n  - {}",
            problems.join("\n  - ")
        ))
    }
}
