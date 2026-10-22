//! CycloneDX SBOM aggregation (PRD 0021 RF2).
//!
//! The desktop app and the CLI/TUI are two Cargo workspaces, so
//! `cargo-cyclonedx` produces two BOMs with the same `specVersion`. This
//! module merges them into the single `sbom.cdx.json` release asset:
//!
//! * both inputs must be CycloneDX 1.5 JSON documents with a numeric document
//!   `version`, a `metadata.component`, a `components` array and
//!   `dependencies` whose refs resolve to a component or to the root;
//! * components are de-duplicated by `bom-ref`, so a crate shared by both
//!   workspaces is published once;
//! * every dependency edge survives the merge and is sorted, so the same
//!   inputs always produce byte-identical output;
//! * the aggregate `metadata.component` names the release and depends on both
//!   workspace roots, so neither workspace is left unconnected;
//! * `serialNumber` and `metadata.timestamp` are derived from the inputs, not
//!   from a random generator, so a rerun reproduces the same SBOM byte for
//!   byte.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{json, Value};

use crate::{err, Error, Res, Version};

/// CycloneDX specification version this module produces and validates.
pub const SPEC_VERSION: &str = "1.5";

/// `bom-ref` of the synthetic aggregate root component.
pub const ROOT_REF: &str = "cuotai-release";

/// One workspace BOM as read from disk.
struct Input {
    path: String,
    version: Version,
    root_ref: String,
    root_name: String,
    components: Vec<Value>,
    dependencies: Vec<(String, Vec<String>)>,
}

/// Reads, validates and merges workspace BOMs into the aggregate document.
///
/// `timestamp` is the RFC 3339 UTC instant the caller derived from the tagged
/// commit; it is stored as `metadata.timestamp` unchanged.
pub fn merge(paths: &[&Path], version: &Version, timestamp: &str) -> Res<Value> {
    if paths.len() < 2 {
        return err("merging an SBOM needs at least two workspace BOMs");
    }
    if !is_rfc3339_utc(timestamp) {
        return err(format!(
            "timestamp '{timestamp}' is not RFC 3339 UTC (YYYY-MM-DDTHH:MM:SSZ)"
        ));
    }
    let mut inputs = Vec::with_capacity(paths.len());
    for path in paths {
        inputs.push(read_input(path)?);
    }
    // Deterministic order regardless of the order the caller passed.
    inputs.sort_by(|a, b| a.path.cmp(&b.path));

    let mut roots = BTreeSet::new();
    for input in &inputs {
        if !roots.insert(input.root_ref.clone()) {
            return err(format!(
                "{}: metadata component bom-ref '{}' is used by more than one workspace BOM",
                input.path, input.root_ref
            ));
        }
    }

    // Component union: the first input (in sorted path order) that carries a
    // bom-ref wins; the map keeps the aggregate sorted by bom-ref.
    let mut components: BTreeMap<String, Value> = BTreeMap::new();
    for input in &inputs {
        // Each workspace root is a component of the aggregate.
        components.insert(input.root_ref.clone(), input.root_component());
        for component in &input.components {
            components
                .entry(reference_of(component))
                .or_insert_with(|| component.clone());
        }
    }

    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for input in &inputs {
        for (from, targets) in &input.dependencies {
            edges
                .entry(from.clone())
                .or_default()
                .extend(targets.iter().cloned());
        }
    }
    // The aggregate root is the entry point of the release graph.
    edges.insert(ROOT_REF.to_string(), roots.iter().cloned().collect());

    let known: BTreeSet<&String> = components.keys().collect();
    let mut dangling: Vec<&String> = edges
        .iter()
        .flat_map(|(from, targets)| {
            targets
                .iter()
                .chain(std::iter::once(from))
                .filter(|r| !known.contains(*r) && r.as_str() != ROOT_REF)
                .collect::<Vec<_>>()
        })
        .collect();
    dangling.sort();
    dangling.dedup();
    if !dangling.is_empty() {
        let names: Vec<&str> = dangling.iter().map(|name| name.as_str()).collect();
        return err(format!(
            "SBOM dependency references do not resolve to a component: {}",
            names.join(", ")
        ));
    }

    let dependencies: Vec<Value> = edges
        .iter()
        .map(|(from, targets)| {
            json!({ "ref": from, "dependsOn": targets.iter().cloned().collect::<Vec<_>>() })
        })
        .collect();

    Ok(json!({
        "bomFormat": "CycloneDX",
        "specVersion": SPEC_VERSION,
        "serialNumber": serial_number(version, paths, timestamp),
        "version": 1,
        "metadata": {
            "timestamp": timestamp,
            "component": {
                "bom-ref": ROOT_REF,
                "type": "application",
                "name": "Cuotai",
                "version": version.to_string(),
                "description": format!(
                    "Cuotai {} release: aggregate of the CLI/TUI workspace and the desktop workspace",
                    version
                ),
            },
        },
        "components": components.into_values().collect::<Vec<_>>(),
        "dependencies": dependencies,
    }))
}

