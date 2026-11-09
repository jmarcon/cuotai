//! PRD 0050 RF3.1/RF3.2: the contract of a user theme.
//!
//! A user theme (`<user folder>/themes/<id>.css`) is parsed here before its
//! bytes reach a webview. It is token-only: an optional
//! `/*! cuotai-theme {…} */` metadata object, then exactly one
//! `:root[data-theme="<id>"][data-mode="dark"]` and one `…[data-mode="light"]`
//! rule whose declarations are `color-scheme` or a documented theme custom
//! property. Values are tokenized and checked against a small grammar
//! (colors, lengths, times, easing, system font lists, gradients and
//! shadows); anything else — another selector or property, `!important`, an
//! at-rule, an escape, a `url` token, an image or unknown function — refuses
//! the whole file with a stable reason id. The accepted theme is
//! re-serialized from its tokens, so comments and unparsed text never reach
//! the interface. CSP stays the authoritative barrier against fetches; this
//! parser is defense in depth.

use serde::Serialize;

/// Theme custom properties a user theme may set (the token set of
/// `apps/desktop/ui/css/themes/README.md`), with the value class each takes.
const PROPERTIES: &[(&str, Class)] = &[
    ("color-scheme", Class::ColorScheme),
    ("--bg", Class::Color),
    ("--surface", Class::Color),
    ("--surface-2", Class::Color),
    ("--border", Class::Color),
    ("--fg", Class::Color),
    ("--fg-muted", Class::Color),
    ("--accent", Class::Color),
    ("--accent-fg", Class::Color),
    ("--good", Class::Color),
    ("--warn", Class::Color),
    ("--bad", Class::Color),
    ("--bad-fg", Class::Color),
    ("--info", Class::Color),
    ("--track", Class::Color),
    ("--focus", Class::Color),
    ("--chart-neutral", Class::Color),
    ("--scrim", Class::Color),
    ("--switch-knob", Class::Color),
    ("--btn-fg", Class::Color),
    ("--tile-fg", Class::Color),
    ("--font-ui", Class::Font),
    ("--font-mono", Class::Font),
    ("--radius-lg", Class::Length),
    ("--radius-md", Class::Length),
    ("--radius-sm", Class::Length),
    ("--radius-pill", Class::Length),
    ("--logo-radius", Class::Length),
    ("--border-w", Class::Length),
    ("--shadow", Class::Paint),
    ("--shadow-card", Class::Paint),
    ("--shadow-dialog", Class::Paint),
    ("--card-bg", Class::Paint),
    ("--head-bg", Class::Paint),
    ("--topbar-bg", Class::Paint),
    ("--btn-bg", Class::Paint),
    ("--input-bg", Class::Paint),
    ("--body-bg", Class::Paint),
    ("--hatch", Class::Paint),
    ("--motion-fast", Class::Time),
    ("--motion-base", Class::Time),
    ("--motion-slow", Class::Time),
    ("--motion-spin", Class::Time),
    ("--ease-out", Class::Easing),
    ("--ease-spring", Class::Easing),
];

/// Functions a value may call. `url` and every other function (`image-set`,
/// `cross-fade`, `element`, `paint`, `src`, `image`, `calc`, `env`…) refuse
/// the theme.
const FUNCTIONS: &[&str] = &[
    "rgb",
    "rgba",
    "hsl",
    "hsla",
    "color-mix",
    "var",
    "linear-gradient",
    "repeating-linear-gradient",
    "radial-gradient",
    "repeating-radial-gradient",
    "cubic-bezier",
    "steps",
    "linear",
];
const COLOR_FUNCTIONS: &[&str] = &["rgb", "rgba", "hsl", "hsla", "color-mix", "var"];
const EASING_FUNCTIONS: &[&str] = &["cubic-bezier", "steps", "linear", "var"];
const UNITS: &[&str] = &["px", "em", "rem", "%", "deg", "turn", "ms", "s"];

/// Nesting allowed for functions (`linear-gradient(color-mix(in srgb, var(--x) 5%, …))`).
const MAX_DEPTH: usize = 3;
const MAX_DECLARATIONS: usize = 128;
const MAX_COMPONENTS: usize = 256;
const MAX_STRING: usize = 64;
const MAX_LABEL: usize = 48;
const MAX_GROUP: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Class {
    Color,
    Length,
    Time,
    Easing,
    Font,
    Paint,
    ColorScheme,
}

