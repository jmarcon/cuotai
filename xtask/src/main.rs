//! `cargo xtask` command line (PRD 0021 RF1–RF4). Thin wrapper over the
//! library; every subcommand exits 1 with a clear message on failure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use xtask::updater::PlatformArchive;
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
    --platform KEY=ARCHIVE  signed updater archive per manifest key (repeat)
                            KEY is {os}-{arch}-{installer}: darwin-{aarch64,x86_64}-app,
                            windows-{x86_64,aarch64}-{nsis,msi},
                            linux-{x86_64,aarch64}-{appimage,deb,rpm}
                            ARCHIVE needs its sibling .sig; the generic
                            {os}-{arch} key is added automatically
    --require KEY           platform key a complete release must carry (repeat)
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

  changelog-gate      Commit gate (PRD 0039 RF2): conventional header, Changelog:
                      trailer, no version/CHANGELOG edits outside a release commit
    --repo PATH             repository root (default: .)
    --base REF              first excluded commit (default: HEAD~1)
    --before SHA            a push's previous tip instead of --base; fetched when
                            missing, and the gate fails closed when it cannot be
    --head REF              last checked commit (default: HEAD)

  changelog-draft     Print the next version and its would-be section (PRD 0039 RF4.8)
    --repo PATH             repository root (default: .)
    --date YYYY-MM-DD       section date (default: today, UTC)

  access-staleness    Warn about stale access evidence (PRD 0028 RF6.2)
    --repo PATH             repository root (default: .)
    --days N                warn past N days (default: 90)
    --fail-after N          also fail past N days (the non-required job; e.g. 180)
    --today YYYY-MM-DD      date to compare against (default: today, UTC)

  release prepare     Compute the next version and write the release (PRD 0039 RF4)
    --repo PATH             repository root (default: .)
    --version X.Y.Z         override the computed version (RF3.4)
    --date YYYY-MM-DD       section date (default: today, UTC)
    --dry-run               print the version, rationale and diffs; write nothing
    --branch                commit 'chore(release): X.Y.Z' on a new release/vX.Y.Z
    --refresh               re-render the prepared version's section (after an rc)
    --check                 verify a release commit instead (RF4.7)
    --rev REF               commit checked by --check (default: HEAD)

  release tag         Create the local annotated tag of the release commit (RF5.2)
    --repo PATH             repository root (default: .)
    --rc                    tag vX.Y.Z-rc.N instead of vX.Y.Z
    --main REF              branch the commit must be on (default: origin/main)

  package-manifests  Render and validate every package-manager channel (RF5.6)
    --release-manifest PATH  manifest.json, or a directory containing it
    --repo SLUG              owner/name used for the release download URLs
    --tag TAG                vX.Y.Z (must match the manifest version)
    --templates-dir DIR      channel templates (default: packaging)
    --out DIR                output directory for the rendered manifests

  workflow-check      Enforce GitHub Actions least privilege and cost guardrails (RF9)
    --workflow PATH         workflow to inspect (repeat; default: every .yml/.yaml workflow)

  external-rules-check
                       Compare external policy hashes and GitHub Action releases (RF9.3)
    --repo PATH             repository root (default: .)
    --baseline PATH         committed baseline (default: .github/external-rules-baseline.json)
    --json-out PATH         machine-readable report
    --markdown-out PATH     issue-ready report
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
    if sub == "release" {
        return release(&args[1..]);
    }
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
        "access-staleness" => access_staleness(&opts),
        "package-manifests" => package_manifests(&opts),
        "workflow-check" => workflow_check(&opts),
        "external-rules-check" => external_rules_check(&opts),
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

    let mut archives: Vec<xtask::updater::PlatformArchive> = Vec::new();
    for entry in opts.all("platform") {
        let (key, path) = entry
            .split_once('=')
            .ok_or_else(|| Error::Msg(format!("--platform '{entry}' is not 'KEY=ARCHIVE'")))?;
        let archive = PlatformArchive::new(key, std::path::Path::new(path))?;
        match archives
            .iter_mut()
            .find(|other| other.archive == archive.archive)
        {
            // The same file answering for several keys: one universal app for
            // both macOS architectures, one AppImage for every Linux key.
            Some(existing) => existing.merge(archive)?,
            None => archives.push(archive),
        }
    }
    let required = opts.all("require");
    let required: Vec<&str> = required.iter().map(String::as_str).collect();
    let json = xtask::updater::latest_json(
        &version, &notes, &pub_date, &repo, &tag, &archives, &required,
    )?;
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
    let changelog = xtask::changelog::parse(&text).map_err(|e| Error::Msg(e.to_string()))?;
    let history = changelog
        .pre_release
        .as_ref()
        .map(|h| format!(" plus the pre-release history ({})", h.range()))
        .unwrap_or_default();
    println!(
        "changelog check passed: {} released sections{history}, format, PRD references and plan links agree",
        changelog.releases.len()
    );
    Ok(())
}

