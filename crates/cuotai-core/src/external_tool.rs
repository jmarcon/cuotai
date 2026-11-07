//! PRD 0054 RF6: the one way Cuotai finds and checks the provider CLIs it
//! runs (`aws` for AWS Bedrock, `bl` for the Alibaba plans).
//!
//! An app opened from the Finder or a desktop launcher does not inherit the
//! shell's `PATH` (on macOS it gets `/usr/bin:/bin:/usr/sbin:/sbin`), so a CLI
//! a terminal finds would read as not installed. [`Resolver::resolve`] tries,
//! in order:
//!
//! 1. the account's `params.cli_path`, when set: an absolute path to an
//!    executable file, authoritative (an invalid one fails, with no fallback);
//! 2. the program names in `PATH`;
//! 3. the documented install locations of [`Resolver::well_known`].
//!
//! Every input (PATH, home, `%ProgramFiles%`, the OS) is injected, so the
//! order is tested without the real environment; nothing shells out to
//! `which`/`where`. Errors and diagnostics name the tool, the candidate
//! files (home shown as `~`) and the literal word `PATH`, never the value of
//! the environment.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::text::{home_relative, params, Localized, ParamValue};

/// The account parameter holding an explicit program path (RF6.5).
pub const CLI_PATH_PARAM: &str = "cli_path";

/// A provider CLI Cuotai can run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tool {
    /// AWS CLI v2 (`aws`), for AWS Bedrock.
    Aws,
    /// Alibaba Model Studio CLI (`bl`), for the Alibaba plans.
    Bailian,
}

impl Tool {
    /// The program name without extension, also the diagnostic slug.
    pub fn name(self) -> &'static str {
        match self {
            Self::Aws => "aws",
            Self::Bailian => "bl",
        }
    }

    /// The tool an account of `provider` runs, if any.
    pub fn for_provider(provider: crate::Provider) -> Option<Self> {
        match provider {
            crate::Provider::AwsBedrock => Some(Self::Aws),
            crate::Provider::AlibabaTokenPlan | crate::Provider::AlibabaModelStudio => {
                Some(Self::Bailian)
            }
            _ => None,
        }
    }
}

/// Which rule found the program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSource {
    /// The account's `params.cli_path`.
    Configured,
    /// A directory listed in `PATH`.
    Path,
    /// One of the documented install locations.
    WellKnown,
}

impl ToolSource {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::Path => "path",
            Self::WellKnown => "well_known",
        }
    }
}

/// A program to start, and how it was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTool {
    pub tool: Tool,
    pub program: PathBuf,
    pub source: ToolSource,
}

/// Why a tool could not be resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// `params.cli_path` is not an absolute path to an executable file.
    InvalidConfigured { tool: Tool },
    /// Found nowhere: the well-known candidates that were tried (PATH too).
    Missing {
        tool: Tool,
        candidates: Vec<PathBuf>,
    },
}

impl ToolError {
    /// The redacted, localized explanation.
    pub fn l10n(&self) -> Localized {
        match self {
            Self::InvalidConfigured { tool } => Localized::with_param(
                "tool.error.invalid_path",
                "tool",
                tool.name(),
                "params.cli_path must be the absolute path of the {tool} program, an executable file.",
            ),
            Self::Missing { tool, candidates } => Localized::with_params(
                "tool.error.missing",
                params([
                    ("tool", ParamValue::from(tool.name())),
                    ("candidates", ParamValue::from(render_candidates(candidates))),
                ]),
                "The {tool} program was not found in PATH or at {candidates}. Install it, or set params.cli_path to its full path.",
            ),
        }
    }
}

fn render_candidates(candidates: &[PathBuf]) -> String {
    if candidates.is_empty() {
        return "—".to_owned();
    }
    candidates
        .iter()
        .map(|path| home_relative(path))
        .collect::<Vec<_>>()
        .join(", ")
}

/// The operating systems with distinct install locations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    MacOs,
    Linux,
    Windows,
    Other,
}

impl Os {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else if cfg!(target_os = "linux") {
            Self::Linux
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Other
        }
    }
}

/// The resolution inputs. [`Resolver::from_process`] reads them from the
/// environment; tests build them by hand.
#[derive(Debug, Clone)]
pub struct Resolver {
    pub path: OsString,
    pub home: Option<PathBuf>,
    pub program_files: Option<PathBuf>,
    pub os: Os,
}

impl Resolver {
    pub fn from_process() -> Self {
        Self {
            path: std::env::var_os("PATH").unwrap_or_default(),
            home: dirs::home_dir(),
            program_files: std::env::var_os("ProgramFiles").map(PathBuf::from),
            os: Os::current(),
        }
    }

    /// The file name of `tool` on this OS.
    fn file_name(&self, tool: Tool) -> String {
        match self.os {
            Os::Windows => format!("{}.exe", tool.name()),
            _ => tool.name().to_owned(),
        }
    }

