//! PRD 0023 re-validation (V-02): amounts formatted in the reader's
//! language.
//!
//! An amount travels as a nested [`Localized`] whose params are machine
//! values (a dot-decimal number with no grouping, an ISO 4217 code, a
//! unit), so the `export::v1` shape of `Localized` does not change:
//!
//! - `amount.money` `{value, currency}`: `$6.25`, `6,25 $`, `US$ 6,25`;
//! - `amount.number` `{value}`: `1,234.5`, `1.234,5`;
//! - `amount.units` `{value, unit}`: `12.5 credits`, a no-break space between
//!   the number and its unit (PRD 0034 re-validation V-10). `unit` is a
//!   plain text or a nested `Localized` (`metric.unit.credits`).
//!
//! A share in percent (PRD 0034 re-validation V-17) is a catalog text,
//! `number.percent` `{p}`, whose `p` is an `amount.number`: each language
//! places the sign its own way (`64%`, `64 %` in German, `%64` in Turkish).
//!
//! [`Localized::resolve`] renders these ids here instead of through a
//! catalog, and the desktop renders them with `Intl.NumberFormat`. The
//! table below is a small hand-written subset of CLDR for the 14 built-in
//! languages (no ICU dependency): decimal and group separators, Indian
//! grouping, the Spanish minimum grouping, and where the currency sign
//! goes. The TUI and `--once` write every metric figure with
//! [`format_number`] too (PRD 0034 re-validation V-07).

use super::{params, Localized, ParamValue};

/// The id of an amount of money.
pub const MONEY: &str = "amount.money";
/// The id of a plain number.
pub const NUMBER: &str = "amount.number";
/// The id of a number followed by a unit.
pub const UNITS: &str = "amount.units";

const NBSP: &str = "\u{a0}";
const NNBSP: &str = "\u{202f}";

#[derive(Clone, Copy)]
enum Grouping {
    /// Groups of three.
    Three,
    /// Groups of three only from five integer digits (es: `1234`, `12.345`).
    MinTwo,
    /// 3 then 2 (hi-IN: `12,34,567`).
    Indian,
}

#[derive(Clone, Copy)]
struct Format {
    decimal: &'static str,
    group: &'static str,
    grouping: Grouping,
    /// The currency sign comes before the number.
    prefix: bool,
    /// What separates the sign from the number.
    gap: &'static str,
    /// How the language writes the US dollar.
    usd: &'static str,
}

const fn format(
    decimal: &'static str,
    group: &'static str,
    grouping: Grouping,
    prefix: bool,
    gap: &'static str,
    usd: &'static str,
) -> Format {
    Format {
        decimal,
        group,
        grouping,
        prefix,
        gap,
        usd,
    }
}

fn locale_format(code: &str) -> Format {
    use Grouping::{Indian, MinTwo, Three};
    match code {
        "pt-BR" => format(",", ".", Three, true, NBSP, "US$"),
        "es" => format(",", ".", MinTwo, false, NBSP, "US$"),
        "it" => format(",", ".", Three, false, NBSP, "USD"),
        "de" => format(",", ".", Three, false, NBSP, "$"),
        "ru" => format(",", NBSP, Three, false, NBSP, "$"),
        "fr" => format(",", NNBSP, Three, false, NBSP, "$US"),
        "vi" => format(",", ".", Three, false, NBSP, "US$"),
        "id-ID" => format(",", ".", Three, true, "", "US$"),
        "tr" => format(",", ".", Three, true, "", "$"),
        "ko" | "zh-CN" => format(".", ",", Three, true, "", "US$"),
        "hi-IN" => format(".", ",", Indian, true, "", "$"),
        // en-US, ja, the pseudo-locale and unknown codes.
        _ => format(".", ",", Three, true, "", "$"),
    }
}

