//! PRD 0017 RF2: the output contract of an extension. One run prints a
//! single JSON object to `stdout`:
//!
//! ```json
//! { "schema": 1, "status": "ready", "metrics": [...], "notes": [...] }
//! ```
//!
//! Each metric carries the fields of [`Metric`] (`label`, `used`, `limit`,
//! `remaining`, `unit`, `kind`, `window`, `period`, `resets_at`); a missing
//! or `null` value stays unknown, never zero. [`check`] treats the output as
//! untrusted data: it is at most [`MAX_BYTES`], with at most
//! [`MAX_METRICS`] metrics and [`MAX_NOTES`] notes, finite numbers, RFC 3339
//! dates, known `status`, `kind` and `window` names and a short `unit`;
//! labels, periods and notes lose their markup and are truncated. Fields
//! this version does not know are ignored, so the contract can grow within
//! schema 1. An output that breaks the contract becomes an `Error` reading
//! whose only note is [`INVALID_NOTE`] (the `invalid_output` failure
//! category); the violations name fields, never the printed content.
//!
//! The extension's `status` and texts never decide behavior: any status
//! other than `ready` is a plain `Error`, never a login or rate-limit
//! state, and its notes carry their own id, so no text can pose as a
//! Cuotai note.

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::{Map, Value};

use super::SCHEMA;
use crate::incidents::plain_text;
use crate::text::{params, Localized, Notes};
use crate::{
    Billing, Metric, MetricKind, MetricWindow, Provider, Report, Scope, ScopeKind, Status,
};

/// A larger output is refused unread.
pub const MAX_BYTES: usize = 256 * 1024;
/// The most metrics one output may carry.
pub const MAX_METRICS: usize = 64;
/// The most notes one output may carry.
pub const MAX_NOTES: usize = 16;
/// Longer labels are truncated.
pub const LABEL_MAX_CHARS: usize = 80;
/// Longer periods are truncated.
pub const PERIOD_MAX_CHARS: usize = 64;
/// Longer notes are truncated.
pub const NOTE_MAX_CHARS: usize = 500;
/// A longer unit breaks the contract.
pub const UNIT_MAX_CHARS: usize = 16;

/// Note id of a reading whose output broke the contract (the
/// `invalid_output` failure category of `events`).
pub(crate) const INVALID_NOTE: &str = "extensions.output.invalid";

const STATUSES: [Status; 6] = [
    Status::Ready,
    Status::NeedsConfiguration,
    Status::Unsupported,
    Status::AuthenticationError,
    Status::RateLimited,
    Status::Error,
];

const KINDS: [MetricKind; 6] = [
    MetricKind::Quota,
    MetricKind::RateLimit,
    MetricKind::Balance,
    MetricKind::Credits,
    MetricKind::Spend,
    MetricKind::Usage,
];

const BILLINGS: [Billing; 2] = [Billing::Included, Billing::Overage];

const SCOPE_KINDS: [ScopeKind; 1] = [ScopeKind::Model];

const WINDOWS: [MetricWindow; 5] = [
    MetricWindow::Window5h,
    MetricWindow::Daily,
    MetricWindow::Weekly,
    MetricWindow::Monthly,
    MetricWindow::Other,
];

/// One output checked against the contract: the normalized reading and
/// what broke the contract.
#[derive(Debug, Clone, Serialize)]
pub struct Checked {
    /// `extension:<id>`.
    pub source: String,
    /// `Ready` only when the output is valid and says `ready`.
    pub status: Status,
    /// The valid metrics; none unless `Ready`.
    pub metrics: Vec<Metric>,
    /// The readable notes: the extension's (cleaned and truncated) or, for
    /// an invalid output, only [`INVALID_NOTE`].
    pub notes: Vec<Localized>,
    /// Every contract violation, naming the field (such as
    /// `metrics[2].kind`) and never the printed value; empty when valid.
    pub violations: Vec<Localized>,
}

impl Checked {
    /// The reading as the [`Report`] of `account`.
    pub fn into_report(
        self,
        account: impl Into<String>,
        provider: Provider,
        checked_at: DateTime<Utc>,
    ) -> Report {
        let (notes, notes_l10n) = self.notes.into_iter().collect::<Notes>().finish();
        Report {
            account_id: None,
            diagnostic: None,
            account: account.into(),
            provider,
            checked_at,
            status: self.status,
            metrics: self.metrics,
            notes,
            notes_l10n,
            source: self.source,
            session_expires_at: None,
            sub_reports: Vec::new(),
            as_of: None,
            overage_hint: None,
        }
    }
}