    /// RF6.2: the documented install locations of `tool`, in order, whether
    /// or not they exist.
    pub fn well_known(&self, tool: Tool) -> Vec<PathBuf> {
        let file = self.file_name(tool);
        let home_bin = self
            .home
            .as_ref()
            .map(|home| home.join(".local").join("bin").join(&file));
        let mut out = Vec::new();
        match self.os {
            Os::MacOs => {
                out.push(PathBuf::from("/opt/homebrew/bin").join(&file));
                out.push(PathBuf::from("/usr/local/bin").join(&file));
                out.extend(home_bin);
            }
            Os::Linux | Os::Other => {
                out.push(PathBuf::from("/usr/local/bin").join(&file));
                out.push(PathBuf::from("/usr/bin").join(&file));
                out.extend(home_bin);
            }
            Os::Windows => {
                if tool == Tool::Aws {
                    if let Some(program_files) = &self.program_files {
                        out.push(program_files.join("Amazon").join("AWSCLIV2").join(&file));
                    }
                }
                out.extend(home_bin);
            }
        }
        out
    }

    /// The names a program can have in a `PATH` directory.
    fn path_names(&self, tool: Tool) -> Vec<String> {
        match self.os {
            Os::Windows => [".exe", ".cmd", ".bat"]
                .iter()
                .map(|suffix| format!("{}{suffix}", tool.name()))
                .collect(),
            _ => vec![tool.name().to_owned()],
        }
    }

    /// Resolve `tool`; `configured` is the account's `params.cli_path`.
    pub fn resolve(&self, tool: Tool, configured: Option<&str>) -> Result<ResolvedTool, ToolError> {
        if let Some(configured) = configured.map(str::trim).filter(|c| !c.is_empty()) {
            let program = PathBuf::from(configured);
            return if self.valid_configured(&program) {
                Ok(ResolvedTool {
                    tool,
                    program,
                    source: ToolSource::Configured,
                })
            } else {
                Err(ToolError::InvalidConfigured { tool })
            };
        }
        let names = self.path_names(tool);
        for dir in std::env::split_paths(&self.path) {
            if let Some(program) = names
                .iter()
                .map(|name| dir.join(name))
                .find(|candidate| is_program(candidate))
            {
                return Ok(ResolvedTool {
                    tool,
                    program,
                    source: ToolSource::Path,
                });
            }
        }
        let candidates = self.well_known(tool);
        match candidates.iter().find(|candidate| is_program(candidate)) {
            Some(program) => Ok(ResolvedTool {
                tool,
                program: program.clone(),
                source: ToolSource::WellKnown,
            }),
            None => Err(ToolError::Missing { tool, candidates }),
        }
    }

    fn valid_configured(&self, program: &Path) -> bool {
        // `Path::is_absolute` follows the host; an injected Windows OS also
        // accepts a drive path so the tests can run anywhere.
        let absolute = program.is_absolute()
            || (self.os == Os::Windows && program.to_string_lossy().get(1..3) == Some(":\\"));
        let extension = self.os != Os::Windows
            || program
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"));
        absolute && extension && is_program(program)
    }
}

/// RF6.1: the program for `tool` on an account with `params`, from the
/// process environment.
pub fn resolve_for(
    tool: Tool,
    params: &std::collections::BTreeMap<String, String>,
) -> Result<ResolvedTool, ToolError> {
    Resolver::from_process().resolve(tool, params.get(CLI_PATH_PARAM).map(String::as_str))
}

/// Whether `tool` is found without a configured path.
pub fn found(tool: Tool) -> bool {
    Resolver::from_process().resolve(tool, None).is_ok()
}

/// RF6.3: the redacted resolution of one tool, for `--doctor`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ToolReport {
    pub tool: Tool,
    /// `found`, `missing` or `invalid`.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<&'static str>,
    /// The program, home shown as `~`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

impl ToolReport {
    pub fn of(result: &Result<ResolvedTool, ToolError>) -> Self {
        match result {
            Ok(resolved) => Self {
                tool: resolved.tool,
                status: "found",
                source: Some(resolved.source.slug()),
                path: Some(home_relative(&resolved.program)),
            },
            Err(ToolError::InvalidConfigured { tool }) => Self {
                tool: *tool,
                status: "invalid",
                source: Some(ToolSource::Configured.slug()),
                path: None,
            },
            Err(ToolError::Missing { tool, .. }) => Self {
                tool: *tool,
                status: "missing",
                source: None,
                path: None,
            },
        }
    }