/// The sign of `currency` in `code`, `None` to write the ISO code itself.
fn symbol(code: &str, currency: &str, usd: &'static str) -> Option<&'static str> {
    Some(match (currency, code) {
        ("USD", _) => usd,
        ("EUR", _) => "€",
        ("GBP", _) => "£",
        ("BRL", _) => "R$",
        ("JPY", "ja") => "￥",
        ("JPY", _) => "¥",
        ("CNY", "zh-CN") => "¥",
        ("CNY", _) => "CN¥",
        ("INR", _) => "₹",
        ("KRW", _) => "₩",
        ("RUB", "ru") => "₽",
        ("TRY", "tr") => "₺",
        ("VND", _) => "₫",
        ("IDR", "id-ID") => "Rp",
        _ => return None,
    })
}

/// ISO 4217 minor units of the currencies that have none.
/// The smallest amount `currency` shows (`0.01` USD, `1` JPY): a positive
/// amount below half of it reads as zero.
pub fn smallest_money(currency: &str) -> f64 {
    10f64.powi(-(fraction_digits(&currency.to_ascii_uppercase()) as i32))
}

fn fraction_digits(currency: &str) -> usize {
    match currency {
        "JPY" | "KRW" | "VND" | "CLP" | "ISK" | "HUF" | "TWD" | "UGX" => 0,
        _ => 2,
    }
}

fn group_digits(digits: &str, format: Format) -> String {
    let len = digits.len();
    let grouped = match format.grouping {
        Grouping::MinTwo => len >= 5,
        _ => len >= 4,
    };
    if !grouped {
        return digits.to_owned();
    }
    let mut cuts = Vec::new();
    let mut end = len;
    let mut size = 3;
    while end > size {
        end -= size;
        cuts.push(end);
        if matches!(format.grouping, Grouping::Indian) {
            size = 2;
        }
    }
    let mut out = String::with_capacity(len + cuts.len() * 3);
    for (i, ch) in digits.char_indices() {
        if cuts.contains(&i) {
            out.push_str(format.group);
        }
        out.push(ch);
    }
    out
}

/// `value` with `min..=max` fraction digits, in the separators of `code`.
/// The sign is returned apart so a currency sign can go between.
fn digits(value: f64, code: &str, min: usize, max: usize) -> (bool, String) {
    let format = locale_format(code);
    let fixed = format!("{:.*}", max, value.abs());
    let (int, frac) = fixed.split_once('.').unwrap_or((&fixed, ""));
    let mut frac = frac.to_owned();
    while frac.len() > min && frac.ends_with('0') {
        frac.pop();
    }
    let negative = value < 0.0 && fixed.bytes().any(|b| matches!(b, b'1'..=b'9'));
    let mut out = group_digits(int, format);
    if !frac.is_empty() {
        out.push_str(format.decimal);
        out.push_str(&frac);
    }
    (negative, out)
}

/// `value` as a person reads it in `locale`: grouped, up to two decimals.
pub fn format_number(value: f64, locale: &str) -> String {
    if !value.is_finite() {
        return value.to_string();
    }
    let (negative, text) = digits(value, super::Locale::resolve(locale).code(), 0, 2);
    if negative {
        format!("-{text}")
    } else {
        text
    }
}

/// `value` of `currency` (an ISO 4217 code) as a person reads it in
/// `locale`: `$6.25`, `6,25 $`, `US$ 6,25`, `1 234,56 $US`.
pub fn format_money(value: f64, currency: &str, locale: &str) -> String {
    let currency = currency.to_ascii_uppercase();
    if !value.is_finite() {
        return format!("{value} {currency}");
    }
    let code = super::Locale::resolve(locale).code();
    let format = locale_format(code);
    let places = fraction_digits(&currency);
    let (negative, number) = digits(value, code, places, places);
    let minus = if negative { "-" } else { "" };
    match symbol(code, &currency, format.usd) {
        Some(sign) if format.prefix => format!("{minus}{sign}{}{number}", format.gap),
        Some(sign) => format!("{minus}{number}{}{sign}", format.gap),
        // An ISO code is always set apart from the number.
        None if format.prefix => format!("{minus}{currency}{NBSP}{number}"),
        None => format!("{minus}{number}{NBSP}{currency}"),
    }
}

/// The machine form of a value param: dot decimal, no grouping, at most
/// four decimals, no trailing zeros.
fn machine(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" {
        "0".to_owned()
    } else {
        text.to_owned()
    }
}

fn source() -> &'static str {
    super::source_locale().code
}

/// An amount of money, rendered per locale; its fallback is the en-US form.
pub fn money(value: f64, currency: &str) -> Localized {
    let currency = currency.to_ascii_uppercase();
    Localized {
        fallback: format_money(value, &currency, source()).into(),
        id: MONEY.into(),
        params: params([
            ("value", ParamValue::Text(machine(value))),
            ("currency", ParamValue::Text(currency)),
        ]),
    }
}

