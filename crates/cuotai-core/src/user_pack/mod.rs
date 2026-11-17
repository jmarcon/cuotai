//! PRD 0050 RF3.3: a user language pack, read fail-closed.
//!
//! A pack (`<user folder>/locales/<code>.json`) may add or replace ordinary
//! interface and core texts, but never the words of credential warnings,
//! consent, confirmations or the local server: those always use the
//! reviewed built-in or core text.
//!
//! PRD 0035 re-validation S-02: [`load`] is the only way a pack reaches a
//! front end. It parses the file once, into typed values (never a free JSON
//! value passed on), checks its shape, and removes the protected ids. Any
//! parse or shape failure refuses the **whole** pack with a [`Refusal`]
//! whose reason the pack cannot influence, so no text of a file the core
//! could not read safely reaches the interface. A front end hands on only
//! the [`Pack`] (its `messages` re-serialized from the typed values), never
//! the file's bytes, so a second parser can never disagree with this one.

use crate::text::{
    parity::{has_markup, is_hidden_or_directional},
    Message,
};
use serde::{
    de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor},
    Serialize,
};
use std::{cell::Cell, collections::BTreeMap, fmt};

/// The largest pack file read, in bytes (the desktop shell's file cap).
pub const MAX_PACK_BYTES: usize = 256 * 1024;
/// The longest `_meta.name`, in characters.
pub const MAX_NAME_CHARS: usize = 48;
/// The longest pack code (the file name without `.json`), in bytes.
pub const MAX_CODE_LEN: usize = 35;
/// The longest message id, in bytes.
pub const MAX_ID_LEN: usize = 128;
/// The CLDR plural categories a plural object may use.
pub const PLURAL_CATEGORIES: [&str; 6] = ["zero", "one", "two", "few", "many", "other"];
/// How deep the free provenance keys of `_meta` (`status`, `reviewed`,
/// `pending_review`, … copied from a shipped catalog) may nest.
const META_DEPTH: usize = 3;

/// A pack the core read and filtered: everything a front end may use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Pack {
    /// The language code: the file name without `.json`, a BCP 47 tag
    /// `Intl` reads ([`is_language_tag`]).
    pub code: String,
    /// What the language selector shows: `_meta.name`.
    pub label: String,
    /// `_meta.flag`: two ASCII letters, a country code.
    pub flag: Option<String>,
    /// The SHA-256 of the file's bytes, lowercase hex: tells two contents
    /// of one file apart.
    pub digest: String,
    /// The messages the pack may set, by id: a text or a plural object.
    pub messages: BTreeMap<String, Message>,
    /// The ids the pack set but may not replace (sorted); they were removed
    /// from `messages`.
    pub ignored: Vec<String>,
}

/// Why a pack was refused as a whole. The reason is fixed by the core: no
/// text of the pack reaches it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The file name is not `<language tag>.json`.
    InvalidCode,
    /// The file is larger than [`MAX_PACK_BYTES`].
    TooLarge,
    /// Not a JSON object, a syntax error, a number out of range, nesting
    /// past the parser's limit, a lone surrogate, a byte order mark.
    InvalidJson,
    /// `_meta` missing or not an object, a missing, empty, long or
    /// disguised `name`, a `flag` that is not two letters, or a provenance
    /// value that is not plain data.
    InvalidMeta,
    /// A message id outside `[A-Za-z0-9_.-]{1,128}` (or `__proto__`).
    InvalidKey,
    /// A value that is not a text or a plural object of texts, a plural
    /// category outside CLDR or without `other`, or a text with control,
    /// hidden or direction characters or markup.
    InvalidValue,
}

impl Refusal {
    /// The reason id (`userContent.reason.<id>` in the UI catalogs).
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::InvalidCode => "invalidCode",
            Refusal::TooLarge => "tooLarge",
            Refusal::InvalidJson => "invalidJson",
            Refusal::InvalidMeta => "invalidMeta",
            Refusal::InvalidKey => "invalidKey",
            Refusal::InvalidValue => "invalidValue",
        }
    }
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason())
    }
}

impl std::error::Error for Refusal {}

/// Id prefixes a pack may not replace.
pub const PROTECTED_PREFIXES: [&str; 12] = [
    "credential_help.",
    "oauth_error.",
    "edit_error.",
    "remove_error.",
    "user_session.",
    "access.risk.",
    "access.credential_hint.",
    "hooks.",
    "extensions.consent.",
    "serve.",
    "update.",
    "execution.",
];

/// Whether a pack may not replace `id`: a [`PROTECTED_PREFIXES`] id, the
/// delete confirmation (`dialog.delete…`), or any id with a `.confirm.` or
/// `.consent.` part.
pub fn is_protected(id: &str) -> bool {
    PROTECTED_PREFIXES
        .iter()
        .any(|prefix| id.starts_with(prefix))
        || id == "dialog.delete"
        || id.starts_with("dialog.delete.")
        || id.contains(".confirm.")
        || id.contains(".consent.")
}