/// Why a user theme was refused. [`ThemeError::reason`] is the stable id the
/// front ends localize (`userContent.reason.<id>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeError {
    /// The file name is not `[a-z0-9][a-z0-9-]{0,31}`.
    InvalidId,
    /// Unbalanced or unparseable text, an escape (`\`) or a control character.
    Syntax,
    /// An at-rule other than `@import`/`@font-face`.
    AtRule,
    /// `@import`, `@font-face` or a `url(…)` token.
    ExternalUrl,
    /// A function outside the theme grammar (`image-set`, `element`, …).
    Function,
    /// A selector other than the two `:root[data-theme="<id>"][data-mode]` rules.
    Scope,
    /// A property outside the documented theme tokens.
    Property,
    /// A value outside its token's grammar, `!important` included.
    Value,
    /// Not exactly one dark and one light rule.
    Modes,
    /// The `cuotai-theme` header is not a valid JSON object, or appears twice.
    InvalidMeta,
}

impl ThemeError {
    pub fn reason(self) -> &'static str {
        match self {
            Self::InvalidId => "invalidId",
            Self::Syntax => "syntax",
            Self::AtRule => "atRule",
            Self::ExternalUrl => "externalUrl",
            Self::Function => "function",
            Self::Scope => "scope",
            Self::Property => "property",
            Self::Value => "value",
            Self::Modes => "modes",
            Self::InvalidMeta => "invalidMeta",
        }
    }
}

impl std::fmt::Display for ThemeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.reason())
    }
}

impl std::error::Error for ThemeError {}

/// An accepted user theme: its registry entry and the canonical CSS rebuilt
/// from its tokens (the only text a front end may insert).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Theme {
    pub id: String,
    pub label: String,
    pub group: String,
    pub modes: Vec<&'static str>,
    pub css: String,
}

/// Whether `id` is a valid theme id (the file name without `.css`).
pub fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 32
        && (bytes[0].is_ascii_lowercase() || bytes[0].is_ascii_digit())
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

/// Parse the theme `text` of the file whose id is `id`.
pub fn parse(id: &str, text: &str) -> Result<Theme, ThemeError> {
    if !valid_id(id) {
        return Err(ThemeError::InvalidId);
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if text.contains('\\')
        || text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(ThemeError::Syntax);
    }
    let (body, meta) = strip_comments(text)?;
    prescan(&body)?;
    let rules = rules(id, &body)?;
    let dark = rules.iter().filter(|r| r.mode == "dark").count();
    let light = rules.iter().filter(|r| r.mode == "light").count();
    if dark != 1 || light != 1 {
        return Err(ThemeError::Modes);
    }
    let (label, group) = metadata(id, meta.as_deref())?;
    let mut css = String::new();
    for rule in &rules {
        css.push_str(&format!(
            ":root[data-theme=\"{id}\"][data-mode=\"{}\"] {{\n",
            rule.mode
        ));
        for (name, value) in &rule.declarations {
            css.push_str(&format!("  {name}: {value};\n"));
        }
        css.push_str("}\n");
    }
    Ok(Theme {
        id: id.to_owned(),
        label,
        group,
        modes: vec!["dark", "light"],
        css,
    })
}

/// The text with every comment replaced by a space, and the body of the
/// `cuotai-theme` header when there is one.
fn strip_comments(text: &str) -> Result<(String, Option<String>), ThemeError> {
    let mut out = String::with_capacity(text.len());
    let mut meta = None;
    let mut rest = text;
    loop {
        let Some(start) = find_outside_strings(rest, "/*") else {
            out.push_str(rest);
            break;
        };
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let end = after.find("*/").ok_or(ThemeError::Syntax)?;
        let comment = &after[..end];
        if let Some(header) = comment
            .strip_prefix('!')
            .map(str::trim_start)
            .and_then(|c| c.strip_prefix("cuotai-theme"))
        {
            if meta.is_some() {
                return Err(ThemeError::InvalidMeta);
            }
            meta = Some(header.trim().to_owned());
        }
        out.push(' ');
        rest = &after[end + 2..];
    }
    Ok((out, meta))
}

/// The byte offset of `needle` outside quoted strings.
fn find_outside_strings(text: &str, needle: &str) -> Option<usize> {
    let mut quote = None;
    for (i, c) in text.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if text[i..].starts_with(needle) => return Some(i),
            None => {}
        }
    }
    None
}

