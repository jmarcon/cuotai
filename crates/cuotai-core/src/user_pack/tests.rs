use super::*;
use std::fmt::Write as _;

fn reason(name: &str, text: &str) -> &'static str {
    load(name, text)
        .map(|_| "ok")
        .unwrap_or_else(Refusal::reason)
}

fn text(message: &Message) -> &str {
    message.as_text().expect("a plain text")
}

/// Real shipped ids a pack may not replace (S-01): the delete dialog and
/// its buttons, a billed-query warning, the vault, the local server, the
/// user-content notices, a credential help, a consent mapped by
/// `desktop-wording.js`.
const NAMED_PROTECTED: &[&str] = &[
    "dialog.deleteTitle",
    "dialog.deleteVaultPurge",
    "confirm.cancel",
    "confirm.remove",
    "access.cost_advisory",
    "settings.vault.confirm",
    "serve.allow_browsers.warning",
    "userContent.protected",
    "credential_help.alibaba_cloud",
    "coreText.migrateConsent",
    "hooks.confirm.command",
    "extensions.consent.risk",
    "refresh.billed.title",
];

/// Real shipped ids a pack may replace.
const NAMED_OVERRIDABLE: &[&str] = &[
    "tray.ok",
    "settings.page.general",
    "card.connectSession",
    "access.summary.claude",
    "common.cancel",
    "models.search",
    "quota.count",
];

// security-invariant: SEC-USER-PACK
#[test]
fn shipped_security_texts_are_protected_and_ordinary_ones_are_not() {
    for id in NAMED_PROTECTED.iter().chain(PROTECTED_IDS) {
        assert!(is_protected(id), "{id}");
    }
    for id in NAMED_OVERRIDABLE.iter().chain(OVERRIDABLE_IDS) {
        assert!(is_overridable(id), "{id}");
    }
    // No rule, or a `confirm`/`consent` segment: protected.
    for id in [
        "brand_new.namespace.title",
        "x.consent.y",
        "tray.confirm.title",
        "confirm",
    ] {
        assert!(is_protected(id), "{id}");
    }
    // The longest match wins and an exact id beats any prefix.
    let policy = policy();
    assert_eq!(
        policy.rule(Layer::Ui, "dialog.deleteTitle"),
        Some(("dialog.delete*", Class::Protected))
    );
    assert_eq!(
        policy.rule(Layer::Ui, "dialog.title"),
        Some(("dialog.*", Class::Overridable))
    );
    assert_eq!(
        policy.rule(Layer::Ui, "dialog.saveAnyway"),
        Some(("dialog.saveAnyway", Class::Protected))
    );
    assert_eq!(
        policy.class(Layer::Core, "nothing.matches"),
        Class::Protected
    );
}

/// Every id the tests of `user_pack` (and `user_content.rs`, which uses the
/// same ones) name exists in a shipped catalog, so a renamed id fails here
/// instead of silently testing nothing (S-01: the old test asserted
/// `dialog.delete.title`, which never existed).
#[test]
fn every_id_these_tests_name_is_shipped() {
    let ui_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/desktop/ui/locales/en-US.json");
    let ui: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(ui_path).unwrap()).unwrap();
    let shipped = |id: &str| {
        ui.contains_key(id)
            || crate::text::catalog_locales()
                .iter()
                .filter_map(|code| crate::text::core_catalog(code))
                .any(|catalog| catalog.contains_key(id))
    };
    for id in NAMED_PROTECTED
        .iter()
        .chain(NAMED_OVERRIDABLE)
        .chain(PROTECTED_IDS)
        .chain(OVERRIDABLE_IDS)
    {
        assert!(shipped(id), "{id} is not in any shipped catalog");
    }
}

