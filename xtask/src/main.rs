//! `cargo xtask` command line (PRD 0021 RF1–RF4). Thin wrapper over the
//! library; every subcommand exits 1 with a clear message on failure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use xtask::{err, Channel, Error, Res, Version};

const USAGE: &str = "\
Repository automation tasks (PRD 0021).

USAGE:
  cargo xtask <SUBCOMMAND> [OPTIONS]

SUBCOMMANDS:
  release-check       Verify versions, lockfiles, changelog and tag (RF1)
    --repo PATH             repository root (default: .)
    --tag TAG               vX.Y.Z or vX.Y.Z-rc.N to validate against the workspace
    --require-main REF      also require the annotated tag's commit in REF (e.g. origin/main)
    --json-out PATH         write the machine-readable result (version/channel/tag)
    --changelog-out PATH      write the version's changelog section (updater notes)

  release-manifest    Classify artifacts, write manifest.json and SHA256SUMS (RF2)
    --artifacts-dir DIR     flat directory with the collected build artifacts
    --version X.Y.Z         release version (stable names embed it)
    --channel C             stable | prerelease
    --out-dir DIR           where manifest.json and SHA256SUMS go (default: artifacts dir)

  signing-state       Merge per-job fragments into signing-state.json (RF3)
    --version X.Y.Z
    --channel C
    --manifest PATH         manifest.json from release-manifest
    --fragments-dir DIR     directory with the per-job fragment JSON files
    --checksums S           signed | unavailable
    --checksums-status TXT  status/reason for the checksum signature
    --out PATH              signing-state.json output path

  release-notes       Assemble the GitHub Release body (RF4.2)
    --changelog PATH
    --version X.Y.Z
    --manifest PATH
    --signing-state PATH
    --repo SLUG             owner/name (used in verification commands)
    --tag TAG
    --out PATH

  updater-manifest    Emit latest.json when updater signing was available (RF2)
    --version X.Y.Z
    --repo SLUG
    --tag TAG
    --notes-file PATH       plain-text notes (changelog section)
    --pub-date RFC3339      e.g. from `date -u +%Y-%m-%dT%H:%M:%SZ`
    --archive PATH          path to Cuotai.app.tar.gz (sibling .sig required)
    --out PATH

  merge-sbom        Merge the per-workspace CycloneDX BOMs into sbom.cdx.json (RF2)
    --bom PATH             a workspace BOM produced by cargo cyclonedx (repeat, >= 2)
    --version X.Y.Z        release version (the aggregate root component)
    --source-date-epoch N  tagged commit timestamp; used as metadata.timestamp
    --out PATH             sbom.cdx.json output path

  verify-download     Re-check downloaded assets against manifest.json (RF4.4)
    --dir DIR
    --manifest PATH

  changelog-check     CHANGELOG format, PRD references and plan links (RF7.4, RF7.8)
    --repo PATH             repository root (default: .)

  changelog-gate      Pull-request gate: feat/fix in crates/ or apps/ needs the changelog
    --repo PATH             repository root (default: .)
    --base REF              base of the pull request (default: HEAD~1)
    --head REF              head of the pull request (default: HEAD)

  changelog-draft     Draft the [Unreleased] section from the commits (RF7.3)
    --repo PATH             repository root (default: .)
    --since REF             default: the last v* tag, else the last version bump

  package-manifests  Render and validate every package-manager channel (RF5.6)
    --release-manifest PATH  manifest.json, or a directory containing it
    --repo SLUG              owner/name used for the release download URLs
    --tag TAG                vX.Y.Z (must match the manifest version)
    --templates-dir DIR      channel templates (default: packaging)
    --out DIR                output directory for the rendered manifests
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Res<()> {
    let Some(sub) = args.first() else {
        eprint!("{USAGE}");
        return err("missing subcommand");
    };
    let opts = Opts::parse(&args[1..])?;
    match sub.as_str() {
        "release-check" => release_check(&opts),
        "release-manifest" => release_manifest(&opts),
        "signing-state" => signing_state(&opts),
        "release-notes" => release_notes(&opts),
        "updater-manifest" => updater_manifest(&opts),
        "merge-sbom" => merge_sbom(&opts),
        "verify-download" => verify_download(&opts),
        "changelog-check" => changelog_check(&opts),
        "changelog-gate" => changelog_gate(&opts),
        "changelog-draft" => changelog_draft(&opts),
        "package-manifests" => package_manifests(&opts),
        "help" | "--help" | "-h" => {
            print!("{USAGE}");
            Ok(())
        }
        other => {
            eprint!("{USAGE}");
            err(format!("unknown subcommand '{other}'"))
        }
    }
}

