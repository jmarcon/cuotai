//! Package-manager manifest rendering and validation (PRD 0021 RF5, RF5.6).
//!
//! One data source — the verified release `manifest.json` (plus
//! `signing-state.json` when present), the repository slug and the tag — is
//! rendered through the templates in `packaging/<channel>/` into the manifest
//! set each channel consumes. Rendered output is validated structurally
//! before it is written, so a broken template fails the pipeline instead of
//! a package-manager review. Nothing here contacts a network or a package
//! manager; publishing is the workflow's job (RF5.7).

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use crate::manifest::{Manifest, ManifestArtifact};
use crate::signing::SigningState;
use crate::{err, Res};

/// Everything a render needs; URLs and checksums always come from the
/// verified release manifest, never from a rebuilt file (RF5).
pub struct RenderRequest {
    pub manifest: Manifest,
    pub signing: Option<SigningState>,
    /// `owner/name`; used for download URLs and publisher metadata.
    pub repo: String,
    /// Release tag (`vX.Y.Z`; `vX.Y.Z-rc.N` renders for rehearsal but the
    /// workflow never publishes channels from a pre-release).
    pub tag: String,
}

/// One rendered manifest file, with its path relative to the output root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rendered {
    pub channel: &'static str,
    pub path: PathBuf,
    pub content: String,
}

/// Accepts either a `manifest.json` file or a directory that contains one.
/// A sibling `signing-state.json` (same directory) is loaded when present.
pub fn load_inputs(path: &Path) -> Res<(Manifest, Option<SigningState>)> {
    if !path.exists() {
        return err(format!(
            "no release manifest at {}: pass manifest.json, or a directory that contains it (the release-assembly artifact)",
            path.display()
        ));
    }
    let (manifest_path, dir) = if path.is_dir() {
        (path.join("manifest.json"), path.to_path_buf())
    } else {
        let dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        (path.to_path_buf(), dir)
    };
    let manifest = crate::manifest::load(&manifest_path)?;
    let signing_path = dir.join("signing-state.json");
    let signing = if signing_path.is_file() {
        let state = crate::signing::load(&signing_path)?;
        if state.version != manifest.version {
            return err(format!(
                "signing-state.json version {} does not match manifest version {}",
                state.version, manifest.version
            ));
        }
        Some(state)
    } else {
        None
    };
    Ok((manifest, signing))
}

fn download_url(repo: &str, tag: &str, name: &str) -> String {
    format!("https://github.com/{repo}/releases/download/{tag}/{name}")
}

/// The one artifact of a kind/platform/arch triple; a channel fails with a
/// clear message instead of rendering a manifest that cannot work.
fn need<'m>(
    manifest: &'m Manifest,
    kind: &str,
    platform: &str,
    arch: &str,
    channel: &str,
) -> Res<&'m ManifestArtifact> {
    manifest
        .artifacts
        .iter()
        .find(|a| a.kind == kind && a.platform == platform && a.arch == arch)
        .ok_or_else(|| {
            crate::Error::Msg(format!(
                "channel {channel}: manifest lists no {kind} artifact for {platform}/{arch}"
            ))
        })
}

/// An optional artifact (ARM64 legs, RF2.5): included when produced.
fn optional<'m>(
    manifest: &'m Manifest,
    kind: &str,
    platform: &str,
    arch: &str,
) -> Option<&'m ManifestArtifact> {
    manifest
        .artifacts
        .iter()
        .find(|a| a.kind == kind && a.platform == platform && a.arch == arch)
}

fn validate_repo(repo: &str) -> Res<()> {
    let valid = repo.split('/').count() == 2
        && repo.split('/').all(|part| {
            !part.is_empty()
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        });
    if valid {
        Ok(())
    } else {
        err(format!(
            "invalid repository slug '{repo}' (expected owner/name)"
        ))
    }
}

fn validate_tag(req: &RenderRequest) -> Res<()> {
    let tag = crate::parse_tag(&req.tag)?;
    if tag.version.to_string() != req.manifest.version {
        return err(format!(
            "tag {} does not match manifest version {}",
            req.tag, req.manifest.version
        ));
    }
    Ok(())
}