/// S-01 coverage: every id of every core catalog (which hold every id the
/// core writes, `text::emitted_ids_tests`, plus the deliberately absent
/// `metric.verbatim`) matches a rule of the `core` section explicitly, so
/// no shipped id relies on the default; and no rule is in both lists of a
/// section. The desktop crate checks the `ui` section the same way.
#[test]
fn every_shipped_core_id_is_classified() {
    let policy = policy();
    let mut ids: std::collections::BTreeSet<&str> = ["metric.verbatim"].into();
    for code in crate::text::catalog_locales() {
        ids.extend(
            crate::text::core_catalog(code)
                .unwrap()
                .keys()
                .map(String::as_str),
        );
    }
    let unclassified: Vec<&str> = ids
        .iter()
        .copied()
        .filter(|id| policy.rule(Layer::Core, id).is_none())
        .collect();
    assert!(
        unclassified.is_empty(),
        "add a rule to user_pack/policy.json for {unclassified:?}"
    );
    for layer in [Layer::Core, Layer::Ui] {
        let rules: Vec<(&str, Class)> = policy.rules(layer).collect();
        for (rule, class) in &rules {
            assert!(
                !rules.iter().any(|(other, c)| other == rule && c != class),
                "{rule} is both protected and overridable"
            );
            assert!(
                !rule.is_empty() && !rule[..rule.len() - 1].contains('*'),
                "{rule}: a prefix rule ends in its only `*`"
            );
        }
    }
    // The header carries the contributor rule.
    let raw: serde_json::Value = serde_json::from_str(POLICY_JSON).unwrap();
    assert!(raw["_comment"]
        .as_array()
        .is_some_and(|lines| lines.len() > 3));
}

#[test]
fn a_pack_loses_only_its_protected_ids() {
    let file = r#"{"_meta":{"name":"Français","flag":"FR"},"tray.ok":"Tout va bien","serve.allow_browsers.warning":"Sans danger !","hooks.confirm.command":"rien","x.consent.y":{"one":"a","other":"b"},"quota.count":{"one":"{count} quota","other":"{count} quotas"}}"#;
    let pack = load("fr.json", file).unwrap();
    assert_eq!(pack.code, "fr");
    assert_eq!(pack.label, "Français");
    assert_eq!(pack.flag.as_deref(), Some("FR"));
    assert_eq!(
        pack.ignored,
        [
            "hooks.confirm.command",
            "serve.allow_browsers.warning",
            "x.consent.y"
        ]
    );
    assert_eq!(
        pack.messages.keys().collect::<Vec<_>>(),
        ["quota.count", "tray.ok"]
    );
    assert_eq!(text(&pack.messages["tray.ok"]), "Tout va bien");
    assert!(
        matches!(&pack.messages["quota.count"], Message::Plural(forms) if forms["one"] == "{count} quota")
    );
    // The digest is the file's SHA-256: one byte more is another content.
    assert_eq!(pack.digest.len(), 64);
    assert_eq!(pack.digest, load("fr.json", file).unwrap().digest);
    assert_ne!(
        pack.digest,
        load("fr.json", &format!("{file} ")).unwrap().digest
    );
    // What reaches a front end is the typed values, re-serialized.
    let sent = serde_json::to_value(&pack).unwrap();
    assert_eq!(sent["messages"]["tray.ok"], "Tout va bien");
    assert_eq!(sent["messages"]["quota.count"]["other"], "{count} quotas");
    assert!(sent["messages"].get("_meta").is_none());
}

