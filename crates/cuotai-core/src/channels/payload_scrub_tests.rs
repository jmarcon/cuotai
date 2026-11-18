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
