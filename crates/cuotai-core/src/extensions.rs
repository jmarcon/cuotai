//! Script extensions (PRD 0017). RF1: each extension is a folder of the
//! extensions directory ([`default_dir`], `<OS local data dir>/cuotai/
//! extensions/<id>/`) with a `manifest.toml` ([`MANIFEST_FILE`]) that
//! declares it: `id` (a slug), `name`, `version`, `schema` ([`SCHEMA`]),
//! `command` (an argument vector whose program lives inside the folder),
//! `interval_seconds` (at least [`MIN_INTERVAL_SECONDS`]), `timeout_seconds`
//! (default [`DEFAULT_TIMEOUT_SECONDS`], at most [`MAX_TIMEOUT_SECONDS`]),
//! optional `description` and `homepage`, and `env`, the **names** of the
//! environment variables it asks for.
//!
//! [`discover`] reads the manifests without running anything: every folder
//! becomes either a valid [`Manifest`] or a [`Rejected`] entry with its
//! localized reason (`extensions.error.*`), so one bad manifest never stops
//! the others nor anything else from loading. [`Discovery::catalog`] gives
//! the account types the editors offer ("Extensão: {name}", the
//! [`Level::Extension`] access level and its warning).
//!
//! RF2: [`output::check`] reads what one run printed against the output
//! contract and gives the normalized reading (`source` `extension:<id>`)
//! with every contract violation.
//!
//! RF3: [`runner::Runner`] runs an approved extension without a shell, in
//! its folder, with a reduced environment, closed `stdin`, time and output
//! limits, a process tree killed on timeout, at most two at once and a
//! per-extension deadline with the `RefreshCycle`'s backoff.
//!
//! RF4: [`consent::Consent`] keeps the SHA-256 of the manifest, the
//! program and the whole folder ([`tree::scan`]) of each approved
//! extension; any change disables it until a new approval, and only an
//! [`consent::Approved`] can be run. A folder that cannot be hashed whole
//! (a link out of it, a special file, past the size bounds) or that, on
//! Unix, other users could change is rejected by [`discover`].

pub mod consent;
pub mod management;
pub mod output;
pub mod runner;
pub mod tree;

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::access::Level;
use crate::text::{params, Localized};

/// The manifest's file name inside an extension's folder.
pub const MANIFEST_FILE: &str = "manifest.toml";

/// The only contract version this Cuotai reads.
pub const SCHEMA: u32 = 1;

/// The shortest `interval_seconds` accepted.
pub const MIN_INTERVAL_SECONDS: u64 = 60;

/// An extension's time limit when its manifest sets none.
pub const DEFAULT_TIMEOUT_SECONDS: u64 = 15;

/// The longest `timeout_seconds` accepted.
pub const MAX_TIMEOUT_SECONDS: u64 = 60;

/// A larger manifest is refused unread.
const MANIFEST_MAX_BYTES: u64 = 64 * 1024;

const ID_MAX_CHARS: usize = 64;
const NAME_MAX_CHARS: usize = 64;
const VERSION_MAX_CHARS: usize = 32;
const DESCRIPTION_MAX_CHARS: usize = 500;
const HOMEPAGE_MAX_CHARS: usize = 2048;
const ENV_MAX_NAMES: usize = 32;
const ENV_NAME_MAX_CHARS: usize = 128;

/// The keys a schema-1 manifest accepts, as the errors list them.
const KEYS: [&str; 10] = [
    "id",
    "name",
    "version",
    "schema",
    "command",
    "interval_seconds",
    "timeout_seconds",
    "description",
    "homepage",
    "env",
];

/// `<OS local data dir>/cuotai/extensions`; `None` without a data directory.
pub fn default_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("cuotai").join("extensions"))
}

/// One valid extension, as its manifest declares it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub schema: u32,
    /// The argument vector as written: the program relative to [`Self::dir`]
    /// and its arguments, passed as they are, without a shell.
    pub command: Vec<String>,
    pub interval_seconds: u64,
    pub timeout_seconds: u64,
    pub description: Option<String>,
    pub homepage: Option<String>,
    /// Names of the environment variables the extension asks for (never
    /// their values), without repetitions, in the manifest's order.
    pub env: Vec<String>,
    /// The extension's folder, with symbolic links resolved.
    pub dir: PathBuf,
    /// The program `command[0]` names, resolved inside [`Self::dir`].
    pub program: PathBuf,
    /// The interpreter line of the program (`#!…`, without the `#!`) when
    /// it is a script, shown in the disclosure; `None` for a native
    /// program. Consent only verifies while the program still starts with
    /// exactly this line.
    pub interpreter: Option<String>,
    /// The exact `manifest.toml` text this struct was parsed from. It is
    /// the version the user is shown and the one consent hashes, so parsing
    /// and approving can never straddle two different files.
    pub source: String,
}