fn changelog_gate(opts: &Opts) -> Res<()> {
    let repo = opts.path("repo").unwrap_or_else(|| PathBuf::from("."));
    let head = opts.get("head").unwrap_or_else(|| "HEAD".to_string());
    let base = match (opts.get("base"), opts.get("before")) {
        (Some(_), Some(_)) => return err("--base and --before exclude each other"),
        (None, Some(before)) => xtask::changelog_gate::push_base(&repo, &before)?,
        (Some(base), None) => base,
        (None, None) => "HEAD~1".to_string(),
    };
    let checked = xtask::changelog_gate::run(&repo, &base, &head)?;
    println!(
        "commit gate passed: {checked} commit(s) in {base}..{head} follow the commit contract"
    );
    Ok(())
}

fn changelog_draft(opts: &Opts) -> Res<()> {
    let repo = opts.path("repo").unwrap_or_else(|| PathBuf::from("."));
    let date = opts
        .get("date")
        .unwrap_or_else(xtask::release::prepare::today_utc);
    print!("{}", xtask::release::prepare::draft(&repo, &date)?);
    Ok(())
}

fn access_staleness(opts: &Opts) -> Res<()> {
    let repo = opts.path("repo").unwrap_or_else(|| PathBuf::from("."));
    let number = |key: &str| -> Res<Option<i64>> {
        opts.get(key)
            .map(|v| {
                v.parse::<i64>()
                    .map_err(|_| Error::Msg(format!("--{key} needs a number of days")))
            })
            .transpose()
    };
    let today = opts
        .get("today")
        .unwrap_or_else(xtask::release::prepare::today_utc);
    let days = number("days")?.unwrap_or(90);
    print!(
        "{}",
        xtask::access::run(&repo, &today, days, number("fail-after")?)?
    );
    Ok(())
}

/// `release prepare|tag` (PRD 0039 RF4/RF5): `--key value` options plus
/// boolean flags.
fn release(args: &[String]) -> Res<()> {
    const FLAGS: [&str; 5] = ["--dry-run", "--branch", "--refresh", "--check", "--rc"];
    let Some(action) = args.first() else {
        return err("usage: cargo xtask release prepare|tag [OPTIONS]");
    };
    let flags: Vec<&str> = args[1..]
        .iter()
        .map(String::as_str)
        .filter(|a| FLAGS.contains(a))
        .collect();
    let rest: Vec<String> = args[1..]
        .iter()
        .filter(|a| !FLAGS.contains(&a.as_str()))
        .cloned()
        .collect();
    let opts = Opts::parse(&rest)?;
    // Every flag and option is checked against the action, so a
    // safety-looking flag the action ignores (`tag --dry-run`) is an error.
    let (allowed_flags, allowed_options): (&[&str], &[&str]) = match action.as_str() {
        "prepare" => (
            &["--dry-run", "--branch", "--refresh", "--check"],
            &["repo", "version", "date", "rev"],
        ),
        "tag" => (&["--rc"], &["repo", "main"]),
        other => {
            return err(format!(
                "unknown release action '{other}' (expected prepare or tag)"
            ))
        }
    };
    if let Some(bad) = flags.iter().find(|f| !allowed_flags.contains(f)) {
        return err(format!(
            "release {action} does not accept {bad} (allowed: {})",
            allowed_flags.join(", ")
        ));
    }
    if let Some(bad) = opts
        .0
        .keys()
        .find(|k| !allowed_options.contains(&k.as_str()))
    {
        return err(format!("release {action} does not accept --{bad}"));
    }
    let flag = |name: &str| flags.contains(&name);
    if flag("--check") {
        if let Some(bad) = ["--dry-run", "--branch", "--refresh"]
            .into_iter()
            .find(|f| flag(f))
        {
            return err(format!(
                "--check verifies a release commit; it cannot be combined with {bad}"
            ));
        }
        if let Some(bad) = ["version", "date"]
            .into_iter()
            .find(|k| opts.0.contains_key(*k))
        {
            return err(format!("--check cannot be combined with --{bad}"));
        }
    } else if opts.0.contains_key("rev") {
        return err("--rev is only valid with --check");
    }
    let repo = opts.path("repo").unwrap_or_else(|| PathBuf::from("."));
    match action.as_str() {
        "prepare" if flag("--check") => {
            let rev = opts.get("rev").unwrap_or_else(|| "HEAD".to_string());
            let outcome = xtask::release::check_release(&repo, &rev)?;
            println!(
                "release check passed: {} covers the {} commit(s) since {}",
                outcome.version,
                outcome.commits,
                outcome.base.describe()
            );
            Ok(())
        }
        "prepare" => release_prepare(&repo, &opts, &flag),
        "tag" => {
            let main_ref = opts
                .get("main")
                .unwrap_or_else(|| "origin/main".to_string());
            let tagged = xtask::release::tag::run(&repo, flag("--rc"), &main_ref)?;
            println!(
                "created the annotated tag {} on {}. Push it to start release.yml:\n  git push origin {}",
                tagged.tag,
                &tagged.sha[..7],
                tagged.tag
            );
            Ok(())
        }
        other => err(format!(
            "unknown release action '{other}' (expected prepare or tag)"
        )),
    }
}