    /// `tool.aws: found (well_known, /opt/homebrew/bin/aws)` or
    /// `tool.aws: missing`.
    pub fn line(&self) -> String {
        match (&self.source, &self.path) {
            (Some(source), Some(path)) => {
                format!(
                    "tool.{}: {} ({source}, {path})",
                    self.tool.name(),
                    self.status
                )
            }
            (Some(source), None) => {
                format!("tool.{}: {} ({source})", self.tool.name(), self.status)
            }
            _ => format!("tool.{}: {}", self.tool.name(), self.status),
        }
    }
}

/// PRD 0054 RF6.4: whether `profile` may follow `--profile` on an `aws`
/// command line: 1 to 128 bytes, not starting with `-` (it would read as an
/// option) and without control characters. Login, model listing and queries
/// all check it before starting the CLI.
pub fn valid_aws_profile(profile: &str) -> bool {
    !profile.is_empty()
        && profile.len() <= 128
        && !profile.starts_with('-')
        && !profile.chars().any(char::is_control)
}

/// PRD 0054 RF6.4: the one rule for a `bl` profile name (`--config`): 1 to
/// 32 ASCII letters, digits, `-` or `_`, not starting with `-`. The query,
/// the login and the editor all use it.
pub fn valid_bl_profile(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 32
        && !name.starts_with('-')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(unix)]
