//! PRD 0014 RF2/RF3: the macOS widget extension, its packaging and the core
//! writer agree, on every system CI runs on (no Xcode needed).
//!
//! `apps/desktop/widgets/macos/widget-contract.json` records what macOS
//! stores for every widget a user placed: its kind, the kind's configuration
//! intent, the intent's parameters and the enum cases they hold. A kind that
//! changes its intent, or a parameter or case that is renamed or removed,
//! leaves every widget already placed as an empty card (0.87.0 moved the
//! History kind from `AccountIntent` to `HistoryIntent`). The contract is
//! append-only, and this test ties it to the Swift sources; `check-appex.sh`
//! ties it to the built extension's App Intents metadata.
//!
//! It also keeps the file the extension reads, the sandbox exception that
//! lets it read that file and the path the core writes the same, and the
//! extension's bundle identity loadable inside the app's bundle.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use cuotai_core::widget_snapshot::MACOS_PATH;
use serde_json::Value;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn widget_dir() -> PathBuf {
    repo().join("apps/desktop/widgets/macos")
}

fn read(path: PathBuf) -> String {
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn contract() -> Value {
    serde_json::from_str(&read(widget_dir().join("widget-contract.json")))
        .expect("widget-contract.json is JSON")
}

/// Every Swift source of the extension, by file name.
fn sources() -> BTreeMap<String, String> {
    let dir = widget_dir().join("Sources");
    let mut out = BTreeMap::new();
    for entry in std::fs::read_dir(&dir).expect("the widget sources") {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "swift") {
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            out.insert(name, read(path));
        }
    }
    assert!(!out.is_empty(), "no Swift sources in {}", dir.display());
    out
}

/// The text between the first pair of double quotes after `marker`.
fn quoted_after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let rest = &text[text.find(marker)? + marker.len()..];
    let start = rest.find('"')? + 1;
    let len = rest[start..].find('"')?;
    Some(&rest[start..start + len])
}

/// The identifier right after `marker` (letters, digits, underscores).
fn ident_after<'a>(text: &'a str, marker: &str) -> Option<&'a str> {
    let rest = text[text.find(marker)? + marker.len()..].trim_start();
    let len = rest
        .find(|c: char| !(c.is_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    (len > 0).then(|| &rest[..len])
}

/// The body of each top-level `struct`/`enum` whose header contains
/// `conformance`, by type name: the lines up to the closing `}` in column 0.
fn bodies(conformance: &str) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    for text in sources().values() {
        let mut lines = text.lines();
        while let Some(line) = lines.next() {
            let header = (line.starts_with("struct ") || line.starts_with("enum "))
                && line.contains(conformance);
            if !header {
                continue;
            }
            let name = ident_after(
                line,
                if line.starts_with("struct ") {
                    "struct "
                } else {
                    "enum "
                },
            )
            .unwrap()
            .to_owned();
            let body = lines
                .by_ref()
                .take_while(|l| !l.starts_with('}'))
                .map(str::to_owned)
                .collect();
            out.insert(name, body);
        }
    }
    out
}

fn strings(value: &Value) -> Vec<String> {
    value
        .as_array()
        .expect("a list")
        .iter()
        .map(|v| v.as_str().expect("a string").to_owned())
        .collect()
}

#[test]
fn every_widget_kind_keeps_its_configuration_intent() {
    let mut kinds = BTreeMap::new();
    for (file, text) in sources() {
        let Some(kind) = quoted_after(&text, "static let kind =") else {
            continue;
        };
        let intent = ident_after(&text, "AppIntentConfiguration(kind: Self.kind, intent:")
            .unwrap_or_else(|| panic!("{file}: a widget kind without an AppIntentConfiguration"));
        assert!(
            kinds.insert(kind.to_owned(), intent.to_owned()).is_none(),
            "{file}: kind {kind} is declared twice"
        );
    }
    let expected: BTreeMap<String, String> = contract()["kinds"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| (k.clone(), v.as_str().unwrap().to_owned()))
        .collect();
    assert_eq!(
        kinds, expected,
        "a placed widget keeps its kind and its intent: never change or remove either \
         (add a new kind instead), and list a new kind in widget-contract.json"
    );
}

#[test]
fn intents_keep_every_parameter_and_the_contract_lists_every_new_one() {
    let found: BTreeMap<String, Vec<String>> = bodies("WidgetConfigurationIntent")
        .into_iter()
        .map(|(name, body)| {
            let mut params = Vec::new();
            let mut pending = false;
            for line in body {
                let line = line.trim();
                if line.starts_with("@Parameter") {
                    pending = true;
                } else if pending {
                    if let Some(param) = ident_after(line, "var ") {
                        params.push(param.to_owned());
                        pending = false;
                    }
                }
            }
            (name, params)
        })
        .collect();
    let contract = contract();
    let expected = contract["intents"].as_object().unwrap();
    for (intent, params) in expected {
        let params = strings(params);
        let built = found
            .get(intent)
            .unwrap_or_else(|| panic!("intent {intent} is gone: placed widgets still use it"));
        for param in &params {
            assert!(
                built.contains(param),
                "{intent} lost its parameter {param}: placed widgets saved a value under that name"
            );
        }
        for param in built {
            assert!(
                params.contains(param),
                "{intent} has a new parameter {param}: append it to widget-contract.json"
            );
        }
    }
    for intent in found.keys() {
        assert!(
            expected.contains_key(intent),
            "{intent} is new: list it in widget-contract.json"
        );
    }
}