/// PRD 0035 re-validation S-02: the three values `serde_json` refuses and
/// `JSON.parse` accepts (a number out of range, nesting past the limit, a
/// lone surrogate) used to send the file to the webview unfiltered. Each
/// now refuses the whole pack, wherever it sits.
// security-invariant: SEC-USER-PACK
#[test]
fn the_three_parser_differentials_refuse_the_whole_pack() {
    let body = r#""serve.allow_browsers.warning":"Sans risque","dialog.delete.title":"Annuler","tray.ok":"OK""#;
    let deep = format!("{}1{}", "[".repeat(200), "]".repeat(200));
    let deep_object = format!("{}1{}", r#"{"a":"#.repeat(200), "}".repeat(200));
    for (meta, why) in [
        (r#"{"name":"日本語","n":1e400}"#.to_owned(), "invalidJson"),
        (format!(r#"{{"name":"한국어","p":{deep}}}"#), "invalidMeta"),
        (
            format!(r#"{{"name":"한국어","p":{deep_object}}}"#),
            "invalidMeta",
        ),
        (r#"{"name":"Deutsch\ud800"}"#.to_owned(), "invalidJson"),
        (
            r#"{"name":"Deutsch","x":"\udc00"}"#.to_owned(),
            "invalidJson",
        ),
    ] {
        let file = format!(r#"{{"_meta":{meta},{body}}}"#);
        assert_eq!(reason("fr.json", &file), why, "{meta:.40}");
    }
    // The same values as a message, or inside a plural.
    for value in [
        "1e400".to_owned(),
        "-1e400".to_owned(),
        deep.clone(),
        r#""\ud800""#.to_owned(),
        r#""a\ud800b""#.to_owned(),
        r#"{"one":"a","other":"\udfff"}"#.to_owned(),
        format!(r#"{{"one":{deep},"other":"b"}}"#),
    ] {
        let file = format!(r#"{{"_meta":{{"name":"Français"}},{body},"x.y":{value}}}"#);
        assert!(load("fr.json", &file).is_err(), "{value:.40}");
    }
    // A pack with nothing protected is never handed on as its bytes.
    let clean = r#"{"_meta":{"name":"X"},  "tray.ok":"ok"}"#;
    let pack = load("fr.json", clean).unwrap();
    assert_eq!(
        serde_json::to_string(&pack.messages).unwrap(),
        r#"{"tray.ok":"ok"}"#
    );
}

#[test]
fn each_shape_failure_refuses_the_whole_pack() {
    let ok = r#"{"_meta":{"name":"Français"},"tray.ok":"OK"}"#;
    assert_eq!(reason("fr.json", ok), "ok");
    // A byte order mark, trailing data, not an object.
    assert_eq!(reason("fr.json", &format!("\u{FEFF}{ok}")), "invalidJson");
    assert_eq!(reason("fr.json", &format!("{ok} {{}}")), "invalidJson");
    assert_eq!(reason("fr.json", "[1]"), "invalidJson");
    assert_eq!(reason("fr.json", "\"x\""), "invalidJson");
    assert_eq!(reason("fr.json", "not json"), "invalidJson");
    assert_eq!(reason("fr.json", ""), "invalidJson");
    // `_meta`.
    assert_eq!(reason("fr.json", r#"{"tray.ok":"OK"}"#), "invalidMeta");
    assert_eq!(
        reason("fr.json", r#"{"_meta":[],"tray.ok":"OK"}"#),
        "invalidMeta"
    );
    assert_eq!(reason("fr.json", r#"{"_meta":"Français"}"#), "invalidMeta");
    assert_eq!(reason("fr.json", r#"{"_meta":{}}"#), "invalidMeta");
    for name in [
        r#""""#,
        r#""   ""#,
        r#""a\nb""#,
        r#""\u202eSIUQ""#,
        r#""a\u200bb""#,
        r#""<b>x</b>""#,
        "1",
        "null",
        r#"["Français"]"#,
    ] {
        let file = format!(r#"{{"_meta":{{"name":{name}}}}}"#);
        assert_eq!(reason("fr.json", &file), "invalidMeta", "{name}");
    }
    let long = "é".repeat(MAX_NAME_CHARS + 1);
    assert_eq!(
        reason("fr.json", &format!(r#"{{"_meta":{{"name":"{long}"}}}}"#)),
        "invalidMeta"
    );
    let longest = "é".repeat(MAX_NAME_CHARS);
    assert_eq!(
        reason("fr.json", &format!(r#"{{"_meta":{{"name":"{longest}"}}}}"#)),
        "ok"
    );
    // L-02: `flag` is absent or two letters, never another type.
    for flag in [
        r#"["fr"]"#,
        "7",
        "null",
        r#""FRA""#,
        r#""f1""#,
        r#""""#,
        r#"{"a":"b"}"#,
    ] {
        let file = format!(r#"{{"_meta":{{"name":"Français","flag":{flag}}}}}"#);
        assert_eq!(reason("fr.json", &file), "invalidMeta", "{flag}");
    }
    let flagged = load("fr.json", r#"{"_meta":{"name":"Français","flag":"ca"}}"#).unwrap();
    assert_eq!(flagged.flag.as_deref(), Some("ca"));
    // A shipped catalog's provenance keys may be copied; anything else in
    // `_meta` is plain data at most three levels deep.
    let shipped = r#"{"_meta":{"name":"Français","status":"machine-translated","reviewed":[{"handle":"someone","date":"2026-01-01","scope":"full"}],"machine_reviewed":"2026-10-25","pending_review":["tray.ok"],"n":1.5,"z":null},"tray.ok":"OK"}"#;
    assert_eq!(reason("fr.json", shipped), "ok");
    assert_eq!(
        reason("fr.json", r#"{"_meta":{"name":"F","x":[[[["deep"]]]]}}"#),
        "invalidMeta"
    );
    // Values.
    for value in [
        "1",
        "true",
        "null",
        r#"["a"]"#,
        r#"{"one":"a"}"#,
        r#"{"one":"a","other":"b","several":"c"}"#,
        r#"{"one":1,"other":"b"}"#,
        r#"{}"#,
        r#""\u202eevil""#,
        r#""tab\there""#,
        r#""zero\u200bwidth""#,
        r#""\ufeff""#,
        r#""\u0007""#,
        r#""<b>bold</b>""#,
        r#""<img src=x>""#,
        r#""&amp;""#,
        r#""&#x41;""#,
        r#"{"one":"a","other":"\u2066b"}"#,
    ] {
        let file = format!(r#"{{"_meta":{{"name":"Français"}},"tray.ok":{value}}}"#);
        assert_eq!(reason("fr.json", &file), "invalidValue", "{value}");
    }
    // The shipped catalogs' example values and line breaks stay allowed.
    for value in [
        r#""<token>""#,
        r#""x < y & z""#,
        r#""R&D""#,
        r#""a\nb""#,
        r#""{count} min""#,
    ] {
        let file = format!(r#"{{"_meta":{{"name":"Français"}},"tray.ok":{value}}}"#);
        assert_eq!(reason("fr.json", &file), "ok", "{value}");
    }
    // Ids.
    for id in [
        "",
        "a b",
        "é",
        "a/b",
        "__proto__",
        &"a".repeat(MAX_ID_LEN + 1),
    ] {
        let file = format!(
            r#"{{"_meta":{{"name":"Français"}},{}:"x"}}"#,
            serde_json::to_string(id).unwrap()
        );
        assert_eq!(reason("fr.json", &file), "invalidKey", "{id:.20}");
    }
    let file = format!(
        r#"{{"_meta":{{"name":"Français"}},"{}":"x"}}"#,
        "a".repeat(MAX_ID_LEN)
    );
    assert_eq!(reason("fr.json", &file), "ok");
    // The size cap.
    let filler = "a".repeat(MAX_PACK_BYTES);
    let big = format!(r#"{{"_meta":{{"name":"Français"}},"tray.ok":"{filler}"}}"#);
    assert_eq!(reason("fr.json", &big), "tooLarge");
}

#[test]
fn a_repeated_id_keeps_its_last_value_then_is_filtered() {
    let file = r#"{"_meta":{"name":"A"},"_meta":{"name":"Français"},"tray.ok":"one","tray.ok":"two","serve.banner":"x","serve.banner":"y"}"#;
    let pack = load("fr.json", file).unwrap();
    assert_eq!(pack.label, "Français");
    assert_eq!(text(&pack.messages["tray.ok"]), "two");
    assert_eq!(pack.ignored, ["serve.banner"]);
    // An earlier value is still read in full: a broken one refuses the pack.
    let broken = r#"{"_meta":{"name":"F"},"tray.ok":1e400,"tray.ok":"two"}"#;
    assert_eq!(reason("fr.json", broken), "invalidJson");
    // A later clean value replaces an earlier disguised one, as in `JSON.parse`.
    let replaced = r#"{"_meta":{"name":"F"},"tray.ok":"\u202ex","tray.ok":"ok"}"#;
    assert_eq!(reason("fr.json", replaced), "ok");
}

/// The file code is checked here, the authority (PRD 0034 re-validation
/// S-05's rule, which the webview and the notices also apply).
#[test]
fn the_file_name_is_a_language_tag_intl_reads() {
    let file = r#"{"_meta":{"name":"X"}}"#;
    for name in [
        "fr.json",
        "pt-PT.json",
        "sr-Latn-RS.json",
        "es-419.json",
        "de-CH-1996.json",
        "ca-valencia.json",
        "nl.JSON",
    ] {
        assert_eq!(reason(name, file), "ok", "{name}");
    }
    assert_eq!(load("nl.JSON", file).unwrap().code, "nl");
    for name in [
        "en-US-US.json",
        "en-a1.json",
        "xx-ab-cd-ef.json",
        ".json",
        "e.json",
        "en--US.json",
        "de-1996-1996.json",
        "fr.txt",
        "fr",
        "../fr.json",
        "fr .json",
        "fr.json.json",
        "ab-abcdefgh-abcdefgh-abcdefgh-abcdefgh.json",
    ] {
        assert_eq!(reason(name, file), "invalidCode", "{name}");
    }
    for fine in ["sr-Latn-RS", "es-419", "de-CH-1996", "ca-valencia"] {
        assert!(is_language_tag(fine), "{fine}");
    }
    for bad in [
        "en-US-US",
        "en-a1",
        "xx-ab-cd-ef",
        "",
        "e",
        "en--US",
        "de-1996-1996",
    ] {
        assert!(!is_language_tag(bad), "{bad}");
    }
}

// ---------------------------------------------------------------------------
// Deterministic property test and the fixture the webview test replays.
// ---------------------------------------------------------------------------

/// xorshift64*: deterministic, no dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

/// Real shipped ids the policy protects.
const PROTECTED_IDS: &[&str] = &[
    "serve.allow_browsers.warning",
    "hooks.confirm.command",
    "hooks.error.command",
    "update.consent.body",
    "credential_help.alibaba_cloud",
    "oauth_error.store.sqlite_dev_only",
    "edit_error.toml_int_range",
    "extensions.consent.risk",
    "user_session.slot.secret",
    "access.risk.undocumented",
    "settings.vault.confirm",
    "dialog.deleteTitle",
    "dialog.deleteVaultPurge",
    "confirm.cancel",
];

/// Real shipped ids a pack may set.
const OVERRIDABLE_IDS: &[&str] = &[
    "tray.ok",
    "tray.lastCheck",
    "settings.page.general",
    "card.connectSession",
    "access.summary.claude",
    "card.keyMissing",
    "models.search",
    "sort.custom",
    "quota.count",
    "cost.plan.single",
    "metric.label.balance_organization",
    "dialog.copyFailed",
];

const CLEAN_TEXTS: &[&str] = &[
    "Tout va bien",
    "{count} quota",
    "日本語のテキスト",
    "Ünïcödé · {name}",
    "a\nb",
    "<token>",
    "x < y",
    "R&D",
    "😀",
    "",
];

const DIRTY_TEXTS: &[&str] = &[
    "\u{202E}evil",
    "zero\u{200B}width",
    "tab\there",
    "<b>bold</b>",
    "&amp;",
    "bell\u{7}",
    "\u{FEFF}",
    "\u{2067}isolate",
    "<img src=x>",
];

/// One generated value, its JSON and what the parser must make of it.
struct Value {
    json: String,
    /// The parser reads it (a text or a well-formed plural of texts).
    parses: bool,
    /// The parsed message when every text is clean.
    message: Option<Message>,
}

fn gen_text(rng: &mut Rng) -> (String, bool) {
    if rng.chance(12) {
        ((*rng.pick(DIRTY_TEXTS)).to_owned(), false)
    } else {
        ((*rng.pick(CLEAN_TEXTS)).to_owned(), true)
    }
}

fn gen_value(rng: &mut Rng) -> Value {
    let unread = |json: String| Value {
        json,
        parses: false,
        message: None,
    };
    match rng.below(100) {
        0..=64 => {
            let (text, clean) = gen_text(rng);
            Value {
                json: serde_json::to_string(&text).unwrap(),
                parses: true,
                message: clean.then_some(Message::Text(text)),
            }
        }
        65..=84 => {
            let mut forms = BTreeMap::new();
            let mut json = Vec::new();
            // A repeated category keeps its last form, so its last cleanliness.
            let mut clean_forms = BTreeMap::new();
            let mut categories = vec!["other"];
            categories.extend(PLURAL_CATEGORIES.iter().copied().filter(|_| rng.chance(30)));
            let mut parses = true;
            if rng.chance(8) {
                categories.retain(|c| *c != "other");
                categories.push("one");
                parses = false;
            }
            if rng.chance(5) {
                categories.push("several");
                parses = false;
            }
            for category in categories {
                let (text, ok) = gen_text(rng);
                clean_forms.insert(category, ok);
                json.push(format!(
                    "\"{category}\":{}",
                    serde_json::to_string(&text).unwrap()
                ));
                forms.insert(category.to_owned(), text);
            }
            let clean = clean_forms.values().all(|ok| *ok);
            Value {
                json: format!("{{{}}}", json.join(",")),
                parses,
                message: (parses && clean).then_some(Message::Plural(forms)),
            }
        }
        85..=88 => unread((*rng.pick(&["1", "true", "null", "[\"a\"]", "2.5"])).to_owned()),
        89 => unread(r#"{"one":1,"other":"b"}"#.to_owned()),
        90..=92 => {
            unread((*rng.pick(&["1e400", "-1e400", r#""\ud800""#, r#""x\udfffy""#])).to_owned())
        }
        _ => unread(format!("{}1{}", "[".repeat(200), "]".repeat(200))),
    }
}

/// A generated pack file and what [`load`] must return for it.
struct Case {
    name: String,
    input: String,
    expected: Result<(BTreeMap<String, Message>, Vec<String>), ()>,
}

fn gen_case(rng: &mut Rng) -> Case {
    let name = if rng.chance(4) {
        (*rng.pick(&["en-US-US.json", "en-a1.json", "fr.txt"])).to_owned()
    } else {
        (*rng.pick(&[
            "fr.json",
            "pt-PT.json",
            "xx.json",
            "sr-Latn-RS.json",
            "de-CH-1996.json",
            "nl.json",
        ]))
        .to_owned()
    };
    let mut ok = !name.ends_with(".txt") && !name.starts_with("en-");
    // `_meta`.
    let (meta, meta_parses, meta_ok) = match rng.below(100) {
        0..=59 => (r#"{"name":"Langue"}"#.to_owned(), true, true),
        60..=69 => (r#"{"name":"Langue","flag":"BR"}"#.to_owned(), true, true),
        70..=74 => (
            r#"{"name":"Langue","status":"draft","reviewed":false,"pending_review":["tray.ok"]}"#
                .to_owned(),
            true,
            true,
        ),
        75..=77 => (
            r#"{"name":"Langue","flag":["fr"]}"#.to_owned(),
            false,
            false,
        ),
        78..=79 => (r#"{"name":"Langue","flag":"BRA"}"#.to_owned(), true, false),
        80..=81 => (r#"{"name":"Langue","n":1e400}"#.to_owned(), false, false),
        82..=83 => (
            format!(
                r#"{{"name":"Langue","p":{}0{}}}"#,
                "[".repeat(200),
                "]".repeat(200)
            ),
            false,
            false,
        ),
        84..=85 => (r#"{"name":"Langue\ud800"}"#.to_owned(), false, false),
        86..=87 => (r#"{"name":"\u202eeugnaL"}"#.to_owned(), true, false),
        88..=89 => (
            format!(r#"{{"name":"{}"}}"#, "x".repeat(MAX_NAME_CHARS + 1)),
            true,
            false,
        ),
        90..=91 => ("[]".to_owned(), false, false),
        92..=93 => (r#"{"name":7}"#.to_owned(), false, false),
        _ => (String::new(), true, false), // no `_meta`
    };
    ok &= meta_parses && meta_ok;
    let mut entries: Vec<String> = Vec::new();
    if !meta.is_empty() {
        entries.push(format!(r#""_meta":{meta}"#));
    }
    let mut items: Vec<(String, Option<Message>)> = Vec::new();
    for _ in 0..rng.below(9) {
        let id = match rng.below(100) {
            0..=44 => (*rng.pick(OVERRIDABLE_IDS)).to_owned(),
            45..=89 => (*rng.pick(PROTECTED_IDS)).to_owned(),
            90..=94 => format!("custom.{}", rng.below(4)),
            _ => (*rng.pick(&["bad key", "é", "__proto__", "a/b"])).to_owned(),
        };
        let value = gen_value(rng);
        ok &= value.parses;
        entries.push(format!(
            "{}:{}",
            serde_json::to_string(&id).unwrap(),
            value.json
        ));
        items.push((id, value.message));
    }
    if rng.chance(15) {
        entries.reverse(); // `_meta` anywhere
        items.reverse();
    }
    // A repeated id keeps its last value.
    let last: BTreeMap<String, Option<Message>> = items.into_iter().collect();
    let mut input = format!("{{{}}}", entries.join(","));
    if rng.chance(3) {
        input.insert(0, '\u{FEFF}');
        ok = false;
    }
    if rng.chance(3) {
        input.push_str(",{}");
        ok = false;
    }
    let mut kept = BTreeMap::new();
    let mut ignored = Vec::new();
    for (id, message) in last {
        let Some(message) = message.filter(|_| is_valid_id(&id)) else {
            ok = false;
            continue;
        };
        if is_protected(&id) {
            ignored.push(id);
        } else {
            kept.insert(id, message);
        }
    }
    Case {
        name,
        input,
        expected: if ok { Ok((kept, ignored)) } else { Err(()) },
    }
}

fn check(case: &Case) -> Result<Pack, Refusal> {
    let result = load(&case.name, &case.input);
    match (&result, &case.expected) {
        (Ok(pack), Ok((kept, ignored))) => {
            assert_eq!(&pack.messages, kept, "{}", case.input);
            assert_eq!(&pack.ignored, ignored, "{}", case.input);
            for (id, message) in &pack.messages {
                assert!(is_valid_id(id) && !is_protected(id), "{id}");
                assert!(message.templates().into_iter().all(is_clean_value), "{id}");
            }
            // What the shell sends parses back to the same messages.
            let sent = serde_json::to_string(&pack.messages).unwrap();
            let again = format!(
                r#"{{"_meta":{{"name":"R"}}{}{}"#,
                if sent.len() > 2 { "," } else { "" },
                &sent[1..]
            );
            let again = load("fr.json", &again).expect("a re-serialized pack loads");
            assert_eq!(again.messages, pack.messages);
            assert!(again.ignored.is_empty());
        }
        (Err(_), Err(())) => {}
        (got, want) => panic!(
            "{} {:?}: got {got:?}, want ok={}",
            case.name,
            case.input,
            want.is_ok()
        ),
    }
    result
}

/// PRD 0035 re-validation S-02: about 2 000 generated packs mixing shipped
/// protected and overridable ids, garbage values and the parser
/// differentials. No panic; a pack loads exactly when every part is valid,
/// and then holds only the overridable ids with clean values, which
/// survive a re-serialization.
// security-invariant: SEC-USER-PACK
#[test]
fn generated_packs_load_only_when_valid_and_keep_only_allowed_clean_texts() {
    let mut rng = Rng(0x0035_5EED_C0DE_0002);
    let (mut loaded, mut refused) = (0, 0);
    for _ in 0..2_000 {
        match check(&gen_case(&mut rng)) {
            Ok(_) => loaded += 1,
            Err(_) => refused += 1,
        }
    }
    assert!(
        loaded > 300 && refused > 300,
        "{loaded} loaded, {refused} refused"
    );
}

/// The cases `scripts/test-user-content-ui.mjs` replays: what the webview
/// registers must equal what the core loaded. Regenerate with
/// `CUOTAI_REGENERATE_PACK_FIXTURE=1 cargo test -p cuotai-core user_pack`.
#[test]
fn the_webview_fixture_matches_the_core() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/user_pack/filtered.json");
    let mut cases: Vec<(String, String)> = [
        // Security's three S-02 packs, as they were reproduced.
        ("ja.json", r#"{"_meta":{"name":"日本語","n":1e400},"serve.allow_browsers.warning":"Sans risque","dialog.delete":"Annuler","tray.ok":"OK"}"#.to_owned()),
        ("ko.json", format!(r#"{{"_meta":{{"name":"한국어","p":{}{}}},"serve.allow_browsers.warning":"Sans risque","tray.ok":"OK"}}"#, "[".repeat(200), "]".repeat(200))),
        ("de.json", r#"{"_meta":{"name":"Deutsch\ud800"},"serve.allow_browsers.warning":"Kein Risiko","tray.ok":"OK"}"#.to_owned()),
        ("fr.json", r#"{"_meta":{"name":"Français","flag":["fr"]},"tray.ok":"OK"}"#.to_owned()),
        // S-03: a pack cannot pose as a shipped language, and en-US is reserved.
        ("fr.json", r#"{"_meta":{"name":"Deutsch"},"tray.ok":"Tout va bien"}"#.to_owned()),
        ("xx.json", r#"{"_meta":{"name":"Français"},"tray.ok":"OK"}"#.to_owned()),
        ("en-US.json", r#"{"_meta":{"name":"English"},"tray.ok":"Nope"}"#.to_owned()),
        ("en-GB.json", r#"{"_meta":{"name":"English (UK)"},"tray.ok":"All fine, mate"}"#.to_owned()),
    ]
    .into_iter()
    .map(|(name, input)| (name.to_owned(), input))
    .collect();
    let mut rng = Rng(0x0035_F1C7_0000_0030);
    let (mut loaded, mut refused) = (0, 0);
    while loaded < 18 || refused < 8 {
        let case = gen_case(&mut rng);
        let is_ok = case.expected.is_ok();
        if (is_ok && loaded < 18) || (!is_ok && refused < 8) {
            if is_ok {
                loaded += 1;
            } else {
                refused += 1;
            }
            cases.push((case.name, case.input));
        }
    }
    let out: Vec<serde_json::Value> = cases
        .iter()
        .map(|(name, input)| {
            let output = match load(name, input) {
                Ok(pack) => serde_json::to_value(&pack).unwrap(),
                Err(refusal) => serde_json::json!({ "refused": refusal.reason() }),
            };
            serde_json::json!({ "name": name, "input": input, "output": output })
        })
        .collect();
    let mut rendered = serde_json::to_string_pretty(&out).unwrap();
    rendered.push('\n');
    if std::env::var_os("CUOTAI_REGENERATE_PACK_FIXTURE").is_some() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, rendered).unwrap();
        return;
    }
    let stored = std::fs::read_to_string(&path).unwrap_or_default();
    let mut hint = String::new();
    let _ = write!(
        hint,
        "{} is stale: run CUOTAI_REGENERATE_PACK_FIXTURE=1 cargo test -p cuotai-core user_pack",
        path.display()
    );
    assert!(stored == rendered, "{hint}");
}

/// PRD 0035 re-validation S-03: a pack cannot pose as a shipped language.
// security-invariant: SEC-USER-PACK
#[test]
fn a_pack_never_labels_itself_as_a_shipped_language() {
    let label = |file: &str, name: &str| {
        load(file, &format!(r#"{{"_meta":{{"name":"{name}"}}}}"#))
            .unwrap()
            .label
    };
    // A built-in code (or one that normalizes to it) shows the registry endonym.
    assert_eq!(label("fr.json", "Deutsch"), "Français");
    assert_eq!(label("fr-CA.json", "Mon français"), "Français");
    assert_eq!(label("en-GB.json", "Deutsch"), "English");
    assert_eq!(label("pt-PT.json", "Português de Portugal"), "Português");
    // Another code keeps its own name, unless the name folds to an endonym.
    assert_eq!(label("nl.json", "Nederlands"), "Nederlands");
    assert_eq!(label("xx.json", "Français"), "xx");
    assert_eq!(label("xx.json", "  FRANCAIS "), "xx");
    assert_eq!(label("xx.json", "deutsch"), "xx");
    assert_eq!(label("sr-Latn-RS.json", "Srpski"), "Srpski");
    // en-US is every language's fallback: refused, in any case; en-GB is not.
    for file in ["en-US.json", "en-us.json", "EN-US.json"] {
        assert_eq!(
            reason(file, r#"{"_meta":{"name":"English"}}"#),
            "reservedCode",
            "{file}"
        );
    }
    assert_eq!(
        reason("en-GB.json", r#"{"_meta":{"name":"English"}}"#),
        "ok"
    );
}

/// S-03: the file name the interface shows is stripped of control,
/// direction and hidden characters and capped.
#[test]
fn the_file_name_shown_is_safe() {
    assert_eq!(safe_file_name("fr.json"), "fr.json");
    assert_eq!(safe_file_name("f\u{202E}nosj.rf"), "fnosj.rf");
    assert_eq!(safe_file_name("a\u{200B}b\u{7}\n\u{2066}.json"), "ab.json");
    let long = format!("{}.json", "x".repeat(100));
    let shown = safe_file_name(&long);
    assert_eq!(shown.chars().count(), MAX_FILE_CHARS);
    assert!(shown.ends_with('…'));
    let pack = load("fr.json", r#"{"_meta":{"name":"F"}}"#).unwrap();
    assert_eq!(pack.file, "fr.json");
}