/// Substitutes `{{TOKEN}}` placeholders; any leftover `{{` afterwards means
/// the template references data the renderer does not provide.
fn substitute(template: &str, values: &BTreeMap<&str, String>, context: &str) -> Res<String> {
    let mut out = template.to_string();
    for (key, value) in values {
        out = out.replace(&format!("{{{{{key}}}}}"), value);
    }
    if let Some(at) = out.find("{{") {
        let token: String = out[at..].chars().take(32).collect();
        return err(format!("unresolved placeholder in {context}: {token}…"));
    }
    Ok(out)
}

fn load_template(templates_dir: &Path, relative: &str) -> Res<String> {
    let path = templates_dir.join(relative);
    std::fs::read_to_string(&path)
        .map_err(|e| crate::Error::Msg(format!("cannot read template {}: {e}", path.display())))
}

/// Structural check for the Ruby cask/formula: required stanzas per file
/// and a balanced block structure (`class`/`def`/`… do` vs `end`). The CI
/// job additionally runs `brew style`/`brew audit` on a macOS runner.
fn validate_ruby(context: &str, content: &str, required: &[&str]) -> Res<()> {
    for stanza in required {
        if !content.contains(stanza) {
            return err(format!("{context}: missing required stanza '{stanza}'"));
        }
    }
    let mut depth: i64 = 0;
    for (n, line) in content.lines().enumerate() {
        let t = line.trim();
        let opens = t.ends_with(" do")
            || t.contains(" do |")
            || t.starts_with("class ")
            || t.starts_with("def ")
            || t.starts_with("module ");
        if t == "end" {
            depth -= 1;
        }
        if opens {
            depth += 1;
        }
        if depth < 0 {
            return err(format!("{context}: unbalanced 'end' at line {}", n + 1));
        }
    }
    if depth != 0 {
        return err(format!("{context}: {depth} unclosed block(s)"));
    }
    Ok(())
}

/// Structural check for winget YAML: parses as YAML and carries the fields
/// the winget schema requires for each manifest type.
fn validate_yaml(context: &str, content: &str) -> Res<()> {
    let value: serde_yaml::Value = serde_yaml::from_str(content)
        .map_err(|e| crate::Error::Msg(format!("{context}: invalid YAML: {e}")))?;
    let get = |key: &str| value.get(key).and_then(|v| v.as_str());
    if get("PackageIdentifier") != Some("Cuotai.Cuotai") {
        return err(format!(
            "{context}: PackageIdentifier must be Cuotai.Cuotai"
        ));
    }
    match get("ManifestType") {
        Some("version") | Some("defaultLocale") => {}
        Some("installer") => {
            let installers = value
                .get("Installers")
                .and_then(|v| v.as_sequence())
                .filter(|s| !s.is_empty());
            let Some(installers) = installers else {
                return err(format!(
                    "{context}: installer manifest needs at least one installer"
                ));
            };
            for installer in installers {
                let url_ok = installer
                    .get("InstallerUrl")
                    .and_then(|v| v.as_str())
                    .is_some_and(|u| u.starts_with("https://github.com/"));
                let sha_ok = installer
                    .get("InstallerSha256")
                    .and_then(|v| v.as_str())
                    .is_some_and(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()));
                if !url_ok || !sha_ok {
                    return err(format!(
                        "{context}: every installer needs an https InstallerUrl and a SHA-256"
                    ));
                }
            }
        }
        other => {
            return err(format!(
                "{context}: unexpected ManifestType {}",
                other.unwrap_or("<missing>")
            ))
        }
    }
    Ok(())
}

/// Structural check for the Scoop bucket manifest: parses as JSON and
/// carries the version, per-architecture URLs/hashes and the
/// `checkver`/`autoupdate` pair that lets the bucket follow releases.
fn validate_scoop(context: &str, content: &str, version: &str) -> Res<()> {
    let value: serde_json::Value =
        serde_json::from_str(content).map_err(|e| crate::Error::Json {
            context: context.to_string(),
            source: e,
        })?;
    if value.get("version").and_then(|v| v.as_str()) != Some(version) {
        return err(format!(
            "{context}: version field does not match the release"
        ));
    }
    if value.get("checkver").and_then(|v| v.as_str()) != Some("github") {
        return err(format!("{context}: checkver must be 'github'"));
    }
    let arch = value
        .get("architecture")
        .and_then(|a| a.get("64bit"))
        .ok_or_else(|| crate::Error::Msg(format!("{context}: missing architecture.64bit")))?;
    for key in ["url", "hash"] {
        if arch
            .get(key)
            .and_then(|v| v.as_str())
            .is_none_or(str::is_empty)
        {
            return err(format!("{context}: architecture.64bit.{key} is missing"));
        }
    }
    if value
        .pointer("/autoupdate/architecture/64bit/url")
        .is_none()
    {
        return err(format!(
            "{context}: autoupdate.architecture.64bit.url is missing"
        ));
    }
    Ok(())
}