/// A plain number, rendered per locale.
pub fn number(value: f64) -> Localized {
    Localized {
        fallback: format_number(value, source()).into(),
        id: NUMBER.into(),
        params: params([("value", ParamValue::Text(machine(value)))]),
    }
}

/// A number with its unit (`12.5 credits`), the number rendered per locale.
pub fn units(value: f64, unit: &str) -> Localized {
    Localized {
        fallback: format!("{}{NBSP}{unit}", format_number(value, source())).into(),
        id: UNITS.into(),
        params: params([
            ("value", ParamValue::Text(machine(value))),
            ("unit", ParamValue::Text(unit.to_owned())),
        ]),
    }
}

/// A number with a unit that is a text of its own (`metric.unit.credits`),
/// so the unit reads in the reader's language too (PRD 0034 re-validation,
/// G13). The fallback is the English form.
pub fn units_text(value: f64, unit: Localized) -> Localized {
    Localized {
        fallback: format!("{}{NBSP}{}", format_number(value, source()), unit.fallback).into(),
        id: UNITS.into(),
        params: params([
            ("value", ParamValue::Text(machine(value))),
            ("unit", ParamValue::l10n(unit)),
        ]),
    }
}

/// A share already in percent (`64.0` for 64 %), rendered per locale:
/// `64%`, `64\u{a0}%` (de), `%64` (tr). The figure keeps up to two decimals;
/// callers round it first when they show whole percents.
pub fn percent(value: f64) -> Localized {
    Localized::with_param(
        "number.percent",
        "p",
        ParamValue::l10n(number(value)),
        "{p}%",
    )
}

/// A percent sign around a figure the caller already wrote (`<1`), placed
/// as `locale`'s [`percent`] places it.
pub fn percent_of(figure: &str) -> Localized {
    Localized::with_param("number.percent", "p", figure, "{p}%")
}

/// [`percent`] in `locale`.
pub fn format_percent(value: f64, locale: &str) -> String {
    percent(value).resolve(locale)
}

/// PRD 0041 re-validation (F-05): why a typed amount was not read.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AmountProblem {
    /// Empty, not a number, an exponent, or separators in a shape no
    /// language writes (`1,2.3`, `1.00.0`).
    Invalid,
    /// One separator followed by exactly three digits (`1,000`, `1.500`):
    /// a thousands group in some languages and a decimal in others. Both
    /// readings travel so the message can name them.
    Ambiguous { whole: f64, decimal: f64 },
}

fn is_space_group(c: char) -> bool {
    matches!(c, ' ' | '\u{a0}' | '\u{202f}' | '\u{2009}')
}

fn all_digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// Whether the groups after the first are thousands groups (Indian
/// grouping, groups of two before the last, also passes).
fn grouped(groups: &[&str]) -> bool {
    let Some((first, rest)) = groups.split_first() else {
        return false;
    };
    let Some((last, middle)) = rest.split_last() else {
        return all_digits(first);
    };
    (1..=3).contains(&first.len())
        && all_digits(first)
        && last.len() == 3
        && all_digits(last)
        && (middle.iter().all(|g| g.len() == 3 && all_digits(g))
            || middle.iter().all(|g| g.len() == 2 && all_digits(g)))
}