/// Whole-file refusals that take precedence over the rule structure:
/// at-rules and `url(` tokens.
fn prescan(body: &str) -> Result<(), ThemeError> {
    if let Some(at) = find_outside_strings(body, "@") {
        let word: String = body[at + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect::<String>()
            .to_ascii_lowercase();
        return Err(if word == "import" || word == "font-face" {
            ThemeError::ExternalUrl
        } else {
            ThemeError::AtRule
        });
    }
    let lower = body.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find("url") {
        let at = from + pos;
        let before = lower[..at].chars().next_back();
        let ident_before =
            before.is_some_and(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        let after = lower[at + 3..].trim_start();
        if !ident_before && after.starts_with('(') {
            return Err(ThemeError::ExternalUrl);
        }
        from = at + 3;
    }
    Ok(())
}

struct Rule {
    mode: &'static str,
    declarations: Vec<(String, String)>,
}

fn rules(id: &str, body: &str) -> Result<Vec<Rule>, ThemeError> {
    let dark = format!(":root[data-theme=\"{id}\"][data-mode=\"dark\"]");
    let light = format!(":root[data-theme=\"{id}\"][data-mode=\"light\"]");
    let mut rules = Vec::new();
    let mut rest = body;
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            break;
        }
        let open = find_outside_strings(rest, "{").ok_or(ThemeError::Syntax)?;
        let prelude: String = rest[..open]
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if prelude.contains('}') || prelude.contains(';') {
            return Err(ThemeError::Syntax);
        }
        let prelude = prelude.replace('\'', "\"");
        let mode = if prelude == dark {
            "dark"
        } else if prelude == light {
            "light"
        } else {
            return Err(ThemeError::Scope);
        };
        let block = &rest[open + 1..];
        let close = find_outside_strings(block, "}").ok_or(ThemeError::Syntax)?;
        let inner = &block[..close];
        if find_outside_strings(inner, "{").is_some() {
            return Err(ThemeError::Scope);
        }
        rules.push(Rule {
            mode,
            declarations: declarations(inner)?,
        });
        rest = &block[close + 1..];
    }
    Ok(rules)
}

fn declarations(block: &str) -> Result<Vec<(String, String)>, ThemeError> {
    let mut out = Vec::new();
    let mut rest = block;
    loop {
        let end = find_outside_strings(rest, ";").unwrap_or(rest.len());
        let decl = rest[..end].trim();
        if !decl.is_empty() {
            let (name, value) = decl.split_once(':').ok_or(ThemeError::Syntax)?;
            let name = name.trim();
            let class = PROPERTIES
                .iter()
                .find(|(n, _)| *n == name)
                .map(|(_, c)| *c)
                .ok_or(ThemeError::Property)?;
            if find_outside_strings(value, "!").is_some() {
                return Err(ThemeError::Value);
            }
            let comps = Parser::new(value).components(0)?;
            check_class(class, &comps)?;
            out.push((name.to_owned(), serialize(&comps)));
            if out.len() > MAX_DECLARATIONS {
                return Err(ThemeError::Value);
            }
        }
        if end >= rest.len() {
            break;
        }
        rest = &rest[end + 1..];
    }
    Ok(out)
}

#[derive(Clone, Debug, PartialEq)]
enum Comp {
    Ident(String),
    Str(String),
    Hash(String),
    Num { value: String, unit: String },
    Func(String, Vec<Comp>),
    Comma,
    Slash,
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
    count: usize,
}

impl Parser {
    fn new(text: &str) -> Self {
        Self {
            chars: text.chars().collect(),
            pos: 0,
            count: 0,
        }
    }

    fn peek(&self, ahead: usize) -> Option<char> {
        self.chars.get(self.pos + ahead).copied()
    }