/// Reads the epoch-seconds of the tagged commit as an RFC 3339 UTC instant.
/// The value comes from `SOURCE_DATE_EPOCH`, which the release job exports
/// from `git show -s --format=%ct HEAD`.
pub fn timestamp_from_epoch(epoch: u64) -> String {
    // Civil date from days since the Unix epoch (Howard Hinnant's algorithm).
    let days = (epoch / 86_400) as i64;
    let seconds = epoch % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    format!(
        "{year:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

fn is_rfc3339_utc(text: &str) -> bool {
    // YYYY-MM-DDTHH:MM:SSZ — the workflow normalizes the tagged commit's
    // fixed timestamp into this UTC form.
    let bytes = text.as_bytes();
    bytes.len() == 20
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && bytes[16] == b':'
        && bytes[19] == b'Z'
        && bytes
            .iter()
            .enumerate()
            .all(|(i, b)| matches!(i, 4 | 7 | 10 | 13 | 16 | 19) || b.is_ascii_digit())
}

impl Input {
    fn root_component(&self) -> Value {
        json!({
            "bom-ref": self.root_ref,
            "type": "application",
            "name": self.root_name,
            "version": self.version.to_string(),
        })
    }
}

fn read_input(path: &Path) -> Res<Input> {
    let shown = || path.display().to_string();
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Msg(format!("cannot read SBOM {}: {e}", shown())))?;
    let value: Value = serde_json::from_str(&text).map_err(|source| Error::Json {
        context: format!("SBOM {}", shown()),
        source,
    })?;
    if value.get("bomFormat").and_then(Value::as_str) != Some("CycloneDX") {
        return err(format!("{}: bomFormat is not 'CycloneDX'", shown()));
    }
    let spec = value
        .get("specVersion")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if spec != SPEC_VERSION {
        return err(format!(
            "{}: specVersion is '{spec}', expected '{SPEC_VERSION}'",
            shown()
        ));
    }
    if value.get("version").and_then(Value::as_u64).is_none() {
        return err(format!(
            "{}: numeric document 'version' is missing",
            shown()
        ));
    }
    let root = value
        .get("metadata")
        .and_then(|metadata| metadata.get("component"))
        .ok_or_else(|| Error::Msg(format!("{}: 'metadata.component' is missing", shown())))?;
    let root_ref = reference_of(root);
    if root_ref.is_empty() {
        return err(format!("{}: metadata component has no bom-ref", shown()));
    }
    let root_name = root
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Msg(format!("{}: metadata component has no name", shown())))?
        .to_string();
    let version_text = root
        .get("version")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Msg(format!("{}: metadata component has no version", shown())))?;
    let version = Version::parse(version_text).map_err(|e| {
        Error::Msg(format!(
            "{}: metadata component version '{version_text}' is not X.Y.Z: {e}",
            shown()
        ))
    })?;

    let mut components = Vec::new();
    let mut refs: BTreeSet<String> = BTreeSet::new();
    if let Some(array) = value.get("components").and_then(Value::as_array) {
        for component in array {
            let reference = reference_of(component);
            if reference.is_empty() {
                return err(format!("{}: a component has no bom-ref", shown()));
            }
            if !refs.insert(reference.clone()) {
                return err(format!(
                    "{}: component bom-ref '{reference}' appears more than once",
                    shown()
                ));
            }
            components.push(component.clone());
        }
    }

    let mut dependencies = Vec::new();
    let mut seen_edges: BTreeSet<String> = BTreeSet::new();
    if let Some(array) = value.get("dependencies").and_then(Value::as_array) {
        for entry in array {
            let from = entry
                .get("ref")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Msg(format!("{}: a dependency entry has no ref", shown())))?
                .to_string();
            if !seen_edges.insert(from.clone()) {
                return err(format!(
                    "{}: dependency ref '{from}' appears more than once",
                    shown()
                ));
            }
            let mut depends_on: BTreeSet<String> = match entry.get("dependsOn") {
                None => BTreeSet::new(),
                Some(Value::Array(targets)) => {
                    let mut parsed = BTreeSet::new();
                    for target in targets {
                        // Every element must be a string: a numeric, null or
                        // object entry is a malformed document, not an entry to
                        // skip, because skipping it would silently drop an
                        // edge from the published graph.
                        let reference = target.as_str().ok_or_else(|| {
                            Error::Msg(format!(
                                "{}: dependency '{from}' has a non-string dependsOn entry ({})",
                                shown(),
                                target
                            ))
                        })?;
                        parsed.insert(reference.to_string());
                    }
                    parsed
                }
                Some(other) => {
                    return err(format!(
                        "{}: dependency '{from}' has a non-array dependsOn ({other})",
                        shown()
                    ))
                }
            };
            depends_on.remove(&from);
            dependencies.push((from, depends_on.into_iter().collect()));
        }
    }

    Ok(Input {
        path: shown(),
        version,
        root_ref,
        root_name,
        components,
        dependencies,
    })
}

