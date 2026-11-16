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

/// The owner's GitHub handle may appear only in the repository's own address
/// (`github.com/<owner>/cuotai`); demo fixtures and smokes use placeholders
/// such as `octo-dev`. Another repository of the owner, or a name that only
/// starts with `cuotai`, still counts.
fn handle_outside_repo_urls(text: &str, handle: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let handle = handle.to_ascii_lowercase();
    lower.match_indices(&handle).any(|(i, _)| {
        // `i` and `i + handle.len()` are char boundaries: the match is the
        // ASCII handle itself, so slicing at them cannot split a character.
        let before = &lower[..i];
        let after = &lower[i + handle.len()..];
        let repo = after.strip_prefix("/cuotai").is_some_and(|rest| {
            !rest
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '-' || c == '_')
        });
        !(before.ends_with("github.com/") && repo)
    })
}

#[test]
fn handle_scan_ignores_only_the_repository_address() {
    let handle = ["jm", "arcon"].concat();
    let outside = |text: String| handle_outside_repo_urls(&text, &handle);
    assert!(!outside(format!("https://github.com/{handle}/cuotai")));
    assert!(!outside(format!(
        "https://github.com/{handle}/cuotai/pull/1"
    )));
    assert!(!outside(format!("https://github.com/{handle}/cuotai.git")));
    assert!(outside(format!("dr(\"{handle}\", \"copilot\")")));
    assert!(outside(format!("https://github.com/{handle}/dotfiles")));
    assert!(outside(format!("https://github.com/{handle}/cuotai-fork")));
    // At the end of a line, and before a multibyte character: reported, no panic.
    assert!(outside(format!("owner {handle}")));
    assert!(outside(format!("{handle}é")));
    assert!(outside(format!("github.com/{handle}")));
}

#[test]
fn demo_fixtures_never_name_the_owner() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let handle = ["jm", "arcon"].concat();
    let listing = match Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "ls-files",
            "-z",
            "apps/desktop/ui",
            "apps/desktop/tools",
            "scripts",
        ])
        .output()
    {
        Ok(out) if out.status.success() => out.stdout,
        _ => return,
    };
    let mut offenders = Vec::new();
    for rel in listing.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let rel = String::from_utf8_lossy(rel).into_owned();
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            if handle_outside_repo_urls(line, &handle) {
                offenders.push(format!("{rel}:{}", n + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "the owner's handle is in demo data; use a placeholder (octo-dev):\n{}",
        offenders.join("\n")
    );
}

/// PRD 0031 S-01 and S-02: a documented or scripted call to the local server
/// must not put the bearer token on a command line (any local user reads it
/// with `ps`) nor let a proxy receive it. The safe form hands the header to
/// `curl` on stdin (`printf 'header = …' "$token" | curl -K - …`) with
/// `--noproxy '*'` and `--proto '=http'`. This scan lives here, in the
/// always-on privacy step, because CI skips the test jobs for docs-only
/// changes. Plans (`docs/plans/`) quote the old form on purpose.
fn scans_for_token_exposure(rel: &str) -> bool {
    if rel.starts_with("docs/plans/") {
        return false;
    }
    rel.ends_with(".sh") || rel.ends_with(".md") || rel.starts_with("site/")
}

/// The command-like parts of `text`, each with its first line number: every
/// line of a script or page, or in Markdown the fenced code blocks and the
/// inline code spans. Backslash-continued lines are joined into one command.
fn command_segments(rel: &str, text: &str) -> Vec<(usize, String)> {
    let markdown = rel.ends_with(".md");
    let mut segments = Vec::new();
    let mut fenced = false;
    let mut pending: Option<(usize, String)> = None;
    for (n, line) in text.lines().enumerate() {
        let n = n + 1;
        if markdown && line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let parts: Vec<String> = if !markdown || fenced {
            vec![line.to_owned()]
        } else {
            // Inline code spans: the odd pieces between backticks.
            line.split('`')
                .skip(1)
                .step_by(2)
                .map(str::to_owned)
                .collect()
        };
        for part in parts {
            let (start, mut joined) = pending.take().unwrap_or((n, String::new()));
            joined.push(' ');
            match part.trim_end().strip_suffix('\\') {
                Some(head) if !markdown || fenced => {
                    joined.push_str(head);
                    pending = Some((start, joined));
                }
                _ => {
                    joined.push_str(&part);
                    segments.push((start, joined));
                }
            }
        }
    }
    segments.extend(pending);
    segments
}

/// Why `segment` exposes the token, if it does.
fn token_exposure(segment: &str) -> Option<&'static str> {
    let bearer_value = segment.match_indices("Bearer").any(|(i, word)| {
        let rest = segment[i + word.len()..].trim_start();
        let rest = rest.strip_prefix('"').unwrap_or(rest);
        rest.starts_with('$') || rest.starts_with("cuotai_")
    });
    if bearer_value {
        return Some("the token is on a command line; pass the header to curl on stdin (-K -)");
    }
    let calls_curl = segment.match_indices("curl").any(|(i, word)| {
        let before = segment[..i].chars().next_back();
        let after = segment[i + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '/')
            && after.is_some_and(char::is_whitespace)
    });
    let loopback = [
        "127.0.0.1",
        "localhost",
        "[::1]",
        "$CUOTAI_URL",
        "${CUOTAI_URL",
        "$cuotai_url",
        "${cuotai_url",
    ]
    .iter()
    .any(|target| segment.contains(target));
    if calls_curl && loopback && !(segment.contains("--noproxy") && segment.contains("--proto")) {
        return Some("curl to the local server without --noproxy '*' and --proto '=http'");
    }
    None
}