/// Read the pack `text` of the file `file_name` (`fr.json`): parse, check
/// and filter it, or refuse it whole.
pub fn load(file_name: &str, text: &str) -> Result<Pack, Refusal> {
    let code = pack_code(file_name).ok_or(Refusal::InvalidCode)?;
    if text.len() > MAX_PACK_BYTES {
        return Err(Refusal::TooLarge);
    }
    let raw = parse(text)?;
    let meta = raw.meta.ok_or(Refusal::InvalidMeta)?;
    let name = meta.name.ok_or(Refusal::InvalidMeta)?;
    if !is_clean_name(&name) {
        return Err(Refusal::InvalidMeta);
    }
    if meta
        .flag
        .as_deref()
        .is_some_and(|flag| !(flag.len() == 2 && flag.bytes().all(|b| b.is_ascii_alphabetic())))
    {
        return Err(Refusal::InvalidMeta);
    }
    for (id, message) in &raw.messages {
        if !is_valid_id(id) {
            return Err(Refusal::InvalidKey);
        }
        if !message.templates().into_iter().all(is_clean_value) {
            return Err(Refusal::InvalidValue);
        }
    }
    let (messages, ignored) = filter(raw.messages);
    Ok(Pack {
        code: code.to_owned(),
        label: name,
        flag: meta.flag,
        digest: digest(text.as_bytes()),
        messages,
        ignored,
    })
}

/// The code of a pack file: `file_name` without its `.json` (any case),
/// when it is a language tag of at most [`MAX_CODE_LEN`] bytes.
pub fn pack_code(file_name: &str) -> Option<&str> {
    let cut = file_name.len().checked_sub(5)?;
    let (code, ext) = (file_name.get(..cut)?, file_name.get(cut..)?);
    (ext.eq_ignore_ascii_case(".json") && code.len() <= MAX_CODE_LEN && is_language_tag(code))
        .then_some(code)
}

/// Whether `code` is a BCP 47 language tag of the shape a language pack
/// name may take (the webview's `LOCALE_RE` plus `Intl`'s grammar): a
/// 2-3 letter language, then an optional 4-letter script, an optional
/// region (2 letters or 3 digits) and distinct variants (5-8 alphanumerics,
/// or 4 starting with a digit). `en-US-US` and `en-a1` are refused, as
/// `Intl.getCanonicalLocales` refuses them (PRD 0034 re-validation S-05).
pub fn is_language_tag(code: &str) -> bool {
    let mut parts = code.split('-').peekable();
    let alpha = |s: &str, n: std::ops::RangeInclusive<usize>| {
        n.contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphabetic())
    };
    if !parts.next().is_some_and(|lang| alpha(lang, 2..=3)) {
        return false;
    }
    if parts.peek().is_some_and(|s| alpha(s, 4..=4)) {
        parts.next();
    }
    if parts
        .peek()
        .is_some_and(|s| alpha(s, 2..=2) || (s.len() == 3 && s.bytes().all(|b| b.is_ascii_digit())))
    {
        parts.next();
    }
    let mut variants = Vec::new();
    for variant in parts {
        let alnum = variant.bytes().all(|b| b.is_ascii_alphanumeric());
        let shaped = (5..=8).contains(&variant.len())
            || (variant.len() == 4 && variant.as_bytes()[0].is_ascii_digit());
        let lower = variant.to_ascii_lowercase();
        if !alnum || !shaped || variants.contains(&lower) {
            return false;
        }
        variants.push(lower);
    }
    true
}

/// Whether `id` may name a message: 1 to [`MAX_ID_LEN`] ASCII letters,
/// digits, `_`, `.` or `-`, and not `__proto__` (which a JavaScript
/// assignment would read as the object's prototype).
pub fn is_valid_id(id: &str) -> bool {
    (1..=MAX_ID_LEN).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
        && id != "__proto__"
}

/// Whether a message text is clean: no control character but a line
/// break, no hidden or direction character, no markup (the shipped
/// catalogs' rule, PRD 0034 re-validation S-04).
pub fn is_clean_value(text: &str) -> bool {
    !text.chars().any(is_hidden_or_directional) && !has_markup(text)
}

/// Whether `name` may label the language: 1 to [`MAX_NAME_CHARS`]
/// characters, not blank, a clean value without a line break.
fn is_clean_name(name: &str) -> bool {
    (1..=MAX_NAME_CHARS).contains(&name.chars().count())
        && !name.trim().is_empty()
        && !name.contains('\n')
        && is_clean_value(name)
}

/// The messages a pack may set, and the ids it may not (sorted).
fn filter(messages: BTreeMap<String, Message>) -> (BTreeMap<String, Message>, Vec<String>) {
    let (ignored, kept): (BTreeMap<_, _>, BTreeMap<_, _>) =
        messages.into_iter().partition(|(id, _)| is_protected(id));
    (kept, ignored.into_keys().collect())
}

