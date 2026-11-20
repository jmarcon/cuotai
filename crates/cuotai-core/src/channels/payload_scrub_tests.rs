//! PRD 0043 re-validation S-02: [`super::scrub`] drops hidden
//! text-direction characters before anything leaves.

use super::scrub;

#[test]
fn scrub_strips_hidden_direction_marks_and_keeps_lines() {
    assert_eq!(
        scrub("Work \u{202e}txet\n\u{2066}Main\u{2069} at 80%"),
        "Work txet\nMain at 80%"
    );
    assert_eq!(scrub("a\u{200f}b\tc\nme@example.com"), "abc\n[email]");
}

/// PRD 0044 RF2.2 (owner decision 2026-11-20): the value after a secret's
/// name is hidden whatever its length; ordinary text is untouched.
#[test]
fn scrub_hides_the_value_after_a_secret_name() {
    let cases = [
        ("password: hunter2", "password: [redacted]"),
        ("Password=x", "Password=[redacted]"),
        ("PASSWORD = a", "PASSWORD = [redacted]"),
        ("senha: 1", "senha: [redacted]"),
        ("Senha=abc", "Senha=[redacted]"),
        ("secret: s", "secret: [redacted]"),
        ("client_secret=ab", "client_secret=[redacted]"),
        ("token: t", "token: [redacted]"),
        ("access_token=z", "access_token=[redacted]"),
        ("authToken: q", "authToken: [redacted]"),
        ("key: k", "key: [redacted]"),
        ("api_key=k", "api_key=[redacted]"),
        ("apiKey: k", "apiKey: [redacted]"),
        ("x-api-key: k", "x-api-key: [redacted]"),
        ("DB_PASSWORD=p", "DB_PASSWORD=[redacted]"),
        ("client.secret = v", "client.secret = [redacted]"),
        // Quoted values, closed or not, with the quotes.
        (r#"password: "two words""#, "password: [redacted]"),
        ("senha='a b'", "senha=[redacted]"),
        (r#"{"token":"abc","n":1}"#, r#"{"token":[redacted],"n":1}"#),
        ("password: \"open", "password: [redacted]"),
        // The value ends at whitespace, a quote, a comma or an ampersand.
        ("key=a&token=b c", "key=[redacted]&token=[redacted] c"),
        ("password: p, then", "password: [redacted], then"),
        (
            "line\npassword: p\nnext",
            "line\npassword: [redacted]\nnext",
        ),
    ];
    for (raw, want) in cases {
        assert_eq!(scrub(raw), want, "{raw}");
        assert_eq!(scrub(want), want, "idempotent: {want}");
    }
    for plain in [
        "Tokens: 40",
        "token budget at 80%",
        "monkey: 3",
        "hockey = 2",
        "APIKEY: 1",
        "Work: 75% of the weekly limit",
        "Key budget at 90% · resets 14:00",
        "password",
        "password:",
        "Monthly budget · Spending in the month (2)",
    ] {
        assert_eq!(scrub(plain), plain, "{plain}");
    }
}