/// PRD 0041 re-validation (F-05): an amount as a person types it, in any
/// of the built-in languages' separators: `1000`, `12,5`, `1.000,50`,
/// `1,000.50`, `1 000` (a space, no-break space or narrow no-break space
/// groups digits). With both `,` and `.` the last one is the decimal; a
/// separator that repeats groups thousands. One separator followed by
/// exactly three digits (`1,000`, `1.000`) is refused as
/// [`AmountProblem::Ambiguous`] whatever the language, unless spaces
/// already group the digits or the whole part is 0. Exponents, signs other
/// than a leading `-`, and letters are [`AmountProblem::Invalid`].
/// `_locale` is accepted for the callers' symmetry with [`amount_input`];
/// the reading does not depend on it.
pub fn parse_amount(text: &str, _locale: &str) -> Result<f64, AmountProblem> {
    let text = text.trim();
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest.trim_start()),
        None => (false, text),
    };
    let spaced = body.chars().any(is_space_group);
    let compact: String = body.chars().filter(|c| !is_space_group(*c)).collect();
    if compact.is_empty()
        || !compact
            .chars()
            .all(|c| c.is_ascii_digit() || c == ',' || c == '.')
    {
        return Err(AmountProblem::Invalid);
    }
    if spaced {
        // Spaces group the whole part only ("1 000,50"); the one separator
        // left, if any, is then the decimal.
        let int_end = body.find([',', '.']).unwrap_or(body.len());
        let (int, rest) = body.split_at(int_end);
        let groups: Vec<&str> = int.split(is_space_group).collect();
        if !grouped(&groups)
            || rest.chars().any(is_space_group)
            || rest.matches([',', '.']).count() > 1
        {
            return Err(AmountProblem::Invalid);
        }
    }
    let commas = compact.matches(',').count();
    let dots = compact.matches('.').count();
    let number = |int: &str, frac: &str| -> Result<f64, AmountProblem> {
        let int = if int.is_empty() { "0" } else { int };
        let plain = if frac.is_empty() {
            int.to_owned()
        } else {
            format!("{int}.{frac}")
        };
        let value: f64 = plain.parse().map_err(|_| AmountProblem::Invalid)?;
        let value = if negative { -value } else { value };
        value
            .is_finite()
            .then_some(value)
            .ok_or(AmountProblem::Invalid)
    };
    let split_group = |int: &str, group: char| -> Result<String, AmountProblem> {
        let groups: Vec<&str> = int.split(group).collect();
        if grouped(&groups) {
            Ok(groups.concat())
        } else {
            Err(AmountProblem::Invalid)
        }
    };
    match (commas, dots) {
        (0, 0) => number(&compact, ""),
        (c, d) if c > 0 && d > 0 => {
            // The last separator is the decimal; it appears once.
            let last = compact.rfind([',', '.']).unwrap_or(0);
            let decimal = compact[last..].chars().next().unwrap_or('.');
            let group = if decimal == ',' { '.' } else { ',' };
            if compact.matches(decimal).count() != 1 {
                return Err(AmountProblem::Invalid);
            }
            let (int, frac) = (&compact[..last], &compact[last + 1..]);
            if !frac.bytes().all(|b| b.is_ascii_digit()) {
                return Err(AmountProblem::Invalid);
            }
            number(&split_group(int, group)?, frac)
        }
        (1, 0) | (0, 1) => {
            let at = compact.find([',', '.']).unwrap_or(0);
            let (int, frac) = (&compact[..at], &compact[at + 1..]);
            if frac.len() == 3 && !spaced && !int.trim_start_matches('0').is_empty() {
                let whole = number(&format!("{int}{frac}"), "")?;
                let decimal = number(int, frac)?;
                return Err(AmountProblem::Ambiguous { whole, decimal });
            }
            number(int, frac)
        }
        (_, 0) => number(&split_group(&compact, ',')?, ""),
        _ => number(&split_group(&compact, '.')?, ""),
    }
}

/// PRD 0041 re-validation (F-05): `value` as an editable amount field in
/// `locale`: the language's decimal separator, no grouping, and a fourth
/// fraction digit after exactly three (`1.1250`), so the text reads back
/// as the same number through [`parse_amount`].
pub fn amount_input(value: f64, locale: &str) -> String {
    if !value.is_finite() {
        return String::new();
    }
    let plain = format!("{value}");
    let decimal = locale_format(super::Locale::resolve(locale).code()).decimal;
    match plain.split_once('.') {
        Some((int, frac)) if frac.len() == 3 => format!("{int}{decimal}{frac}0"),
        Some((int, frac)) => format!("{int}{decimal}{frac}"),
        None => plain,
    }
}

#[cfg(test)]
#[path = "number_amount_tests.rs"]
mod amount_tests;

fn text_param<'a>(note: &'a Localized, key: &str) -> Option<&'a str> {
    match note.params.get(key)? {
        ParamValue::Text(text) => Some(text),
        ParamValue::L10n(_) => None,
    }
}

