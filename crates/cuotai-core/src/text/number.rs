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
//!   the number and its unit (PRD 0034 re-validation V-10).
//!
//! [`Localized::resolve`] renders these ids here instead of through a
//! catalog, and the desktop renders them with `Intl.NumberFormat`. The
//! table below is a small hand-written subset of CLDR for the 14 built-in
//! languages (no ICU dependency): decimal and group separators, Indian
//! grouping, the Spanish minimum grouping, and where the currency sign
//! goes. It formats amounts only; the other metric numbers of the TUI keep
//! their plain `12.50` form (PRD 0023 open item).

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
        UNITS => Some(format!(
            "{}{NBSP}{}",
            format_number(value, locale),
            text_param(note, "unit")?
        )),
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