fn reference_of(component: &Value) -> String {
    component
        .get("bom-ref")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// A UUID-shaped identifier derived from the inputs, so rerunning the same
/// release reproduces the same `serialNumber` instead of a fresh random one.
fn serial_number(version: &Version, paths: &[&Path], timestamp: &str) -> String {
    use sha2::Digest;
    let mut sorted: Vec<&Path> = paths.to_vec();
    sorted.sort();
    let mut hasher = sha2::Sha256::new();
    hasher.update(version.to_string().as_bytes());
    hasher.update(timestamp.as_bytes());
    for path in sorted {
        hasher.update(path.to_string_lossy().as_bytes());
        hasher.update(std::fs::read(path).unwrap_or_default().as_slice());
    }
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    // RFC 4122 version 4, variant 8..b, as the CycloneDX schema pattern wants:
    // 8-4-4-4-12 hex digits, with nibbles 12 and 16 forced to `4` and `8..b`.
    let version = format!("4{}", &hex[13..16]);
    // Variant 10xx: keep the low two bits of the nibble and set the top two.
    let nibble = hex[16..17]
        .chars()
        .next()
        .and_then(|c| c.to_digit(16))
        .unwrap_or(0);
    let variant = char::from_digit(0b1000 | (nibble & 0b0011), 16).unwrap_or('8');
    let variant_group = format!("{variant}{}", &hex[17..20]);
    format!(
        "urn:uuid:{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        version,
        variant_group,
        &hex[20..32]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TS: &str = "2026-10-21T00:00:00Z";

    fn bom(root: &str, components: &[&str], edges: &[(&str, &[&str])]) -> Value {
        json!({
            "bomFormat": "CycloneDX",
            "specVersion": SPEC_VERSION,
            "version": 1,
            "metadata": {
                "component": {
                    "bom-ref": root,
                    "type": "application",
                    "name": root,
                    "version": "0.67.0",
                }
            },
            "components": components
                .iter()
                .map(|name| json!({"bom-ref": format!("pkg:{name}"), "type": "library", "name": name}))
                .collect::<Vec<_>>(),
            "dependencies": edges
                .iter()
                .map(|(from, targets)| json!({
                    "ref": from,
                    "dependsOn": targets.iter().map(|t| format!("pkg:{t}")).collect::<Vec<_>>(),
                }))
                .collect::<Vec<_>>(),
        })
    }

    #[test]
    fn merge_unions_both_workspaces_under_one_root() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("cli.json");
        let desktop = dir.path().join("desktop.json");
        std::fs::write(
            &cli,
            bom(
                "cuotai-tui",
                &["serde", "rusqlite"],
                &[("pkg:serde", &["rusqlite"])],
            )
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            &desktop,
            bom(
                "cuotai-desktop",
                &["serde", "tauri"],
                &[("pkg:serde", &["tauri"]), ("pkg:tauri", &[])],
            )
            .to_string(),
        )
        .unwrap();
        let merged = merge(
            &[cli.as_path(), desktop.as_path()],
            &Version::parse("0.67.0").unwrap(),
            TS,
        )
        .unwrap();

        let refs: Vec<String> = merged["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["bom-ref"].as_str().unwrap().to_string())
            .collect();
        // `serde` is in both inputs and is published once.
        assert_eq!(
            refs,
            vec![
                "cuotai-desktop",
                "cuotai-tui",
                "pkg:rusqlite",
                "pkg:serde",
                "pkg:tauri",
            ]
        );
        assert_eq!(merged["metadata"]["component"]["bom-ref"], ROOT_REF);

        let dependencies = merged["dependencies"].as_array().unwrap();
        let root = dependencies
            .iter()
            .find(|d| d["ref"] == ROOT_REF)
            .expect("aggregate root entry");
        assert_eq!(root["dependsOn"], json!(["cuotai-desktop", "cuotai-tui"]));
        // Both edges of the shared component survive the merge.
        let serde_entry = dependencies
            .iter()
            .find(|d| d["ref"] == "pkg:serde")
            .unwrap();
        assert_eq!(
            serde_entry["dependsOn"],
            json!(["pkg:rusqlite", "pkg:tauri"])
        );
    }

    #[test]
    fn merge_is_independent_of_input_order() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("cli.json");
        let desktop = dir.path().join("desktop.json");
        std::fs::write(&cli, bom("cuotai-tui", &["serde"], &[]).to_string()).unwrap();
        std::fs::write(&desktop, bom("cuotai-desktop", &["serde"], &[]).to_string()).unwrap();
        let version = Version::parse("0.67.0").unwrap();
        let first = merge(&[cli.as_path(), desktop.as_path()], &version, TS).unwrap();
        let second = merge(&[desktop.as_path(), cli.as_path()], &version, TS).unwrap();
        assert_eq!(first, second);
        assert_eq!(first["serialNumber"], second["serialNumber"]);
    }

    #[test]
    fn merge_rejects_dangling_dependency_reference() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("cli.json");
        let desktop = dir.path().join("desktop.json");
        std::fs::write(&cli, bom("cuotai-tui", &["serde"], &[]).to_string()).unwrap();
        std::fs::write(
            &desktop,
            bom("cuotai-desktop", &[], &[("pkg:ghost", &[])]).to_string(),
        )
        .unwrap();
        let err = merge(
            &[cli.as_path(), desktop.as_path()],
            &Version::parse("0.67.0").unwrap(),
            TS,
        )
        .unwrap_err();
        assert!(err.to_string().contains("pkg:ghost"), "{err}");
    }

    #[test]
    fn merge_rejects_foreign_spec_version_and_shared_root() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("cli.json");
        let desktop = dir.path().join("desktop.json");
        let mut foreign = bom("cuotai-tui", &[], &[]);
        foreign["specVersion"] = json!("1.4");
        std::fs::write(&cli, foreign.to_string()).unwrap();
        std::fs::write(&desktop, bom("cuotai-desktop", &[], &[]).to_string()).unwrap();
        let err = merge(
            &[cli.as_path(), desktop.as_path()],
            &Version::parse("0.67.0").unwrap(),
            TS,
        )
        .unwrap_err();
        assert!(err.to_string().contains("specVersion"), "{err}");

        let mut shared = bom("cuotai-desktop", &[], &[]);
        shared["metadata"]["component"]["bom-ref"] = json!("cuotai-tui");
        std::fs::write(&cli, bom("cuotai-tui", &[], &[]).to_string()).unwrap();
        std::fs::write(&desktop, shared.to_string()).unwrap();
        let err = merge(
            &[cli.as_path(), desktop.as_path()],
            &Version::parse("0.67.0").unwrap(),
            TS,
        )
        .unwrap_err();
        assert!(err.to_string().contains("more than one workspace"), "{err}");
    }

    #[test]
    fn merge_fails_closed_on_non_string_depends_on_entries() {
        // A dependency whose `dependsOn` holds something other than a string is
        // a malformed document. Skipping such an entry would silently drop an
        // edge from the published graph, so the merge must refuse the whole
        // input rather than publish a quietly incomplete SBOM.
        for malformed in [
            json!(42),
            json!(null),
            json!({"ref": "pkg:a"}),
            json!(["pkg:ok", 7]),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let cli = dir.path().join("cli.json");
            let desktop = dir.path().join("desktop.json");
            std::fs::write(&cli, bom("cuotai-tui", &[], &[]).to_string()).unwrap();
            let mut broken: Value = bom("cuotai-desktop", &["serde"], &[]);
            broken["dependencies"]
                .as_array_mut()
                .unwrap()
                .push(json!({"ref": "pkg:serde", "dependsOn": [malformed.clone()]}));
            std::fs::write(&desktop, broken.to_string()).unwrap();

            let result = merge(
                &[cli.as_path(), desktop.as_path()],
                &Version::parse("0.67.0").unwrap(),
                TS,
            );
            assert!(
                result.is_err(),
                "dependsOn entry {malformed} must be rejected, not skipped"
            );
        }
    }

    #[test]
    fn merge_fails_closed_when_depends_on_is_not_an_array() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("cli.json");
        let desktop = dir.path().join("desktop.json");
        std::fs::write(&cli, bom("cuotai-tui", &[], &[]).to_string()).unwrap();
        let mut broken: Value = bom("cuotai-desktop", &["serde"], &[]);
        broken["dependencies"] = json!([{"ref": "pkg:serde", "dependsOn": "pkg:serde"}]);
        std::fs::write(&desktop, broken.to_string()).unwrap();
        assert!(merge(
            &[cli.as_path(), desktop.as_path()],
            &Version::parse("0.67.0").unwrap(),
            TS,
        )
        .is_err());
    }

    #[test]
    fn merge_rejects_non_rfc3339_timestamp_and_single_input() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("cli.json");
        let desktop = dir.path().join("desktop.json");
        std::fs::write(&cli, bom("cuotai-tui", &[], &[]).to_string()).unwrap();
        std::fs::write(&desktop, bom("cuotai-desktop", &[], &[]).to_string()).unwrap();
        let version = Version::parse("0.67.0").unwrap();
        let err = merge(&[cli.as_path(), desktop.as_path()], &version, "yesterday").unwrap_err();
        assert!(err.to_string().contains("RFC 3339"), "{err}");
        let err = merge(&[cli.as_path()], &version, TS).unwrap_err();
        assert!(err.to_string().contains("at least two"), "{err}");
    }

    #[test]
    fn epoch_conversion_matches_known_instants() {
        assert_eq!(timestamp_from_epoch(0), "1970-01-01T00:00:00Z");
        assert_eq!(timestamp_from_epoch(1_767_225_600), "2026-01-01T00:00:00Z");
        assert_eq!(timestamp_from_epoch(1_709_164_800), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn serial_number_is_a_schema_conforming_uuid() {
        let dir = tempfile::tempdir().unwrap();
        let cli = dir.path().join("cli.json");
        let desktop = dir.path().join("desktop.json");
        std::fs::write(&cli, bom("cuotai-tui", &[], &[]).to_string()).unwrap();
        std::fs::write(&desktop, bom("cuotai-desktop", &[], &[]).to_string()).unwrap();
        let version = Version::parse("0.67.0").unwrap();
        let serial = merge(&[cli.as_path(), desktop.as_path()], &version, TS).unwrap()
            ["serialNumber"]
            .as_str()
            .unwrap()
            .to_string();
        // The CycloneDX schema requires `urn:uuid:` plus 8-4-4-4-12 hex digits.
        let serial_body = serial.strip_prefix("urn:uuid:").expect("urn:uuid: prefix");
        let groups: Vec<&str> = serial_body.split('-').collect();
        assert_eq!(
            groups.iter().map(|g| g.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert!(
            groups.iter().all(|g| g
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())),
            "{serial_body} must be lowercase hex"
        );
        // RFC 4122 version 4 and variant 10xx.
        assert!(groups[2].starts_with('4'), "{serial_body} version nibble");
        assert!(
            matches!(groups[3].as_bytes()[0], b'8' | b'9' | b'a' | b'b'),
            "{serial_body} variant nibble"
        );
    }
}
