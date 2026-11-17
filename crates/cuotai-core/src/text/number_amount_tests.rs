//! PRD 0041 re-validation (F-05): typed amounts are read the way people
//! write them, and an amount that reads two ways is refused.

use super::*;

#[test]
fn amounts_are_read_the_way_they_are_written() {
    let ok = [
        ("1000", 1000.0),
        ("12,5", 12.5),
        ("12.5", 12.5),
        ("1.000,50", 1000.5),
        ("1,000.50", 1000.5),
        ("1.234.567,8", 1_234_567.8),
        ("1,234,567", 1_234_567.0),
        ("1.000.000", 1_000_000.0),
        ("1,00,000.25", 100_000.25),
        ("1 000", 1000.0),
        ("1\u{a0}000", 1000.0),
        ("1\u{202f}000,5", 1000.5),
        ("1 000,500", 1000.5),
        ("0,500", 0.5),
        (",5", 0.5),
        ("1.1250", 1.125),
        ("  40 ", 40.0),
        ("-5", -5.0),
    ];
    for locale in ["en-US", "pt-BR", "de", "fr"] {
        for (text, want) in ok {
            assert_eq!(parse_amount(text, locale), Ok(want), "{text} in {locale}");
        }
    }
}

#[test]
fn one_separator_before_three_digits_is_ambiguous_in_every_language() {
    for locale in ["en-US", "pt-BR", "de", "fr", "ja"] {
        assert_eq!(
            parse_amount("1,000", locale),
            Err(AmountProblem::Ambiguous {
                whole: 1000.0,
                decimal: 1.0
            }),
            "{locale}"
        );
        assert_eq!(
            parse_amount("1.500", locale),
            Err(AmountProblem::Ambiguous {
                whole: 1500.0,
                decimal: 1.5
            }),
            "{locale}"
        );
    }
}

#[test]
fn what_is_not_an_amount_is_refused() {
    for text in [
        "",
        " ",
        "1e3",
        "fifty",
        "1,2.3",
        "1.000.0",
        "1,000,5",
        "12 34",
        "1 000 ,5",
        "$5",
        "1.000,5,0",
        "NaN",
        "inf",
        "+5",
        "1_000",
    ] {
        assert_eq!(
            parse_amount(text, "en-US"),
            Err(AmountProblem::Invalid),
            "{text:?}"
        );
    }
}

#[test]
fn an_amount_field_reads_back_as_the_same_number() {
    assert_eq!(amount_input(50.0, "en-US"), "50");
    assert_eq!(amount_input(12.5, "de"), "12,5");
    assert_eq!(amount_input(1.125, "en-US"), "1.1250");
    assert_eq!(amount_input(1000.5, "fr"), "1000,5");
    for value in [50.0, 12.5, 1.125, 1000.5, 0.001, 999_999_999.99] {
        for locale in ["en-US", "pt-BR", "de", "fr", "hi-IN"] {
            let text = amount_input(value, locale);
            assert_eq!(parse_amount(&text, locale), Ok(value), "{text} in {locale}");
        }
    }
}