fn digest(bytes: &[u8]) -> String {
    use sha2::Digest;
    sha2::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------
// The typed parser. Every value is read by a visitor that accepts only its
// shape; nothing is skipped unread (`IgnoredAny` scans numbers, escapes and
// depth more leniently than a real read), so a number out of range, a lone
// surrogate or deep nesting anywhere in the file refuses it.
// ---------------------------------------------------------------------------

/// Where the parser is, to name a type error.
#[derive(Clone, Copy)]
enum Section {
    Top,
    Meta,
    Value,
}

#[derive(Default)]
struct Raw {
    meta: Option<Meta>,
    messages: BTreeMap<String, Message>,
}

#[derive(Default)]
struct Meta {
    name: Option<String>,
    flag: Option<String>,
}

fn parse(text: &str) -> Result<Raw, Refusal> {
    let section = Cell::new(Section::Top);
    let mut deserializer = serde_json::Deserializer::from_str(text);
    PackSeed(&section)
        .deserialize(&mut deserializer)
        .and_then(|raw| deserializer.end().map(|()| raw))
        .map_err(|error| match (error.classify(), section.get()) {
            (serde_json::error::Category::Data, Section::Meta) => Refusal::InvalidMeta,
            (serde_json::error::Category::Data, Section::Value) => Refusal::InvalidValue,
            _ => Refusal::InvalidJson,
        })
}

/// The top-level object. A repeated id keeps its last value, as
/// `JSON.parse` does.
struct PackSeed<'a>(&'a Cell<Section>);

impl<'de> DeserializeSeed<'de> for PackSeed<'_> {
    type Value = Raw;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Raw, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for PackSeed<'_> {
    type Value = Raw;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Raw, A::Error> {
        let mut raw = Raw::default();
        loop {
            self.0.set(Section::Top);
            let Some(key) = map.next_key::<String>()? else {
                return Ok(raw);
            };
            if key == "_meta" {
                self.0.set(Section::Meta);
                raw.meta = Some(map.next_value_seed(MetaSeed)?);
            } else {
                self.0.set(Section::Value);
                let message = map.next_value_seed(MessageSeed)?;
                raw.messages.insert(key, message);
            }
        }
    }
}

/// `_meta`: `name` and `flag` are texts; any other key holds plain
/// provenance data, read and dropped.
struct MetaSeed;

impl<'de> DeserializeSeed<'de> for MetaSeed {
    type Value = Meta;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Meta, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for MetaSeed {
    type Value = Meta;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a metadata object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Meta, A::Error> {
        let mut meta = Meta::default();
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "name" => meta.name = Some(map.next_value_seed(TextSeed)?),
                "flag" => meta.flag = Some(map.next_value_seed(TextSeed)?),
                _ => map.next_value_seed(PlainSeed(0))?,
            }
        }
        Ok(meta)
    }
}

/// A message: a text, or a plural object of CLDR categories (with `other`)
/// whose forms are texts.
struct MessageSeed;

impl<'de> DeserializeSeed<'de> for MessageSeed {
    type Value = Message;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Message, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for MessageSeed {
    type Value = Message;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a text or a plural object")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<Message, E> {
        Ok(Message::Text(text.to_owned()))
    }

    fn visit_string<E: de::Error>(self, text: String) -> Result<Message, E> {
        Ok(Message::Text(text))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Message, A::Error> {
        let mut forms = BTreeMap::new();
        while let Some(category) = map.next_key::<String>()? {
            if !PLURAL_CATEGORIES.contains(&category.as_str()) {
                return Err(de::Error::custom("not a plural category"));
            }
            forms.insert(category, map.next_value_seed(TextSeed)?);
        }
        if !forms.contains_key("other") {
            return Err(de::Error::custom("a plural needs `other`"));
        }
        Ok(Message::Plural(forms))
    }
}

/// A JSON string, and nothing else.
struct TextSeed;

impl<'de> DeserializeSeed<'de> for TextSeed {
    type Value = String;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for TextSeed {
    type Value = String;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a text")
    }

    fn visit_str<E: de::Error>(self, text: &str) -> Result<String, E> {
        Ok(text.to_owned())
    }

    fn visit_string<E: de::Error>(self, text: String) -> Result<String, E> {
        Ok(text)
    }
}

/// Plain data at most [`META_DEPTH`] levels deep: texts, booleans, null,
/// finite numbers, and lists or objects of them. Read in full, then dropped.
struct PlainSeed(usize);

impl<'de> DeserializeSeed<'de> for PlainSeed {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for PlainSeed {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("plain data")
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<(), E> {
        if value.is_finite() {
            Ok(())
        } else {
            Err(E::custom("not a finite number"))
        }
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        if self.0 >= META_DEPTH {
            return Err(de::Error::custom("nested too deep"));
        }
        while seq.next_element_seed(PlainSeed(self.0 + 1))?.is_some() {}
        Ok(())
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        if self.0 >= META_DEPTH {
            return Err(de::Error::custom("nested too deep"));
        }
        while map.next_key::<String>()?.is_some() {
            map.next_value_seed(PlainSeed(self.0 + 1))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