fn release_check(opts: &Opts) -> Res<()> {
    let repo = opts.path("repo").unwrap_or_else(|| PathBuf::from("."));
    let tag = opts.get("tag");
    let require_main = opts.get("require-main");
    let report = xtask::check::run(&repo, tag.as_deref(), require_main.as_deref())?;
    let fallback = format!("v{}", report.version);
    let json = report.to_json(&fallback);
    println!(
        "release check passed: version {} ({}){}",
        json.version,
        json.channel.as_str(),
        tag.map(|t| format!(", tag {t}")).unwrap_or_default()
    );
    if let Some(path) = opts.get("json-out") {
        write_json(Path::new(&path), &json)?;
    }
    if let Some(path) = opts.get("changelog-out") {
        std::fs::write(
            &path,
            format!(
                "## [{}] - {}\n\n{}\n",
                report.version, report.changelog_date, report.changelog_body
            ),
        )?;
    }
    Ok(())
}

fn release_manifest(opts: &Opts) -> Res<()> {
    let dir = PathBuf::from(opts.need("artifacts-dir")?);
    let version = opts.need("version")?;
    let channel = Channel::parse(&opts.need("channel")?)?;
    Version::parse(&version)?;
    let out_dir = opts.path("out-dir").unwrap_or_else(|| dir.clone());
    let (manifest, sums) = xtask::manifest::build_manifest(&dir, &version, channel)?;
    std::fs::write(out_dir.join("SHA256SUMS"), sums)?;
    write_json(&out_dir.join("manifest.json"), &manifest)?;
    println!(
        "manifest: {} artifacts, {} optional not produced",
        manifest.artifacts.len(),
        manifest.missing_optional.len()
    );
    Ok(())
}

fn signing_state(opts: &Opts) -> Res<()> {
    let version = opts.need("version")?;
    Version::parse(&version)?;
    let channel = Channel::parse(&opts.need("channel")?)?;
    let manifest = xtask::manifest::load(&PathBuf::from(opts.need("manifest")?))?;
    let fragments_dir = PathBuf::from(opts.need("fragments-dir")?);
    let mut fragment_files = Vec::new();
    for entry in std::fs::read_dir(&fragments_dir)
        .map_err(|e| Error::Msg(format!("cannot read {}: {e}", fragments_dir.display())))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
            continue;
        }
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return err(format!(
                "{}: signing fragment is not a regular file",
                path.display()
            ));
        }
        fragment_files.push(path);
    }
    fragment_files.sort();
    let fragments = fragment_files
        .iter()
        .map(|p| xtask::signing::load_fragment(p))
        .collect::<Res<Vec<_>>>()?;
    let checksums = match opts.need("checksums")?.as_str() {
        "signed" => xtask::signing::ChecksumSigning {
            signed: true,
            method: xtask::signing::ChecksumSigningMethod::SigstoreKeyless,
            status: "signed by the GitHub Actions OIDC identity (Sigstore keyless)".into(),
        },
        "unavailable" => xtask::signing::ChecksumSigning {
            signed: false,
            method: xtask::signing::ChecksumSigningMethod::None,
            status: opts
                .get("checksums-status")
                .unwrap_or_else(|| "not signed in this run".to_string()),
        },
        other => {
            return err(format!(
                "--checksums must be 'signed' or 'unavailable', got '{other}'"
            ))
        }
    };
    let state = xtask::signing::merge(&version, channel, fragments, &manifest, checksums)?;
    let out = PathBuf::from(opts.need("out")?);
    write_json(&out, &state)?;
    println!(
        "signing state: {} artifacts ({} signed), checksums signed: {}, updater available: {}",
        state.artifacts.len(),
        state.artifacts.iter().filter(|a| a.signed).count(),
        state.checksums.signed,
        state.updater.available
    );
    Ok(())
}