/// A folder of the extensions directory that is not a valid extension.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rejected {
    /// The folder (or, when the directory itself cannot be listed, the
    /// extensions directory).
    pub dir: PathBuf,
    /// The manifest's `id`, when it could be read.
    pub id: Option<String>,
    pub reason: Localized,
}

/// PRD 0017 RF1.3: an extension as an account type of the catalog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CatalogEntry {
    /// The extension's `id`.
    pub id: String,
    /// `extensions.catalog.label`: "Extensão: {name}".
    pub label: Localized,
    /// Always [`Level::Extension`].
    pub level: Level,
    /// The level's warning (`access.risk.extension`): it runs a local
    /// program whose code the user should review before enabling it.
    pub warning: Localized,
}

/// What [`discover`] found, in the folders' name order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Discovery {
    pub extensions: Vec<Manifest>,
    pub rejected: Vec<Rejected>,
}

impl Discovery {
    /// The valid extension with `id`.
    pub fn get(&self, id: &str) -> Option<&Manifest> {
        self.extensions.iter().find(|manifest| manifest.id == id)
    }

    /// RF1.3: one account type per valid extension.
    pub fn catalog(&self) -> Vec<CatalogEntry> {
        self.extensions
            .iter()
            .map(|manifest| CatalogEntry {
                id: manifest.id.clone(),
                label: Localized::with_param(
                    "extensions.catalog.label",
                    "name",
                    manifest.name.as_str(),
                    format!("Extensão: {}", manifest.name),
                ),
                level: Level::Extension,
                warning: Level::Extension.risk(),
            })
            .collect()
    }
}

/// RF1.2: the extensions under `root`, read without running anything. Each
/// folder (hidden ones and plain files are skipped) holds one manifest; a
/// folder whose manifest is missing or invalid, and every valid one sharing
/// its `id` with another valid one, is [`Rejected`] with the reason, never
/// an error of the whole discovery. A missing `root` has no extensions.
pub fn discover(root: &Path) -> Discovery {
    let mut discovery = Discovery::default();
    let entries = match std::fs::read_dir(root) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return discovery,
        Err(_) => {
            discovery.rejected.push(Rejected {
                dir: root.to_path_buf(),
                id: None,
                reason: directory_unreadable(root),
            });
            return discovery;
        }
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .filter(|entry| !entry.file_name().to_string_lossy().starts_with('.'))
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    dirs.sort();
    let mut found: Vec<(PathBuf, Manifest)> = Vec::new();
    for dir in dirs {
        match load(&dir) {
            Ok(manifest) => found.push((dir, manifest)),
            Err((id, reason)) => discovery.rejected.push(Rejected { dir, id, reason }),
        }
    }
    let ids: Vec<String> = found.iter().map(|(_, m)| m.id.clone()).collect();
    for (dir, manifest) in found {
        if ids.iter().filter(|id| **id == manifest.id).count() > 1 {
            discovery.rejected.push(Rejected {
                dir,
                reason: duplicate_id(&manifest.id),
                id: Some(manifest.id),
            });
        } else {
            discovery.extensions.push(manifest);
        }
    }
    discovery.rejected.sort_by(|a, b| a.dir.cmp(&b.dir));
    discovery
}

/// The manifest of the folder `dir`; an error carries the `id` when it was
/// read.
fn load(dir: &Path) -> Result<Manifest, (Option<String>, Localized)> {
    let text = read_manifest(&dir.join(MANIFEST_FILE)).map_err(|reason| (None, reason))?;
    let table: toml::Table = text
        .parse()
        .map_err(|error| (None, toml_error(&text, &error)))?;
    let id = table
        .get("id")
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let fail = |reason| (id.clone(), reason);
    match table.get("schema") {
        Some(toml::Value::Integer(schema)) if *schema == i64::from(SCHEMA) => {}
        Some(toml::Value::Integer(schema)) => return Err(fail(unsupported_schema(*schema))),
        _ => return Err(fail(missing_schema())),
    }
    if let Some(key) = table.keys().find(|key| !KEYS.contains(&key.as_str())) {
        return Err(fail(unknown_field(key)));
    }
    let raw: RawManifest =
        toml::from_str(&text).map_err(|error| fail(toml_error(&text, &error)))?;
    validate(dir, raw, &text).map_err(fail)
}