    /// Components until the end (depth 0) or the closing `)` of a function.
    fn components(&mut self, depth: usize) -> Result<Vec<Comp>, ThemeError> {
        let mut out = Vec::new();
        loop {
            while self.peek(0).is_some_and(char::is_whitespace) {
                self.pos += 1;
            }
            let Some(c) = self.peek(0) else {
                return if depth == 0 {
                    Ok(out)
                } else {
                    Err(ThemeError::Syntax)
                };
            };
            self.count += 1;
            if self.count > MAX_COMPONENTS {
                return Err(ThemeError::Value);
            }
            match c {
                ')' if depth > 0 => {
                    self.pos += 1;
                    return Ok(out);
                }
                ',' => {
                    self.pos += 1;
                    out.push(Comp::Comma);
                }
                '/' => {
                    self.pos += 1;
                    out.push(Comp::Slash);
                }
                '"' | '\'' => out.push(self.string(c)?),
                '#' => out.push(self.hash()?),
                c if c.is_ascii_digit()
                    || (c == '.' && self.peek(1).is_some_and(|d| d.is_ascii_digit()))
                    || ((c == '-' || c == '+')
                        && self.peek(1).is_some_and(|d| d.is_ascii_digit() || d == '.')) =>
                {
                    out.push(self.number()?)
                }
                c if c.is_ascii_alphabetic() || c == '-' || c == '_' => {
                    let name = self.ident();
                    if self.peek(0) == Some('(') {
                        self.pos += 1;
                        let lower = name.to_ascii_lowercase();
                        if lower == "url" {
                            return Err(ThemeError::ExternalUrl);
                        }
                        if !FUNCTIONS.contains(&lower.as_str()) {
                            return Err(ThemeError::Function);
                        }
                        if depth + 1 > MAX_DEPTH {
                            return Err(ThemeError::Value);
                        }
                        let args = if lower == "var" {
                            vec![Comp::Ident(self.var_target()?.to_owned())]
                        } else {
                            self.components(depth + 1)?
                        };
                        out.push(Comp::Func(lower, args));
                    } else {
                        if name.starts_with("--") || name == "-" {
                            return Err(ThemeError::Value);
                        }
                        out.push(Comp::Ident(name));
                    }
                }
                _ => return Err(ThemeError::Value),
            }
        }
    }

    fn string(&mut self, quote: char) -> Result<Comp, ThemeError> {
        self.pos += 1;
        let mut s = String::new();
        loop {
            match self.peek(0) {
                None | Some('\n') | Some('\r') => return Err(ThemeError::Syntax),
                Some(c) if c == quote => {
                    self.pos += 1;
                    break;
                }
                Some(c) if c == '"' || c == '\'' => return Err(ThemeError::Value),
                Some(c) => {
                    s.push(c);
                    self.pos += 1;
                }
            }
        }
        if s.trim().is_empty() || s.chars().count() > MAX_STRING {
            return Err(ThemeError::Value);
        }
        Ok(Comp::Str(s))
    }

    fn hash(&mut self) -> Result<Comp, ThemeError> {
        self.pos += 1;
        let mut s = String::new();
        while let Some(c) = self.peek(0).filter(char::is_ascii_alphanumeric) {
            s.push(c);
            self.pos += 1;
        }
        if ![3, 4, 6, 8].contains(&s.len()) || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(ThemeError::Value);
        }
        Ok(Comp::Hash(s.to_ascii_lowercase()))
    }

    fn number(&mut self) -> Result<Comp, ThemeError> {
        let mut value = String::new();
        if let Some(sign @ ('-' | '+')) = self.peek(0) {
            value.push(sign);
            self.pos += 1;
        }
        let mut dot = false;
        while let Some(c) = self.peek(0) {
            if c.is_ascii_digit() {
                value.push(c);
            } else if c == '.' && !dot && self.peek(1).is_some_and(|d| d.is_ascii_digit()) {
                dot = true;
                value.push(c);
            } else {
                break;
            }
            self.pos += 1;
        }
        let mut unit = String::new();
        if self.peek(0) == Some('%') {
            unit.push('%');
            self.pos += 1;
        } else {
            while let Some(c) = self.peek(0).filter(char::is_ascii_alphabetic) {
                unit.push(c.to_ascii_lowercase());
                self.pos += 1;
            }
        }
        if !unit.is_empty() && !UNITS.contains(&unit.as_str()) {
            return Err(ThemeError::Value);
        }
        Ok(Comp::Num { value, unit })
    }

    /// The single argument of `var()`: a documented theme custom property,
    /// no fallback, then `)`.
    fn var_target(&mut self) -> Result<&'static str, ThemeError> {
        while self.peek(0).is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
        let name = self.ident();
        while self.peek(0).is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
        if self.peek(0) != Some(')') {
            return Err(ThemeError::Value);
        }
        self.pos += 1;
        PROPERTIES
            .iter()
            .map(|(n, _)| *n)
            .find(|n| n.starts_with("--") && *n == name)
            .ok_or(ThemeError::Value)
    }

    fn ident(&mut self) -> String {
        let mut s = String::new();
        while let Some(c) = self
            .peek(0)
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        {
            s.push(c);
            self.pos += 1;
        }
        s
    }
}

fn is_func(comp: &Comp, set: &[&str]) -> bool {
    matches!(comp, Comp::Func(name, _) if set.contains(&name.as_str()))
}

/// The longest duration a theme's time token may set (PRD 0004 re-validation
/// S-01): room for the 900 ms spin and the old 400 ms defaults, never a
/// negative or effectively endless animation.
const MAX_TIME_MS: f64 = 2000.0;