#[test]
fn token_exposure_scanner_flags_the_unsafe_forms() {
    let unsafe_forms = [
        r#"curl -s -H "Authorization: Bearer $(cat "$F")" http://127.0.0.1:6738/v1/limits"#,
        r#"curl -fsS -H "Authorization: Bearer $cuotai_token" "$cuotai_url/v1/limits""#,
        r#"wget --header "Authorization: Bearer ${TOKEN}" http://example.com/"#,
        "curl -H 'Authorization: Bearer cuotai_0123456789abcdef' http://[::1]:6738/health",
        "curl -s http://127.0.0.1:6738/v1/limits",
        "curl --noproxy '*' http://localhost:6738/v1/limits",
        "x=$(curl -fsS \"$CUOTAI_URL/v1/limits\")",
    ];
    for form in unsafe_forms {
        assert!(token_exposure(form).is_some(), "not flagged: {form}");
    }
    let safe_forms = [
        r#"printf 'header = "Authorization: Bearer %s"\n' "$(pbpaste)" | curl -q -K - --noproxy '*' --proto '=http' -fsS http://127.0.0.1:6738/v1/limits"#,
        "every request needs `Authorization: Bearer <token>`",
        r#"answers 401 with WWW-Authenticate: Bearer realm="cuotai""#,
        "curl -fsS https://crates.io/api/v1/crates/cuotai-tui",
        "the recipes need curl and jq on 127.0.0.1",
    ];
    for form in safe_forms[..4].iter() {
        assert!(token_exposure(form).is_none(), "flagged: {form}");
    }
    // Prose is not a command; only the code parts of a page are scanned.
    let page = format!(
        "{}\n\n```sh\ncuotai --serve\n{} \\\n  | curl -q -K - --noproxy '*' --proto '=http' http://127.0.0.1:6738/v1/limits\n```\n",
        safe_forms[4], r#"printf 'header = "Authorization: Bearer %s"\n' "$(cat "$F")""#
    );
    let segments = command_segments("docs/x.md", &page);
    assert!(
        segments.iter().all(|(_, s)| token_exposure(s).is_none()),
        "{segments:?}"
    );
    let old = "Send it (`curl -H \"Authorization: Bearer $(cat <token file>)\" …`), or\n";
    let segments = command_segments("docs/x.md", old);
    assert_eq!(segments.len(), 1);
    assert!(token_exposure(&segments[0].1).is_some());
    // A script's continued curl line is one command.
    let script = "  curl -fsS --max-time 3 \\\n    \"$cuotai_url/v1/limits\"\n";
    let segments = command_segments("examples/x.sh", script);
    assert_eq!(segments[0].0, 1);
    assert!(token_exposure(&segments[0].1).is_some());
    assert!(!scans_for_token_exposure("docs/plans/0031_status_bars.md"));
    assert!(scans_for_token_exposure("docs/local-server.md"));
    assert!(scans_for_token_exposure("site/index.html"));
    assert!(!scans_for_token_exposure("crates/cuotai-core/src/serve.rs"));
}

#[test]
fn no_documented_command_exposes_the_server_token() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let listing = match Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-z"])
        .output()
    {
        Ok(out) if out.status.success() => out.stdout,
        _ => return,
    };
    let mut offenders = Vec::new();
    for rel in listing.split(|b| *b == 0).filter(|p| !p.is_empty()) {
        let rel = String::from_utf8_lossy(rel).into_owned();
        if !scans_for_token_exposure(&rel) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&rel)) else {
            continue;
        };
        for (line, segment) in command_segments(&rel, &text) {
            if let Some(why) = token_exposure(&segment) {
                offenders.push(format!("{rel}:{line}: {why}"));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a command exposes the local server's token (see docs/local-server.md, \
         \"Sending the token safely\"):\n{}",
        offenders.join("\n")
    );
}