fn read_manifest(path: &Path) -> Result<String, Localized> {
    // Only a regular file is opened: opening a FIFO would block discovery.
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(Localized::fixed(
                "extensions.error.no_manifest",
                "A pasta não tem um manifest.toml.",
            ))
        }
        _ => return Err(manifest_unreadable()),
    }
    let file = std::fs::File::open(path).map_err(|_| manifest_unreadable())?;
    let mut bytes = Vec::new();
    file.take(MANIFEST_MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| manifest_unreadable())?;
    if bytes.len() as u64 > MANIFEST_MAX_BYTES {
        return Err(Localized::with_param(
            "extensions.error.too_large",
            "max",
            MANIFEST_MAX_BYTES / 1024,
            format!(
                "O manifest.toml passa de {} KiB.",
                MANIFEST_MAX_BYTES / 1024
            ),
        ));
    }
    String::from_utf8(bytes).map_err(|_| manifest_unreadable())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawManifest {
    id: String,
    name: String,
    version: String,
    #[allow(dead_code)] // checked before, on the table
    schema: i64,
    command: Vec<String>,
    interval_seconds: i64,
    timeout_seconds: Option<i64>,
    description: Option<String>,
    homepage: Option<String>,
    #[serde(default)]
    env: Vec<String>,
}

fn validate(dir: &Path, raw: RawManifest, source: &str) -> Result<Manifest, Localized> {
    if !valid_id(&raw.id) {
        return Err(Localized::with_param(
            "extensions.error.id",
            "max",
            ID_MAX_CHARS,
            format!(
                "O id deve ser um slug de 1 a {ID_MAX_CHARS} caracteres: letras minúsculas, \
                 dígitos, \"-\" e \"_\", começando por letra ou dígito."
            ),
        ));
    }
    let name = single_line("name", &raw.name, NAME_MAX_CHARS)?;
    let version = single_line("version", &raw.version, VERSION_MAX_CHARS)?;
    let interval_seconds = u64::try_from(raw.interval_seconds)
        .ok()
        .filter(|seconds| *seconds >= MIN_INTERVAL_SECONDS)
        .ok_or_else(|| {
            Localized::with_param(
                "extensions.error.interval",
                "min",
                MIN_INTERVAL_SECONDS,
                format!("O interval_seconds deve ser de pelo menos {MIN_INTERVAL_SECONDS}."),
            )
        })?;
    let timeout_seconds = match raw.timeout_seconds {
        None => DEFAULT_TIMEOUT_SECONDS,
        Some(seconds) => u64::try_from(seconds)
            .ok()
            .filter(|seconds| (1..=MAX_TIMEOUT_SECONDS).contains(seconds))
            .ok_or_else(|| {
                Localized::with_param(
                    "extensions.error.timeout",
                    "max",
                    MAX_TIMEOUT_SECONDS,
                    format!("O timeout_seconds deve ficar entre 1 e {MAX_TIMEOUT_SECONDS}."),
                )
            })?,
    };
    let description = match raw.description.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(text) => {
            if text.chars().count() > DESCRIPTION_MAX_CHARS
                || text.chars().any(|c| c.is_control() && c != '\n')
            {
                return Err(field_text("description", DESCRIPTION_MAX_CHARS));
            }
            Some(text.to_owned())
        }
    };
    let homepage = raw
        .homepage
        .map(|url| valid_homepage(url.trim()))
        .transpose()?;
    let env = valid_env(raw.env)?;
    let (dir, program) = resolve_program(dir, &raw.command)?;
    // FIX-04/FIX-05: the whole folder must be hashable, and on Unix
    // nobody else may change it, before it is ever offered for approval.
    tree::scan(&dir).map_err(tree::Refused::reason)?;
    let interpreter = tree::shebang(&program);
    Ok(Manifest {
        id: raw.id,
        name,
        version,
        schema: SCHEMA,
        command: raw.command,
        interval_seconds,
        timeout_seconds,
        description,
        homepage,
        env,
        dir,
        program,
        interpreter,
        source: source.to_owned(),
    })
}