/// Structural check for the rendered PKGBUILD: the assignments the AUR
/// requires and a `package()` function. The CI job additionally runs
/// `bash -n`.
fn validate_pkgbuild(context: &str, content: &str, version: &str) -> Res<()> {
    for required in [
        "pkgname=cuotai-bin",
        &format!("pkgver={version}"),
        "pkgrel=",
        "source_x86_64=(",
        "sha256sums_x86_64=(",
        "package() {",
    ] {
        if !content.contains(required) {
            return err(format!("{context}: missing required line '{required}'"));
        }
    }
    Ok(())
}

fn base_values(req: &RenderRequest) -> Res<BTreeMap<&'static str, String>> {
    validate_repo(&req.repo)?;
    validate_tag(req)?;
    let owner = req.repo.split('/').next().unwrap_or_default().to_string();
    let mut values = BTreeMap::new();
    values.insert("VERSION", req.manifest.version.clone());
    values.insert("TAG", req.tag.clone());
    values.insert("REPO", req.repo.clone());
    values.insert("REPO_OWNER", owner);
    Ok(values)
}

fn render_homebrew(templates_dir: &Path, req: &RenderRequest) -> Res<Vec<Rendered>> {
    let v = &req.manifest.version;
    let dmg = need(
        &req.manifest,
        "desktop-dmg",
        "macos",
        "universal",
        "homebrew",
    )?;
    let cli_arm = need(&req.manifest, "cli-archive", "macos", "aarch64", "homebrew")?;
    let cli_x64 = need(&req.manifest, "cli-archive", "macos", "x86_64", "homebrew")?;

    // RF5.1: the unsigned cask carries the Gatekeeper caveat; a Developer ID
    // signed DMG does not need it. No signing state means "not proven
    // signed", which renders the caveat (honest by default).
    let dmg_trusted = req
        .signing
        .as_ref()
        .and_then(|s| s.artifacts.iter().find(|a| a.name == dmg.name))
        .is_some_and(|a| a.signed);
    let caveats = if dmg_trusted {
        String::new()
    } else {
        "\n  caveats do\n    <<~EOS\n      Cuotai is unsigned in this release (ad-hoc signature, no Apple\n      notarization), so Gatekeeper blocks the first launch. Allow it once\n      with `xattr -dr com.apple.quarantine /Applications/Cuotai.app` or via\n      System Settings > Privacy & Security > Open Anyway.\n    EOS\n  end\n"
            .to_string()
    };

    let mut values = base_values(req)?;
    values.insert("DMG_SHA256", dmg.sha256.clone());
    values.insert("CASK_CAVEATS", caveats);
    let cask = substitute(
        &load_template(templates_dir, "homebrew/Casks/cuotai-app.rb.tmpl")?,
        &values,
        "homebrew cask",
    )?;
    validate_ruby(
        "homebrew cask",
        &cask,
        &[
            "cask \"cuotai-app\" do",
            &format!("version \"{v}\""),
            &format!("sha256 \"{}\"", dmg.sha256),
            // The cask keeps Homebrew's `#{version}` interpolation in the URL.
            &format!(
                "url \"https://github.com/{}/releases/download/{}/Cuotai_#{{version}}_universal.dmg\"",
                req.repo, req.tag
            ),
            "app \"Cuotai.app\"",
            "zap trash:",
        ],
    )?;

    let mut values = base_values(req)?;
    values.insert("CLI_MACOS_AARCH64_SHA256", cli_arm.sha256.clone());
    values.insert("CLI_MACOS_X86_64_SHA256", cli_x64.sha256.clone());
    let formula = substitute(
        &load_template(templates_dir, "homebrew/Formula/cuotai.rb.tmpl")?,
        &values,
        "homebrew formula",
    )?;
    validate_ruby(
        "homebrew formula",
        &formula,
        &[
            "class Cuotai < Formula",
            &format!("sha256 \"{}\"", cli_arm.sha256),
            &format!("sha256 \"{}\"", cli_x64.sha256),
            &format!(
                "cuotai-{}-aarch64-apple-darwin.tar.gz",
                req.manifest.version
            ),
            "bin.install \"cuotai\"",
            "generate_completions_from_executable(bin/\"cuotai\", \"--completions\")",
        ],
    )?;

    Ok(vec![
        Rendered {
            channel: "homebrew",
            path: PathBuf::from("homebrew/Casks/cuotai-app.rb"),
            content: cask,
        },
        Rendered {
            channel: "homebrew",
            path: PathBuf::from("homebrew/Formula/cuotai.rb"),
            content: formula,
        },
    ])
}