fn is_program(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_program(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod profile_tests {
    #[test]
    fn an_aws_profile_is_never_an_option_or_a_control_character() {
        for ok in ["prod", "my-profile_1", "a", &"p".repeat(128), "sso:team"] {
            assert!(super::valid_aws_profile(ok), "{ok}");
        }
        for bad in [
            "",
            "-p",
            "--debug",
            "--endpoint-url=x",
            "a\nb",
            "a\u{7}",
            &"p".repeat(129),
        ] {
            assert!(!super::valid_aws_profile(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_bl_profile_is_short_ascii_and_never_an_option() {
        for ok in ["default", "work-profile-1_v2", &"x".repeat(32)] {
            assert!(super::valid_bl_profile(ok), "{ok}");
        }
        for bad in [
            "",
            "-leading",
            "with space",
            "semi;colon",
            "with.dot",
            &"x".repeat(33),
        ] {
            assert!(!super::valid_bl_profile(bad), "{bad:?}");
        }
    }
}

#[cfg(test)]
mod well_known_tests {
    use super::*;

    fn resolver(os: Os) -> Resolver {
        Resolver {
            path: OsString::new(),
            home: Some(PathBuf::from("/home/someone")),
            program_files: Some(PathBuf::from("C:\\Program Files")),
            os,
        }
    }

    #[test]
    fn the_install_locations_are_fixed_per_os() {
        let mac = resolver(Os::MacOs).well_known(Tool::Aws);
        assert_eq!(
            mac,
            [
                PathBuf::from("/opt/homebrew/bin/aws"),
                PathBuf::from("/usr/local/bin/aws"),
                PathBuf::from("/home/someone/.local/bin/aws"),
            ]
        );
        let linux = resolver(Os::Linux).well_known(Tool::Bailian);
        assert_eq!(
            linux,
            [
                PathBuf::from("/usr/local/bin/bl"),
                PathBuf::from("/usr/bin/bl"),
                PathBuf::from("/home/someone/.local/bin/bl"),
            ]
        );
        let windows_aws = resolver(Os::Windows).well_known(Tool::Aws);
        assert_eq!(windows_aws.len(), 2);
        assert!(windows_aws[0].ends_with("aws.exe"));
        assert!(windows_aws[0].to_string_lossy().contains("AWSCLIV2"));
        assert!(windows_aws[1].ends_with("bl.exe") || windows_aws[1].ends_with("aws.exe"));
        let windows_bl = resolver(Os::Windows).well_known(Tool::Bailian);
        assert_eq!(windows_bl.len(), 1, "no Program Files location for bl");
        assert!(windows_bl[0].ends_with("bl.exe"));
        let no_home = Resolver {
            home: None,
            ..resolver(Os::Linux)
        };
        assert_eq!(no_home.well_known(Tool::Aws).len(), 2);
    }

    #[test]
    fn a_missing_tool_names_the_candidates_and_path_but_not_its_value() {
        let resolver = Resolver {
            path: OsString::from("/secret-dir-from-path"),
            ..resolver(Os::Linux)
        };
        let error = resolver.resolve(Tool::Aws, None).unwrap_err();
        let note = error.l10n();
        assert_eq!(note.id, "tool.error.missing");
        let text = note.text().to_owned();
        assert!(text.contains("PATH"), "{text}");
        assert!(text.contains("/usr/bin/aws"), "{text}");
        assert!(!text.contains("secret-dir-from-path"), "{text}");
        assert_eq!(ToolReport::of(&Err(error)).line(), "tool.aws: missing");
    }

    #[test]
    fn a_relative_configured_path_fails_without_fallback() {
        let error = resolver(Os::Linux)
            .resolve(Tool::Aws, Some("bin/aws"))
            .unwrap_err();
        assert_eq!(error, ToolError::InvalidConfigured { tool: Tool::Aws });
        assert_eq!(error.l10n().id, "tool.error.invalid_path");
        let report = ToolReport::of(&Err(error));
        assert_eq!(report.line(), "tool.aws: invalid (configured)");
    }

    #[test]
    fn tools_belong_to_their_providers() {
        assert_eq!(
            Tool::for_provider(crate::Provider::AwsBedrock),
            Some(Tool::Aws)
        );
        assert_eq!(
            Tool::for_provider(crate::Provider::AlibabaTokenPlan),
            Some(Tool::Bailian)
        );
        assert_eq!(
            Tool::for_provider(crate::Provider::AlibabaModelStudio),
            Some(Tool::Bailian)
        );
        assert_eq!(Tool::for_provider(crate::Provider::OpenAiApi), None);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn program(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let file = dir.join(name);
        std::fs::write(&file, b"").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(mode)).unwrap();
        file
    }

    /// A Linux resolver whose home is `home` and whose PATH is `path`.
    fn linux(path: &Path, home: &Path) -> Resolver {
        Resolver {
            path: path.as_os_str().to_owned(),
            home: Some(home.to_path_buf()),
            program_files: None,
            os: Os::Linux,
        }
    }

    #[test]
    fn path_comes_before_the_install_locations() {
        let path_dir = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        let in_path = program(path_dir.path(), "bl", 0o755);
        std::fs::create_dir_all(home.path().join(".local/bin")).unwrap();
        program(&home.path().join(".local/bin"), "bl", 0o755);
        let resolved = linux(path_dir.path(), home.path())
            .resolve(Tool::Bailian, None)
            .unwrap();
        assert_eq!(resolved.program, in_path);
        assert_eq!(resolved.source, ToolSource::Path);
    }

    #[test]
    fn a_desktop_path_still_finds_a_program_in_an_install_location() {
        let empty = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".local/bin")).unwrap();
        let file = program(&home.path().join(".local/bin"), "bl", 0o755);
        let resolved = linux(empty.path(), home.path())
            .resolve(Tool::Bailian, None)
            .unwrap();
        assert_eq!(resolved.program, file);
        assert_eq!(resolved.source, ToolSource::WellKnown);
        let report = ToolReport::of(&Ok(resolved));
        assert_eq!(report.status, "found");
        assert_eq!(report.source, Some("well_known"));
    }

    #[test]
    fn a_missing_or_non_executable_file_is_not_a_program() {
        let empty = tempfile::tempdir().unwrap();
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".local/bin")).unwrap();
        program(&home.path().join(".local/bin"), "bl", 0o644);
        program(empty.path(), "bl", 0o644);
        let error = linux(empty.path(), home.path())
            .resolve(Tool::Bailian, None)
            .unwrap_err();
        assert!(matches!(
            error,
            ToolError::Missing {
                tool: Tool::Bailian,
                ..
            }
        ));
    }

    #[test]
    fn a_configured_path_wins_and_an_invalid_one_never_falls_back() {
        let path_dir = tempfile::tempdir().unwrap();
        let custom = tempfile::tempdir().unwrap();
        program(path_dir.path(), "aws", 0o755);
        let mine = program(custom.path(), "my-aws", 0o755);
        let resolver = linux(path_dir.path(), custom.path());
        let resolved = resolver
            .resolve(Tool::Aws, Some(mine.to_str().unwrap()))
            .unwrap();
        assert_eq!(
            (resolved.program, resolved.source),
            (mine, ToolSource::Configured)
        );
        let not_exec = program(custom.path(), "plain", 0o644);
        for bad in [
            not_exec.to_str().unwrap().to_owned(),
            custom.path().to_str().unwrap().to_owned(),
            custom.path().join("absent").to_str().unwrap().to_owned(),
        ] {
            assert_eq!(
                resolver.resolve(Tool::Aws, Some(&bad)),
                Err(ToolError::InvalidConfigured { tool: Tool::Aws }),
                "{bad}"
            );
        }
        // Blank means not configured.
        assert_eq!(
            resolver.resolve(Tool::Aws, Some("  ")).unwrap().source,
            ToolSource::Path
        );
    }

    #[test]
    fn windows_wants_an_exe_for_a_configured_path() {
        let dir = tempfile::tempdir().unwrap();
        let script = program(dir.path(), "aws.sh", 0o755);
        let resolver = Resolver {
            path: OsString::new(),
            home: None,
            program_files: None,
            os: Os::Windows,
        };
        assert!(resolver
            .resolve(Tool::Aws, Some(script.to_str().unwrap()))
            .is_err());
        let exe = program(dir.path(), "aws.exe", 0o755);
        assert!(resolver
            .resolve(Tool::Aws, Some(exe.to_str().unwrap()))
            .is_ok());
    }
}