fn release_notes(opts: &Opts) -> Res<()> {
    let changelog_path = PathBuf::from(opts.need("changelog")?);
    let version = Version::parse(&opts.need("version")?)?;
    let manifest = xtask::manifest::load(&PathBuf::from(opts.need("manifest")?))?;
    let state = xtask::signing::load(&PathBuf::from(opts.need("signing-state")?))?;
    let repo = opts.need("repo")?;
    let changelog = std::fs::read_to_string(&changelog_path)
        .map_err(|e| Error::Msg(format!("cannot read {}: {e}", changelog_path.display())))?;
    let (date, body) = xtask::check::changelog_section(&changelog, &version)?;
    let section = format!("## [{version}] - {date}\n\n{body}");
    let rendered = xtask::notes::render_body(&section, &state, &manifest, &repo);
    let out = PathBuf::from(opts.need("out")?);
    std::fs::write(&out, rendered)?;
    println!("release notes written to {}", out.display());
    Ok(())
}

fn updater_manifest(opts: &Opts) -> Res<()> {
    let version = Version::parse(&opts.need("version")?)?;
    let repo = opts.need("repo")?;
    let tag = opts.need("tag")?;
    let parsed_tag = xtask::parse_tag(&tag)?;
    if parsed_tag.version != version {
        return err(format!(
            "tag {tag} is for version {}, not updater version {version}",
            parsed_tag.version
        ));
    }
    let notes_path = PathBuf::from(opts.need("notes-file")?);
    let notes = std::fs::read_to_string(&notes_path)
        .map_err(|e| Error::Msg(format!("cannot read {}: {e}", notes_path.display())))?;
    let pub_date = opts.need("pub-date")?;
    let archive = PathBuf::from(opts.need("archive")?);
    let json = xtask::updater::latest_json(&version, &notes, &pub_date, &repo, &tag, &archive)?;
    let out = PathBuf::from(opts.need("out")?);
    std::fs::write(&out, serde_json::to_string_pretty(&json)? + "\n")?;
    println!("updater manifest written to {}", out.display());
    Ok(())
}