pub(crate) fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_lowercase() || first.is_ascii_digit())
        && id.chars().count() <= ID_MAX_CHARS
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// A trimmed, non-empty text of at most `max` characters, without control
/// characters.
fn single_line(field: &'static str, text: &str, max: usize) -> Result<String, Localized> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > max || text.chars().any(char::is_control) {
        return Err(field_text(field, max));
    }
    Ok(text.to_owned())
}

/// An `http(s)` address without credentials.
fn valid_homepage(text: &str) -> Result<String, Localized> {
    let url = url::Url::parse(text).ok().filter(|url| {
        matches!(url.scheme(), "https" | "http")
            && url.has_host()
            && url.username().is_empty()
            && url.password().is_none()
            && text.chars().count() <= HOMEPAGE_MAX_CHARS
    });
    match url {
        Some(_) => Ok(text.to_owned()),
        None => Err(Localized::fixed(
            "extensions.error.homepage",
            "O homepage deve ser um endereço http(s) sem usuário nem senha.",
        )),
    }
}

/// Variable names (`[A-Za-z_][A-Za-z0-9_]*`), without repetitions.
fn valid_env(names: Vec<String>) -> Result<Vec<String>, Localized> {
    let valid = |name: &str| {
        let mut chars = name.chars();
        chars
            .next()
            .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
            && name.len() <= ENV_NAME_MAX_CHARS
            && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    };
    let mut env: Vec<String> = Vec::with_capacity(names.len());
    for name in names {
        if !valid(&name) {
            return Err(Localized::with_param(
                "extensions.error.env_name",
                "name",
                quoted(&name),
                format!(
                    "O env lista {}, que não é um nome de variável de ambiente (letras, \
                     dígitos e \"_\", sem começar por dígito).",
                    quoted(&name)
                ),
            ));
        }
        if !env.contains(&name) {
            env.push(name);
        }
    }
    if env.len() > ENV_MAX_NAMES {
        return Err(Localized::with_param(
            "extensions.error.env_count",
            "max",
            ENV_MAX_NAMES,
            format!("O env aceita no máximo {ENV_MAX_NAMES} nomes."),
        ));
    }
    Ok(env)
}

/// The file extensions the operating system hands to a command interpreter
/// instead of executing directly (Windows' `cmd.exe`, PowerShell, VBScript).
/// Those would re-parse the arguments this contract fixes exactly, so they
/// are refused whatever the platform — including case variants such as
/// `.CMD`. A shebang script with the executable bit (`.sh`, `.py`) is not one
/// of these: the kernel executes it directly and never re-parses `argv`.
const INTERPRETED_SUFFIXES: [&str; 5] = ["cmd", "bat", "com", "ps1", "vbs"];

/// The lowercased suffix of `program` when it is an interpreted wrapper.
fn interpreted_suffix(program: &str) -> Option<String> {
    let suffix = Path::new(program)
        .extension()
        .map(|suffix| suffix.to_string_lossy().to_ascii_lowercase())?;
    INTERPRETED_SUFFIXES
        .iter()
        .any(|known| *known == suffix)
        .then_some(suffix)
}

/// `extensions.error.interpreted`: the program would be parsed by an
/// interpreter instead of executed as written.
fn interpreted_program(suffix: String) -> Localized {
    Localized::with_param(
        "extensions.error.interpreted",
        "suffix",
        suffix.as_str(),
        format!(
            "O programa do command termina em .{} e seria interpretado por um shell; \
             use um executável como command[0].",
            suffix
        ),
    )
}

/// The folder and the program of `command`, both with links resolved: the
/// program is a relative path without `..` that names a file inside the
/// folder (an executable one on Unix), never a name looked up in `PATH`.
fn resolve_program(dir: &Path, command: &[String]) -> Result<(PathBuf, PathBuf), Localized> {
    let Some(program) = command.first() else {
        return Err(command_shape());
    };
    if program.is_empty() || command.iter().any(|arg| arg.contains('\0')) {
        return Err(command_shape());
    }
    if let Some(suffix) = interpreted_suffix(program) {
        return Err(interpreted_program(suffix));
    }
    let relative = Path::new(program);
    let inside = relative
        .components()
        .all(|part| matches!(part, Component::Normal(_) | Component::CurDir))
        && relative
            .components()
            .any(|part| matches!(part, Component::Normal(_)));
    if !inside {
        return Err(command_outside());
    }
    let dir = dir.canonicalize().map_err(|_| manifest_unreadable())?;
    let resolved = match dir.join(relative).canonicalize() {
        Ok(path) => path,
        Err(_) => return Err(program_missing(program)),
    };
    if !resolved.starts_with(&dir) {
        return Err(command_outside());
    }
    if !resolved.is_file() {
        return Err(program_missing(program));
    }
    // The suffix check runs again on the **resolved** path: a symlink (or a
    // Windows extension-less alias) pointing at `script.cmd` would be handed
    // to `cmd.exe` with our exact argv, whatever the manifest spelled.
    if let Some(suffix) = interpreted_suffix(&resolved.to_string_lossy()) {
        return Err(interpreted_program(suffix));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&resolved)
            .map_err(|_| program_missing(program))?
            .permissions()
            .mode();
        if mode & 0o111 == 0 {
            return Err(Localized::with_param(
                "extensions.error.not_executable",
                "program",
                quoted(program),
                format!(
                    "O programa {} do command não é executável (chmod +x).",
                    quoted(program)
                ),
            ));
        }
    }
    Ok((dir, resolved))
}