fn render_winget(templates_dir: &Path, req: &RenderRequest) -> Res<Vec<Rendered>> {
    let nsis = need(
        &req.manifest,
        "desktop-installer-nsis",
        "windows",
        "x64",
        "winget",
    )?;
    let arm64 = optional(&req.manifest, "desktop-installer-nsis", "windows", "arm64").map(|a| {
        format!(
            "- Architecture: arm64\n  InstallerUrl: {}\n  InstallerSha256: {}\n",
            download_url(&req.repo, &req.tag, &a.name),
            a.sha256
        )
    });

    let version_manifest = substitute(
        &load_template(templates_dir, "winget/Cuotai.Cuotai.yaml.tmpl")?,
        &base_values(req)?,
        "winget version manifest",
    )?;

    let mut values = base_values(req)?;
    values.insert("NSIS_X64_SHA256", nsis.sha256.clone());
    values.insert("ARM64_INSTALLER", arm64.unwrap_or_default());
    let installer = substitute(
        &load_template(templates_dir, "winget/Cuotai.Cuotai.installer.yaml.tmpl")?,
        &values,
        "winget installer manifest",
    )?;

    let locale = substitute(
        &load_template(templates_dir, "winget/Cuotai.Cuotai.locale.en-US.yaml.tmpl")?,
        &base_values(req)?,
        "winget locale manifest",
    )?;

    for (name, content) in [
        ("winget version manifest", &version_manifest),
        ("winget installer manifest", &installer),
        ("winget locale manifest", &locale),
    ] {
        validate_yaml(name, content)?;
    }

    Ok(vec![
        Rendered {
            channel: "winget",
            path: PathBuf::from("winget/Cuotai.Cuotai.yaml"),
            content: version_manifest,
        },
        Rendered {
            channel: "winget",
            path: PathBuf::from("winget/Cuotai.Cuotai.installer.yaml"),
            content: installer,
        },
        Rendered {
            channel: "winget",
            path: PathBuf::from("winget/Cuotai.Cuotai.locale.en-US.yaml"),
            content: locale,
        },
    ])
}

fn render_scoop(templates_dir: &Path, req: &RenderRequest) -> Res<Vec<Rendered>> {
    let cli_x64 = need(&req.manifest, "cli-archive", "windows", "x86_64", "scoop")?;
    let arm64 = optional(&req.manifest, "cli-archive", "windows", "aarch64");
    let arm64_block = arm64
        .map(|a| {
            format!(
                ",\n        \"arm64\": {{\n            \"url\": \"{}\",\n            \"hash\": \"{}\"\n        }}",
                download_url(&req.repo, &req.tag, &a.name),
                a.sha256
            )
        })
        .unwrap_or_default();
    let arm64_autoupdate = arm64
        .map(|_| {
            format!(
                ",\n            \"arm64\": {{\n                \"url\": \"https://github.com/{}/releases/download/v$version/cuotai-$version-aarch64-pc-windows-msvc.zip\"\n            }}",
                req.repo
            )
        })
        .unwrap_or_default();

    let mut values = base_values(req)?;
    values.insert("CLI_WINDOWS_X64_SHA256", cli_x64.sha256.clone());
    values.insert("SCOOP_ARM64", arm64_block);
    values.insert("SCOOP_ARM64_AUTOUPDATE", arm64_autoupdate);
    let manifest = substitute(
        &load_template(templates_dir, "scoop/bucket/cuotai.json.tmpl")?,
        &values,
        "scoop manifest",
    )?;
    validate_scoop("scoop manifest", &manifest, &req.manifest.version)?;

    Ok(vec![Rendered {
        channel: "scoop",
        path: PathBuf::from("scoop/bucket/cuotai.json"),
        content: manifest,
    }])
}