/// A time value in milliseconds: a number in `ms` or `s`, or a bare zero.
fn time_ms(value: &str, unit: &str) -> Option<f64> {
    let number: f64 = value.parse().ok()?;
    let ms = match unit {
        "ms" => number,
        "s" => number * 1000.0,
        "" if number == 0.0 => 0.0,
        _ => return None,
    };
    ms.is_finite().then_some(ms)
}

fn check_class(class: Class, comps: &[Comp]) -> Result<(), ThemeError> {
    let ok = match class {
        Class::Color => matches!(comps, [c] if matches!(c, Comp::Hash(_) | Comp::Ident(_)) || is_func(c, COLOR_FUNCTIONS)),
        Class::Length => {
            matches!(comps, [c] if matches!(c, Comp::Num { .. }) || is_func(c, &["var"]))
        }
        Class::Time => matches!(comps, [c] if match c {
            Comp::Num { value, unit } => time_ms(value, unit).is_some_and(|ms| (0.0..=MAX_TIME_MS).contains(&ms)),
            other => is_func(other, &["var"]),
        }),
        Class::Easing => {
            matches!(comps, [c] if matches!(c, Comp::Ident(_)) || is_func(c, EASING_FUNCTIONS))
        }
        Class::Font => {
            !comps.is_empty()
                && comps.iter().all(|c| {
                    matches!(c, Comp::Ident(_) | Comp::Str(_) | Comp::Comma) || is_func(c, &["var"])
                })
        }
        Class::ColorScheme => {
            (1..=2).contains(&comps.len())
                && comps.iter().all(|c| {
                    matches!(c, Comp::Ident(i) if matches!(i.as_str(), "dark" | "light" | "normal" | "only"))
                })
        }
        Class::Paint => {
            !comps.is_empty()
                && comps
                    .iter()
                    .all(|c| !is_func(c, &["cubic-bezier", "steps", "linear"]))
        }
    };
    if ok {
        Ok(())
    } else {
        Err(ThemeError::Value)
    }
}

fn serialize(comps: &[Comp]) -> String {
    let mut out = String::new();
    for comp in comps {
        match comp {
            Comp::Comma => {
                out.push_str(", ");
                continue;
            }
            Comp::Slash => {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                out.push_str("/ ");
                continue;
            }
            _ => {}
        }
        if !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
        match comp {
            Comp::Ident(s) => out.push_str(s),
            Comp::Str(s) => {
                out.push('"');
                out.push_str(s);
                out.push('"');
            }
            Comp::Hash(s) => {
                out.push('#');
                out.push_str(s);
            }
            Comp::Num { value, unit } => {
                out.push_str(value);
                out.push_str(unit);
            }
            Comp::Func(name, args) => {
                out.push_str(name);
                out.push('(');
                out.push_str(&serialize(args));
                out.push(')');
            }
            Comp::Comma | Comp::Slash => unreachable!(),
        }
    }
    out
}

/// Label and group from the header (`label` and `group` strings, `modes`
/// an array of strings when present).
fn metadata(id: &str, header: Option<&str>) -> Result<(String, String), ThemeError> {
    let Some(header) = header else {
        return Ok((id.to_owned(), "custom".to_owned()));
    };
    let value: serde_json::Value =
        serde_json::from_str(header).map_err(|_| ThemeError::InvalidMeta)?;
    let object = value.as_object().ok_or(ThemeError::InvalidMeta)?;
    let text = |key: &str, max: usize, default: &str| -> Result<String, ThemeError> {
        match object.get(key) {
            None => Ok(default.to_owned()),
            Some(serde_json::Value::String(s)) => {
                let clean: String = s.chars().filter(|c| !c.is_control()).take(max).collect();
                let clean = clean.trim();
                Ok(if clean.is_empty() {
                    default.to_owned()
                } else {
                    clean.to_owned()
                })
            }
            Some(_) => Err(ThemeError::InvalidMeta),
        }
    };
    if let Some(modes) = object.get("modes") {
        let ok = modes
            .as_array()
            .is_some_and(|m| m.iter().all(serde_json::Value::is_string));
        if !ok {
            return Err(ThemeError::InvalidMeta);
        }
    }
    Ok((
        text("label", MAX_LABEL, id)?,
        text("group", MAX_GROUP, "custom")?,
    ))
}

#[cfg(test)]
#[path = "user_theme_tests.rs"]
mod tests;