fn quoted(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_default()
}

fn toml_error(text: &str, error: &toml::de::Error) -> Localized {
    let at = error.span().map_or(0, |span| span.start.min(text.len()));
    let line = text.as_bytes()[..at]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    let keys = KEYS.join(", ");
    Localized::with_params(
        "extensions.error.toml",
        params([("line", line.into()), ("keys", keys.as_str().into())]),
        format!(
            "manifest.toml inválido na linha {line}: ele aceita {keys}, com command e env como \
             listas de textos e os números como inteiros."
        ),
    )
}

fn missing_schema() -> Localized {
    Localized::with_param(
        "extensions.error.schema_missing",
        "schema",
        SCHEMA.to_string(),
        format!("O manifest.toml precisa de schema = {SCHEMA}."),
    )
}

fn unsupported_schema(schema: i64) -> Localized {
    Localized::with_params(
        "extensions.error.schema",
        params([
            ("schema", schema.to_string().into()),
            ("supported", SCHEMA.to_string().into()),
        ]),
        format!(
            "O manifest.toml usa o schema {schema}; esta versão do Cuotai lê apenas o schema \
             {SCHEMA}."
        ),
    )
}

fn unknown_field(key: &str) -> Localized {
    let keys = KEYS.join(", ");
    Localized::with_params(
        "extensions.error.unknown_field",
        params([
            ("field", quoted(key).into()),
            ("keys", keys.as_str().into()),
        ]),
        format!(
            "O manifest.toml tem o campo desconhecido {}; os campos aceitos são {keys}.",
            quoted(key)
        ),
    )
}

fn field_text(field: &'static str, max: usize) -> Localized {
    Localized::with_params(
        "extensions.error.text",
        params([("field", field.into()), ("max", max.into())]),
        format!("O {field} deve ter de 1 a {max} caracteres, sem caracteres de controle."),
    )
}

fn command_shape() -> Localized {
    Localized::fixed(
        "extensions.error.command",
        "O command deve ser uma lista de argumentos cujo primeiro item é o programa, \
         relativo à pasta da extensão, como [\"./run.sh\", \"--opcao\"].",
    )
}

fn command_outside() -> Localized {
    Localized::fixed(
        "extensions.error.command_outside",
        "O programa do command deve ficar dentro da pasta da extensão: um caminho relativo, \
         sem \"..\", que não leve para fora dela.",
    )
}

fn program_missing(program: &str) -> Localized {
    Localized::with_param(
        "extensions.error.program_missing",
        "program",
        quoted(program),
        format!(
            "O programa {} do command não é um arquivo da pasta da extensão.",
            quoted(program)
        ),
    )
}

fn duplicate_id(id: &str) -> Localized {
    Localized::with_param(
        "extensions.error.duplicate_id",
        "id",
        id,
        format!("Mais de uma extensão usa o id \"{id}\"; nenhuma delas é carregada."),
    )
}

fn manifest_unreadable() -> Localized {
    Localized::fixed(
        "extensions.error.unreadable",
        "Não foi possível ler o manifest.toml como texto UTF-8.",
    )
}

fn directory_unreadable(root: &Path) -> Localized {
    let path = root.display().to_string();
    Localized::with_param(
        "extensions.error.directory",
        "path",
        path.as_str(),
        format!("Não foi possível listar o diretório de extensões {path}."),
    )
}

#[cfg(test)]
#[path = "extensions_tests.rs"]
mod tests;