/// RF2.2: the `stdout` of one run of the extension `id`, checked against
/// the contract.
pub fn check(id: &str, stdout: &[u8]) -> Checked {
    let source = format!("extension:{id}");
    match read(stdout) {
        Ok((status, metrics, notes)) => Checked {
            source,
            status,
            metrics,
            notes,
            violations: Vec::new(),
        },
        Err(violations) => Checked {
            source,
            status: Status::Error,
            metrics: Vec::new(),
            notes: vec![invalid(violations.len())],
            violations,
        },
    }
}

type Reading = (Status, Vec<Metric>, Vec<Localized>);

fn read(stdout: &[u8]) -> Result<Reading, Vec<Localized>> {
    if stdout.len() > MAX_BYTES {
        return Err(vec![Localized::with_param(
            "extensions.output.too_large",
            "max",
            MAX_BYTES / 1024,
            format!("A saída da extensão passa de {} KiB.", MAX_BYTES / 1024),
        )]);
    }
    let Ok(text) = std::str::from_utf8(stdout) else {
        return Err(vec![Localized::fixed(
            "extensions.output.not_utf8",
            "A saída da extensão não é texto UTF-8.",
        )]);
    };
    if text.trim().is_empty() {
        return Err(vec![Localized::fixed(
            "extensions.output.empty",
            "A extensão não imprimiu nada na saída padrão.",
        )]);
    }
    let value: Value = serde_json::from_str(text).map_err(|error| {
        let (line, column) = (error.line(), error.column());
        vec![Localized::with_params(
            "extensions.output.json",
            params([("line", line.into()), ("column", column.into())]),
            format!(
                "A saída da extensão não é um JSON válido (linha {line}, coluna {column}); \
                 NaN e Infinity não são números JSON."
            ),
        )]
    })?;
    let Value::Object(object) = value else {
        return Err(vec![Localized::fixed(
            "extensions.output.not_object",
            "A saída da extensão deve ser um único objeto JSON.",
        )]);
    };
    // A newer contract may change any field: nothing else is read.
    if object.get("schema").and_then(Value::as_u64) != Some(u64::from(SCHEMA)) {
        return Err(vec![Localized::with_param(
            "extensions.output.schema",
            "schema",
            SCHEMA.to_string(),
            format!("A saída da extensão precisa de \"schema\": {SCHEMA}."),
        )]);
    }
    let mut check = Violations::default();
    let status = match object.get("status") {
        None | Some(Value::Null) => check.missing("status".into()),
        Some(value) => check.choice(value, "status".into(), &STATUSES),
    };
    let metrics = check.metrics(&object);
    let notes = check.notes(&object);
    let (Some(status), Some(metrics), Some(notes), true) =
        (status, metrics, notes, check.0.is_empty())
    else {
        return Err(check.0);
    };
    let notes = notes
        .into_iter()
        .map(|text| Localized::with_param("extensions.output.note", "text", text.clone(), text))
        .collect();
    if status == Status::Ready {
        return Ok((Status::Ready, metrics, notes));
    }
    let name = slug(status);
    let mut all = vec![Localized::with_param(
        "extensions.output.failed",
        "status",
        name.as_str(),
        format!("A extensão informou o status {name}; nenhum valor foi usado."),
    )];
    all.extend(notes);
    Ok((Status::Error, Vec::new(), all))
}

/// The violations found so far; each check returns `None` after recording
/// its own.
#[derive(Default)]
struct Violations(Vec<Localized>);

impl Violations {
    fn metrics(&mut self, object: &Map<String, Value>) -> Option<Vec<Metric>> {
        let items = match object.get("metrics") {
            None | Some(Value::Null) => return self.missing("metrics".into()),
            Some(Value::Array(items)) => items,
            Some(_) => return self.fail(field(Shape::List, "metrics".into())),
        };
        if items.len() > MAX_METRICS {
            return self.fail(too_many("metrics", MAX_METRICS));
        }
        let metrics: Vec<Option<Metric>> = items
            .iter()
            .enumerate()
            .map(|(index, item)| self.metric(index, item))
            .collect();
        metrics.into_iter().collect()
    }