fn release_prepare(repo: &Path, opts: &Opts, flag: &dyn Fn(&str) -> bool) -> Res<()> {
    let version = opts
        .get("version")
        .map(|v| Version::parse(&v))
        .transpose()?;
    let date = opts
        .get("date")
        .unwrap_or_else(xtask::release::prepare::today_utc);
    let options = xtask::release::prepare::Options {
        version,
        date,
        dry_run: flag("--dry-run"),
        branch: flag("--branch"),
        refresh: flag("--refresh"),
        verify_lockfiles: true,
    };
    if options.dry_run && options.branch {
        return err("--dry-run and --branch exclude each other");
    }
    warn_if_not_main(repo);
    let Some(outcome) = xtask::release::prepare::run(repo, &options)? else {
        let message = format!(
            "{} (pass --version X.Y.Z to release anyway)",
            xtask::release::prepare::idle_status(repo)?
        );
        if options.dry_run {
            println!("{message}");
            return Ok(());
        }
        return err(message);
    };
    for warning in &outcome.warnings {
        eprintln!("warning: {warning}");
    }
    println!(
        "next version: {} ({} bump from {})",
        outcome.version,
        outcome.bump.as_str(),
        outcome.base.describe()
    );
    for line in &outcome.rationale {
        println!("  {line}");
    }
    println!("\n{}", outcome.section);
    if options.dry_run {
        for change in &outcome.changes {
            print!(
                "{}",
                xtask::release::render::unified_diff(&change.path, &change.old, &change.new)
            );
        }
        println!("\ndry run: nothing was written");
        return Ok(());
    }
    println!(
        "wrote {} file(s); release-check and changelog-check passed",
        outcome.changes.len()
    );
    match &outcome.branch {
        Some(branch) => println!(
            "committed on {branch}. Next:\n  git push -u origin {branch}\n  gh pr create --base main --head {branch} --title \"chore(release): {v}\" --body \"$(git log -1 --format=%b)\"\nAfter the rebase-merge: git switch main && git pull --ff-only && cargo xtask release tag --rc",
            v = outcome.version
        ),
        None => println!(
            "review the changes, then commit them as 'chore(release): {}' (or rerun with --branch)",
            outcome.version
        ),
    }
    Ok(())
}

/// RF4.1: prepare runs on an up-to-date main; anything else is a warning.
fn warn_if_not_main(repo: &Path) {
    let head = xtask::git::run(repo, &["rev-parse", "HEAD"]);
    let main = xtask::git::run(repo, &["rev-parse", "--verify", "--quiet", "origin/main"]);
    if let (Ok(head), Ok(main)) = (head, main) {
        if head.trim() != main.trim() {
            eprintln!(
                "warning: HEAD is not origin/main; a release is normally prepared from an up-to-date main"
            );
        }
    }
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

fn external_rules_check(opts: &Opts) -> Res<()> {
    let repo = opts.path("repo").unwrap_or_else(|| PathBuf::from("."));
    let baseline = opts
        .path("baseline")
        .unwrap_or_else(|| repo.join(".github/external-rules-baseline.json"));
    let github_token = std::env::var("GITHUB_TOKEN").ok();
    let report = xtask::external_rules::run(&repo, &baseline, github_token.as_deref())?;
    if let Some(path) = opts.get("json-out") {
        write_json(Path::new(&path), &report)?;
    }
    if let Some(path) = opts.get("markdown-out") {
        std::fs::write(&path, report.markdown())?;
    }
    println!(
        "external rules check: {} policy pages, {} action pins, {}",
        report.policies.len(),
        report.actions.len(),
        if report.needs_attention {
            "review required"
        } else {
            "no drift detected"
        }
    );
    for policy in &report.policies {
        println!("policy {:?}: {:?}", policy.name, policy.status);
    }
    for action in &report.actions {
        println!("action {}: {:?}", action.action, action.status);
    }
    Ok(())
}

fn workflow_check(opts: &Opts) -> Res<()> {
    let mut workflows = opts.all("workflow");
    if workflows.is_empty() {
        workflows = xtask::external_rules::workflow_files(Path::new(".github/workflows"))?
            .into_iter()
            .map(|path| path.display().to_string())
            .collect();
    }
    for workflow in &workflows {
        xtask::workflow::check_file(Path::new(workflow))?;
        println!("workflow policy passed: {workflow}");
    }
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