/// The text of an `amount.*` note in `locale`; `None` for any other id or
/// a malformed value (the caller then shows the fallback).
pub(super) fn render(note: &Localized, locale: &str) -> Option<String> {
    let id = &*note.id;
    if !id.starts_with("amount.") {
        return None;
    }
    let value: f64 = text_param(note, "value")?.parse().ok()?;
    match id {
        MONEY => {
            let currency = text_param(note, "currency")?;
            crate::budget::is_currency(currency).then(|| format_money(value, currency, locale))
        }
        NUMBER => Some(format_number(value, locale)),
        UNITS => {
            let unit = match note.params.get("unit")? {
                ParamValue::Text(text) => text.clone(),
                ParamValue::L10n(unit) => unit.resolve(locale),
            };
            Some(format!("{}{NBSP}{unit}", format_number(value, locale)))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn money_follows_each_language() {
        let cases = [
            ("en-US", "USD", 6.25, "$6.25"),
            ("en-US", "EUR", 1234.5, "€1,234.50"),
            ("en-US", "CHF", 6.25, "CHF\u{a0}6.25"),
            ("pt-BR", "USD", 6.25, "US$\u{a0}6,25"),
            ("pt-BR", "BRL", 12345.67, "R$\u{a0}12.345,67"),
            ("de", "USD", 12345.67, "12.345,67\u{a0}$"),
            ("de", "EUR", 6.25, "6,25\u{a0}€"),
            ("es", "USD", 1234.5, "1234,50\u{a0}US$"),
            ("es", "USD", 12345.5, "12.345,50\u{a0}US$"),
            ("fr", "USD", 1234.5, "1\u{202f}234,50\u{a0}$US"),
            ("ru", "USD", 1234.5, "1\u{a0}234,50\u{a0}$"),
            ("ja", "USD", 6.25, "$6.25"),
            ("ja", "JPY", 1234.4, "￥1,234"),
            ("hi-IN", "USD", 1234567.0, "$12,34,567.00"),
            ("zh-CN", "USD", 6.25, "US$6.25"),
            ("en-US", "USD", -6.25, "-$6.25"),
            ("de", "USD", -0.001, "0,00\u{a0}$"),
        ];
        for (locale, currency, value, expected) in cases {
            assert_eq!(
                format_money(value, currency, locale),
                expected,
                "{locale} {currency} {value}"
            );
        }
    }

    #[test]
    fn numbers_group_and_trim() {
        assert_eq!(format_number(1234.5, "en-US"), "1,234.5");
        assert_eq!(format_number(1234.5, "de"), "1.234,5");
        assert_eq!(format_number(12.0, "pt-BR"), "12");
        assert_eq!(format_number(0.126, "ja"), "0.13");
        assert_eq!(format_number(1234.0, "es"), "1234");
    }

    #[test]
    fn percents_place_the_sign_per_language() {
        let cases = [
            ("en-US", "64%"),
            ("de", "64\u{a0}%"),
            ("fr", "64\u{a0}%"),
            ("tr", "%64"),
            ("ru", "64\u{a0}%"),
            ("ja", "64%"),
        ];
        for (locale, expected) in cases {
            assert_eq!(format_percent(64.0, locale), expected, "{locale}");
        }
        assert_eq!(format_percent(12.5, "de"), "12,5\u{a0}%");
        assert_eq!(format_percent(1234.0, "en-US"), "1,234%");
        assert_eq!(percent_of("<1").resolve("tr"), "%<1");
        assert_eq!(percent(64.0).fallback, "64%");
        // An unknown language reads the English form.
        assert_eq!(format_percent(64.0, "xx"), "64%");
    }

    #[test]
    fn amounts_resolve_per_locale_and_fall_back_to_en_us() {
        let usd = money(6.25, "usd");
        assert_eq!(usd.fallback, "$6.25");
        assert_eq!(usd.resolve("pt-BR"), "US$\u{a0}6,25");
        assert_eq!(usd.resolve("de"), "6,25\u{a0}$");
        assert_eq!(usd.resolve("en-XA"), "$6.25");
        let json = serde_json::to_value(&usd).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": "amount.money",
                "params": {"value": "6.25", "currency": "USD"},
                "fallback": "$6.25"
            })
        );
        let units = units(1234.5, "credits");
        assert_eq!(units.fallback, "1,234.5\u{a0}credits");
        assert_eq!(units.resolve("de"), "1.234,5\u{a0}credits");
        assert_eq!(number(1234.5).resolve("fr"), "1\u{202f}234,5");
        // Nested inside another text, the amount follows the outer locale.
        let outer = Localized::with_param("x.y", "amount", money(6.25, "USD"), "Spent {amount}");
        assert_eq!(outer.fallback, "Spent $6.25");
        // A malformed value shows the fallback, never a made-up figure.
        let mut broken = money(6.25, "USD");
        broken
            .params
            .insert("value".into(), ParamValue::Text("six".into()));
        assert_eq!(broken.resolve("de"), "$6.25");
    }
}