    fn metric(&mut self, index: usize, item: &Value) -> Option<Metric> {
        let at = |key: &str| format!("metrics[{index}].{key}");
        let Value::Object(fields) = item else {
            return self.fail(field(Shape::Object, format!("metrics[{index}]")));
        };
        let label = match fields.get("label") {
            None | Some(Value::Null) => self.missing(at("label")),
            Some(value) => self.text(value, at("label"), LABEL_MAX_CHARS),
        };
        let used = self.number(fields.get("used"), at("used"));
        let limit = self.number(fields.get("limit"), at("limit"));
        let remaining = self.number(fields.get("remaining"), at("remaining"));
        let unit = match fields.get("unit") {
            None | Some(Value::Null) => self.missing(at("unit")),
            Some(value) => self.unit(value, at("unit")),
        };
        let kind = match fields.get("kind") {
            None | Some(Value::Null) => self.missing(at("kind")),
            Some(value) => self.choice(value, at("kind"), &KINDS),
        };
        let window = match fields.get("window") {
            None | Some(Value::Null) => Some(None),
            Some(value) => self.choice(value, at("window"), &WINDOWS).map(Some),
        };
        let period = match fields.get("period") {
            None | Some(Value::Null) => Some(None),
            Some(value) => match value {
                Value::String(text) => {
                    Some(Some(plain_text(text, PERIOD_MAX_CHARS)).filter(|text| !text.is_empty()))
                }
                _ => self.fail(field(Shape::Text, at("period"))),
            },
        };
        let resets_at = match fields.get("resets_at") {
            None | Some(Value::Null) => Some(None),
            Some(value) => value
                .as_str()
                .and_then(|text| DateTime::parse_from_rfc3339(text).ok())
                .map(|time| Some(time.with_timezone(&Utc)))
                .or_else(|| self.fail(field(Shape::Date, at("resets_at")))),
        };
        let billing = match fields.get("billing") {
            None | Some(Value::Null) => Some(Billing::Included),
            Some(value) => self.choice(value, at("billing"), &BILLINGS),
        };
        let scope = match fields.get("scope") {
            None | Some(Value::Null) => Some(None),
            Some(Value::Object(scope)) => {
                let kind = match scope.get("kind") {
                    None | Some(Value::Null) => self.missing(at("scope.kind")),
                    Some(value) => self.choice(value, at("scope.kind"), &SCOPE_KINDS),
                };
                let id = match scope.get("id") {
                    None | Some(Value::Null) => self.missing(at("scope.id")),
                    Some(value) => self.text(value, at("scope.id"), LABEL_MAX_CHARS),
                };
                let label = match scope.get("label") {
                    None | Some(Value::Null) => self.missing(at("scope.label")),
                    Some(value) => self.text(value, at("scope.label"), LABEL_MAX_CHARS),
                };
                match (kind, id, label) {
                    (Some(kind), Some(id), Some(label)) => Some(Some(Scope { kind, id, label })),
                    _ => None,
                }
            }
            Some(_) => self.fail(field(Shape::Object, at("scope"))),
        };
        Some(Metric {
            billing: billing?,
            scope: scope?,
            label: label?,
            used: used?,
            limit: limit?,
            remaining: remaining?,
            unit: unit?,
            kind: kind?,
            window: window?,
            period: period?,
            resets_at: resets_at?,
        })
    }

    fn notes(&mut self, object: &Map<String, Value>) -> Option<Vec<String>> {
        let items = match object.get("notes") {
            None | Some(Value::Null) => return Some(Vec::new()),
            Some(Value::Array(items)) => items,
            Some(_) => return self.fail(field(Shape::List, "notes".into())),
        };
        if items.len() > MAX_NOTES {
            return self.fail(too_many("notes", MAX_NOTES));
        }
        let mut notes = Vec::with_capacity(items.len());
        let mut valid = true;
        for (index, item) in items.iter().enumerate() {
            match item {
                Value::String(text) => notes.push(plain_text(text, NOTE_MAX_CHARS)),
                _ => {
                    valid = false;
                    self.0.push(field(Shape::Text, format!("notes[{index}]")));
                }
            }
        }
        notes.retain(|note| !note.is_empty());
        valid.then_some(notes)
    }

