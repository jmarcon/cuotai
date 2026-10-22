//! Privacy guard: no personal e-mail address may be committed. Demo data,
//! fixtures, tests and docs use RFC 2606 placeholders (`alex@example.com`).
//! The scan covers every tracked text file and flags `local@domain` where the
//! domain is a consumer mailbox provider. Intentionally public maintainer
//! contacts, if one is ever needed, go in `ALLOWED` (empty today).

use std::path::Path;
use std::process::Command;

/// Consumer mailbox providers: an address on one of these is personal.
const PERSONAL_DOMAINS: &[&str] = &[
    "gmail.com",
    "googlemail.com",
    "hotmail.com",
    "outlook.com",
    "live.com",
    "yahoo.com",
    "icloud.com",
    "me.com",
    "proton.me",
    "protonmail.com",
];

/// Exact addresses (lower case) that are public on purpose.
const ALLOWED: &[&str] = &[];

fn is_local_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

/// Personal-domain addresses found in `text`, masked as `j***@gmail.com`
/// so a failure never reprints the address.
fn personal_addresses(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut found = Vec::new();
    for (at, _) in lower.match_indices('@') {
        let local_start = lower[..at]
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_local_char(*c))
            .last()
            .map_or(at, |(i, _)| i);
        let local = &lower[local_start..at];
        let domain: String = lower[at + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-'))
            .collect();
        let domain = domain.trim_end_matches('.');
        if local.is_empty() || !PERSONAL_DOMAINS.contains(&domain) {
            continue;
        }
        if ALLOWED.contains(&format!("{local}@{domain}").as_str()) {
            continue;
        }
        let first = local.chars().next().unwrap_or('?');
        found.push(format!("{first}***@{domain}"));
    }
    found
}

#[test]
fn scanner_flags_personal_domains_and_ignores_placeholders() {
    let personal = format!("{}@{}", "someone.real", "gmail.com");
    assert_eq!(
        personal_addresses(&format!("dr(\"{personal}\", 1)")),
        ["s***@gmail.com"]
    );
    assert_eq!(
        personal_addresses(&format!("A@{}.", "Outlook.com")),
        ["a***@outlook.com"]
    );
    assert!(personal_addresses("alex@example.com, a@corp.example.com").is_empty());
    assert!(personal_addresses("@gmail.com and user@gmail.com.example.org").is_empty());
    assert!(personal_addresses("ci@github.com").is_empty());
}

#[test]
fn no_personal_email_address_is_committed() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let listing = match Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output()
    {
        Ok(out) if out.status.success() => out.stdout,
        // A source archive without .git or a machine without git: nothing to scan.
        _ => return,
    };
    let mut offenders = Vec::new();
    for rel in listing.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let rel = String::from_utf8_lossy(rel).into_owned();
        // Binary or unreadable (images, fonts, deleted-but-listed) files are skipped.
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            for masked in personal_addresses(line) {
                offenders.push(format!("{rel}:{}: {masked}", n + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "personal e-mail addresses are committed; use an RFC 2606 placeholder \
         (alex@example.com) or add a deliberate public contact to ALLOWED:\n{}",
        offenders.join("\n")
    );
}