fn render_aur(templates_dir: &Path, req: &RenderRequest) -> Res<Vec<Rendered>> {
    let deb = need(&req.manifest, "desktop-deb", "linux", "amd64", "aur")?;
    let arm64 = optional(&req.manifest, "desktop-deb", "linux", "aarch64");
    let arm64_source = arm64
        .map(|a| {
            format!(
                "source_aarch64=(\"{}\")\nsha256sums_aarch64=('{}')\n",
                download_url(&req.repo, &req.tag, &a.name),
                a.sha256
            )
        })
        .unwrap_or_default();

    let mut values = base_values(req)?;
    values.insert("DEB_AMD64_SHA256", deb.sha256.clone());
    values.insert(
        "AUR_ARCHES",
        if arm64.is_some() { " 'aarch64'" } else { "" }.to_string(),
    );
    values.insert("AUR_AARCH64_SOURCE", arm64_source);
    let pkgbuild = substitute(
        &load_template(templates_dir, "aur/PKGBUILD.tmpl")?,
        &values,
        "aur PKGBUILD",
    )?;
    validate_pkgbuild("aur PKGBUILD", &pkgbuild, &req.manifest.version)?;

    // .SRCINFO is generated from the same data so it can never drift from
    // the PKGBUILD (AUR refuses pushes where they disagree).
    let mut srcinfo = String::new();
    srcinfo.push_str("pkgbase = cuotai-bin\n");
    let line = |srcinfo: &mut String, key: &str, value: &str| {
        let _ = writeln!(srcinfo, "\t{key} = {value}");
    };
    line(
        &mut srcinfo,
        "pkgdesc",
        "All your AI quotas, usage limits and costs in one place (desktop app)",
    );
    line(&mut srcinfo, "pkgver", &req.manifest.version);
    line(&mut srcinfo, "pkgrel", "1");
    line(
        &mut srcinfo,
        "url",
        &format!("https://github.com/{}", req.repo),
    );
    line(&mut srcinfo, "arch", "x86_64");
    if arm64.is_some() {
        line(&mut srcinfo, "arch", "aarch64");
    }
    line(&mut srcinfo, "license", "MIT");
    line(&mut srcinfo, "license", "Apache-2.0");
    for dep in ["gtk3", "webkit2gtk-4.1", "libappindicator-gtk3"] {
        line(&mut srcinfo, "depends", dep);
    }
    line(&mut srcinfo, "provides", "cuotai");
    line(&mut srcinfo, "conflicts", "cuotai");
    line(
        &mut srcinfo,
        "source_x86_64",
        &download_url(&req.repo, &req.tag, &deb.name),
    );
    line(&mut srcinfo, "sha256sums_x86_64", &deb.sha256);
    if let Some(a) = arm64 {
        line(
            &mut srcinfo,
            "source_aarch64",
            &download_url(&req.repo, &req.tag, &a.name),
        );
        line(&mut srcinfo, "sha256sums_aarch64", &a.sha256);
    }
    srcinfo.push_str("\npkgname = cuotai-bin\n");

    Ok(vec![
        Rendered {
            channel: "aur",
            path: PathBuf::from("aur/PKGBUILD"),
            content: pkgbuild,
        },
        Rendered {
            channel: "aur",
            path: PathBuf::from("aur/.SRCINFO"),
            content: srcinfo,
        },
    ])
}

/// Renders and validates every channel's manifests. Deterministic: the same
/// inputs produce the same bytes, in the same file order.
pub fn render_all(templates_dir: &Path, req: &RenderRequest) -> Res<Vec<Rendered>> {
    let mut out = Vec::new();
    out.extend(render_homebrew(templates_dir, req)?);
    out.extend(render_winget(templates_dir, req)?);
    out.extend(render_scoop(templates_dir, req)?);
    out.extend(render_aur(templates_dir, req)?);
    Ok(out)
}

/// Writes the rendered set under `out_dir`, creating channel directories.
pub fn write_rendered(out_dir: &Path, rendered: &[Rendered]) -> Res<()> {
    for file in rendered {
        let path = out_dir.join(&file.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, &file.content)?;
    }
    Ok(())
}