    /// A string without markup, truncated to `max` characters, that is not
    /// empty once cleaned.
    fn text(&mut self, value: &Value, path: String, max: usize) -> Option<String> {
        let Value::String(text) = value else {
            return self.fail(field(Shape::Text, path));
        };
        let text = plain_text(text, max);
        if text.is_empty() {
            return self.fail(field(Shape::Empty, path));
        }
        Some(text)
    }

    fn unit(&mut self, value: &Value, path: String) -> Option<String> {
        let unit = value.as_str().map(str::trim).filter(|unit| {
            unit.chars().count() <= UNIT_MAX_CHARS && !unit.chars().any(char::is_control)
        });
        match unit {
            Some(unit) => Some(unit.to_owned()),
            None => self.fail(Localized::with_params(
                "extensions.output.unit",
                params([
                    ("field", path.as_str().into()),
                    ("max", UNIT_MAX_CHARS.into()),
                ]),
                format!(
                    "O campo {path} deve ser um texto de até {UNIT_MAX_CHARS} caracteres, sem \
                     caracteres de controle."
                ),
            )),
        }
    }

    /// `None` inside when absent or `null`: an unknown value, never zero.
    fn number(&mut self, value: Option<&Value>, path: String) -> Option<Option<f64>> {
        match value {
            None | Some(Value::Null) => Some(None),
            Some(value) => match value.as_f64().filter(|number| number.is_finite()) {
                Some(number) => Some(Some(number)),
                None => self.fail(field(Shape::Number, path)),
            },
        }
    }

    /// One of `options`, by its serialized name.
    fn choice<T: Serialize + Copy>(
        &mut self,
        value: &Value,
        path: String,
        options: &[T],
    ) -> Option<T> {
        if let Some(found) = value
            .as_str()
            .and_then(|name| options.iter().find(|option| slug(**option) == name))
        {
            return Some(*found);
        }
        let values = options
            .iter()
            .map(|option| slug(*option))
            .collect::<Vec<_>>()
            .join(", ");
        self.fail(Localized::with_params(
            "extensions.output.choice",
            params([
                ("field", path.as_str().into()),
                ("values", values.as_str().into()),
            ]),
            format!("O campo {path} deve ser um destes: {values}."),
        ))
    }

    fn missing<T>(&mut self, path: String) -> Option<T> {
        self.fail(Localized::with_param(
            "extensions.output.missing",
            "field",
            path.as_str(),
            format!("Falta o campo {path}."),
        ))
    }

    fn fail<T>(&mut self, violation: Localized) -> Option<T> {
        self.0.push(violation);
        None
    }
}

/// The serialized name of a contract enum.
fn slug<T: Serialize>(value: T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(name)) => name,
        _ => String::new(),
    }
}

/// What a field that breaks the contract should have been.
#[derive(Clone, Copy)]
enum Shape {
    List,
    Object,
    Text,
    Empty,
    Number,
    Date,
}

/// `extensions.output.<shape>`: the field at `path` does not have the
/// expected shape.
fn field(shape: Shape, path: String) -> Localized {
    let (id, pt) = match shape {
        Shape::List => ("list", format!("O campo {path} deve ser uma lista.")),
        Shape::Object => ("object", format!("O item {path} deve ser um objeto.")),
        Shape::Text => ("text", format!("O campo {path} deve ser um texto.")),
        Shape::Empty => (
            "empty_text",
            format!("O campo {path} fica vazio sem a marcação."),
        ),
        Shape::Number => (
            "number",
            format!("O campo {path} deve ser um número finito ou null."),
        ),
        Shape::Date => (
            "date",
            format!(
                "O campo {path} deve ser uma data RFC 3339, como 2026-10-21T12:00:00Z, ou null."
            ),
        ),
    };
    Localized::with_param(
        format!("extensions.output.{id}"),
        "field",
        path.as_str(),
        pt,
    )
}

fn too_many(path: &'static str, max: usize) -> Localized {
    Localized::with_params(
        "extensions.output.too_many",
        params([("field", path.into()), ("max", max.into())]),
        format!("O campo {path} aceita no máximo {max} itens."),
    )
}

fn invalid(count: usize) -> Localized {
    Localized::with_params(
        INVALID_NOTE,
        params([
            ("schema", SCHEMA.to_string().into()),
            ("count", count.into()),
        ]),
        format!(
            "A saída da extensão não segue o contrato (schema {SCHEMA}): {count} problema(s); \
             nenhum valor foi usado."
        ),
    )
}

#[cfg(test)]
#[path = "output_tests.rs"]
mod tests;