fn merge_sbom(opts: &Opts) -> Res<()> {
    let version = Version::parse(&opts.need("version")?)?;
    let boms = opts.all("bom");
    if boms.len() < 2 {
        return err("--bom must be repeated once per workspace (at least twice)");
    }
    let paths: Vec<PathBuf> = boms.iter().map(PathBuf::from).collect();
    let borrowed: Vec<&Path> = paths.iter().map(PathBuf::as_path).collect();
    let epoch: u64 = opts
        .need("source-date-epoch")?
        .trim()
        .parse()
        .map_err(|_| {
            Error::Msg("--source-date-epoch must be seconds since the Unix epoch".into())
        })?;
    let timestamp = xtask::sbom::timestamp_from_epoch(epoch);
    let merged = xtask::sbom::merge(&borrowed, &version, &timestamp)?;
    let out = PathBuf::from(opts.need("out")?);
    write_json(&out, &merged)?;
    println!(
        "aggregate SBOM written to {} ({} components, {} dependency entries)",
        out.display(),
        merged["components"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default(),
        merged["dependencies"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default()
    );
    Ok(())
}

fn verify_download(opts: &Opts) -> Res<()> {
    let dir = PathBuf::from(opts.need("dir")?);
    let manifest = xtask::manifest::load(&PathBuf::from(opts.need("manifest")?))?;
    let count = xtask::manifest::verify_download(&dir, &manifest)?;
    println!("verified {count} downloaded artifacts against the manifest");
    Ok(())
}

fn changelog_check(opts: &Opts) -> Res<()> {
    let repo = opts.path("repo").unwrap_or_else(|| PathBuf::from("."));
    let text = std::fs::read_to_string(repo.join("CHANGELOG.md"))?;
    xtask::plan_links::run(&repo, &text)?;
    println!(
        "changelog check passed: {} released sections, format, PRD references and plan links agree",
        xtask::changelog::parse(&text)
            .map_err(|e| Error::Msg(e.to_string()))?
            .releases
            .len()
    );
    Ok(())
}

fn changelog_gate(opts: &Opts) -> Res<()> {
    let repo = opts.path("repo").unwrap_or_else(|| PathBuf::from("."));
    let base = opts.get("base").unwrap_or_else(|| "HEAD~1".to_string());
    let head = opts.get("head").unwrap_or_else(|| "HEAD".to_string());
    xtask::changelog_gate::run(&repo, &base, &head)?;
    println!("changelog gate passed: every feat/fix in {base}..{head} is accounted for");
    Ok(())
}

fn changelog_draft(opts: &Opts) -> Res<()> {
    let repo = opts.path("repo").unwrap_or_else(|| PathBuf::from("."));
    let since = opts.get("since");
    print!("{}", xtask::draft::run(&repo, since.as_deref())?);
    Ok(())
}

fn package_manifests(opts: &Opts) -> Res<()> {
    let (manifest, signing) =
        xtask::packages::load_inputs(&PathBuf::from(opts.need("release-manifest")?))?;
    let templates_dir = opts
        .path("templates-dir")
        .unwrap_or_else(|| PathBuf::from("packaging"));
    let out = PathBuf::from(opts.need("out")?);
    let request = xtask::packages::RenderRequest {
        repo: opts.need("repo")?,
        tag: opts.need("tag")?,
        manifest,
        signing,
    };
    let rendered = xtask::packages::render_all(&templates_dir, &request)?;
    xtask::packages::write_rendered(&out, &rendered)?;
    for file in &rendered {
        println!("rendered {} ({})", file.path.display(), file.channel);
    }
    println!(
        "rendered {} package-manager manifests for {} ({})",
        rendered.len(),
        request.tag,
        if request.signing.is_some() {
            "signing state read"
        } else {
            "no signing state: unsigned caveats rendered"
        }
    );
    Ok(())
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Res<()> {
    let text = serde_json::to_string_pretty(value)? + "\n";
    std::fs::write(path, text)?;
    Ok(())
}

/// `--key value` options; everything else is rejected so typos never pass
/// silently. A repeated option (`--bom`) keeps all its values in order.
struct Opts(BTreeMap<String, Vec<String>>);

impl Opts {
    fn parse(args: &[String]) -> Res<Opts> {
        let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut i = 0;
        while i < args.len() {
            let key = &args[i];
            let Some(name) = key.strip_prefix("--") else {
                return err(format!(
                    "unexpected argument '{key}' (options are --key value)"
                ));
            };
            let value = args
                .get(i + 1)
                .ok_or_else(|| Error::Msg(format!("option '{key}' needs a value")))?;
            if value.starts_with("--") {
                return err(format!("option '{key}' needs a value"));
            }
            map.entry(name.to_string()).or_default().push(value.clone());
            i += 2;
        }
        Ok(Opts(map))
    }

    fn get(&self, key: &str) -> Option<String> {
        self.0.get(key).and_then(|values| values.last().cloned())
    }

    /// Every value of a repeated option, in command-line order.
    fn all(&self, key: &str) -> Vec<String> {
        self.0.get(key).cloned().unwrap_or_default()
    }

    fn need(&self, key: &str) -> Res<String> {
        self.get(key)
            .ok_or_else(|| Error::Msg(format!("missing required option --{key}")))
    }

    fn path(&self, key: &str) -> Option<PathBuf> {
        self.get(key).map(PathBuf::from)
    }
}