#[test]
fn enums_and_entities_the_intents_store_keep_their_identifiers() {
    let contract = contract();
    let found: BTreeMap<String, BTreeSet<String>> = bodies("AppEnum")
        .into_iter()
        .map(|(name, body)| {
            let cases = body
                .iter()
                .filter_map(|line| line.trim_start().strip_prefix("case "))
                .filter_map(|rest| ident_after(rest, ""))
                .map(str::to_owned)
                .collect();
            (name, cases)
        })
        .collect();
    let expected = contract["enums"].as_object().unwrap();
    for (name, cases) in expected {
        let cases: BTreeSet<String> = strings(cases).into_iter().collect();
        let built = found
            .get(name)
            .unwrap_or_else(|| panic!("enum {name} is gone: placed widgets still use it"));
        // `switch` arms (`case .all:`) start with a dot and are skipped.
        assert_eq!(
            built, &cases,
            "{name}: cases are append-only; list a new case in widget-contract.json"
        );
    }
    for name in found.keys() {
        assert!(
            expected.contains_key(name),
            "enum {name} is new: list it in widget-contract.json"
        );
    }
    let entities: BTreeSet<String> = bodies("AppEntity").into_keys().collect();
    for entity in strings(&contract["entities"]) {
        assert!(
            entities.contains(&entity),
            "entity {entity} is gone: placed widgets still use it"
        );
    }
}

#[test]
fn the_extension_reads_the_file_the_core_writes_and_its_sandbox_allows() {
    let contract = contract();
    assert_eq!(contract["snapshot_home_relative_path"], MACOS_PATH);

    // The reader: `<home>/<directory>/<fileName>` in CuotaiSnapshot.swift.
    let reader = read(widget_dir().join("Sources/CuotaiSnapshot.swift"));
    let directory =
        quoted_after(&reader, ".appendingPathComponent(").expect("the reader's directory");
    let file = quoted_after(&reader, "static let fileName =").expect("the reader's file name");
    assert_eq!(
        format!("{directory}/{file}"),
        MACOS_PATH,
        "the extension reads another file"
    );

    // The sandbox: exactly that file, read-only, relative to the home.
    let entitlements = read(widget_dir().join("CuotaiWidgets.entitlements"));
    let sandbox = "<key>com.apple.security.app-sandbox</key>";
    let after = &entitlements[entitlements
        .find(sandbox)
        .expect("the extension is sandboxed")
        + sandbox.len()..];
    assert!(
        after.trim_start().starts_with("<true/>"),
        "WidgetKit needs the sandbox on"
    );
    let key =
        "<key>com.apple.security.temporary-exception.files.home-relative-path.read-only</key>";
    let array = &entitlements[entitlements
        .find(key)
        .expect("the read exception for the snapshot")..];
    let array = &array[..array.find("</array>").expect("its path list")];
    let paths: Vec<&str> = array
        .split("<string>")
        .skip(1)
        .map(|s| &s[..s.find("</string>").unwrap()])
        .collect();
    assert_eq!(
        paths,
        [format!("/{MACOS_PATH}")],
        "the sandbox lets the extension read another file"
    );
}

#[test]
fn the_extension_bundle_loads_inside_the_app() {
    let contract = contract();
    let plist = read(widget_dir().join("Info.plist"));
    let value = |key: &str| {
        let rest = &plist[plist
            .find(&format!("<key>{key}</key>"))
            .unwrap_or_else(|| panic!("Info.plist has no {key}"))..];
        let start = rest.find("<string>").expect("a string value") + "<string>".len();
        rest[start..start + rest[start..].find("</string>").unwrap()].to_owned()
    };
    assert_eq!(
        value("CFBundleIdentifier"),
        contract["bundle_identifier"].as_str().unwrap()
    );
    assert_eq!(
        value("CFBundleExecutable"),
        contract["executable"].as_str().unwrap()
    );
    assert_eq!(
        value("NSExtensionPointIdentifier"),
        "com.apple.widgetkit-extension"
    );
    assert_eq!(
        value("LSMinimumSystemVersion"),
        contract["minimum_system_version"].as_str().unwrap()
    );
    // The release writes the app's version into both (build-appex.sh), and
    // chronod refuses what a mismatched extension draws.
    assert_eq!(value("CFBundleVersion"), "@VERSION@");
    assert_eq!(value("CFBundleShortVersionString"), "@VERSION@");

    // An embedded extension's identifier extends the app's.
    let tauri: Value =
        serde_json::from_str(&read(repo().join("apps/desktop/src-tauri/tauri.conf.json"))).unwrap();
    let app = tauri["identifier"].as_str().unwrap();
    assert!(
        contract["bundle_identifier"]
            .as_str()
            .unwrap()
            .starts_with(&format!("{app}.")),
        "the extension's identifier must start with the app's ({app})"
    );

    // One deployment target everywhere it is written.
    let min = contract["minimum_system_version"].as_str().unwrap();
    let build = read(widget_dir().join("build-appex.sh"));
    assert!(
        build.contains(&format!("\ndeployment={min}\n")),
        "build-appex.sh builds for another macOS"
    );
    let major = min.split('.').next().unwrap();
    assert!(
        read(widget_dir().join("Package.swift")).contains(&format!(".macOS(.v{major})")),
        "Package.swift targets another macOS"
    );
}
