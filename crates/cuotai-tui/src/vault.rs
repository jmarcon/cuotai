//! PRD 0029 RF6.2: `--vault-audit [--json]` lists the vault entries Cuotai
//! created (from the core's index) with what uses each one; `--vault-prune`
//! deletes the ones the core proves unused, and `--vault-remove KEY` one
//! entry the user chose, each after a confirmation typed in a terminal
//! (2026-11 re-validation F1, F2, R-01). The rules live in
//! [`cuotai_core::oauth::vault_index`]; nothing read from an entry is ever
//! printed.

use crate::l10n::{tr, trf, trp};
use cuotai_core::config_file::ConfigFile;
use cuotai_core::oauth::vault_index::{self, AuditEntry, Kind, SkipReason, SourcePaths, Status};
use cuotai_core::oauth::OAuthSession;
use std::io::{self, BufRead, IsTerminal, Write};
use unicode_width::UnicodeWidthStr;

/// The word `--vault-prune` and `--vault-remove` ask for, in the CLI locale.
fn answer() -> &'static str {
    tr("vault.prune.answer", "delete")
}

/// Exit code 0; 2 when the audit proves some entries unused (so a script
/// can tell).
pub(crate) async fn audit(
    session: &OAuthSession,
    file: &ConfigFile,
    json: bool,
) -> Result<u8, Box<dyn std::error::Error>> {
    let paths = SourcePaths::for_file(file);
    let plan = match session.vault_index_path() {
        Some(index) => vault_index::audit(session, index, &paths).await,
        None => vault_index::Plan {
            config_path: paths.config.path.clone(),
            entries: Vec::new(),
        },
    };
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(
            &mut stdout,
            &serde_json::json!({
                "schema": 1,
                "checked_against": plan.config_path,
                "entries": plan.entries,
            }),
        )?;
        writeln!(stdout)?;
    } else {
        write!(stdout, "{}", audit_text(&plan.config_path, &plan.entries))?;
        write!(stdout, "{}", summary(&plan.entries))?;
    }
    Ok(if plan.entries.iter().any(|entry| entry.orphan) {
        2
    } else {
        0
    })
}

fn kind_label(kind: Option<Kind>) -> &'static str {
    match kind {
        Some(Kind::Oauth) => "OAuth",
        Some(Kind::Session) => tr("vault.kind.session", "web session"),
        Some(Kind::Channel) => tr("vault.kind.channel", "channel"),
        Some(Kind::ExtensionSecret) => tr("vault.kind.extension_secret", "extension secret"),
        None => tr("vault.kind.unknown", "unknown"),
    }
}

/// Why an entry is kept or removable, as one phrase.
fn status_label(status: Status) -> &'static str {
    match status {
        Status::InUse => tr("vault.status.in_use", "in use"),
        Status::Unused => tr(
            "vault.status.unused",
            "nothing in this configuration uses it (unused)",
        ),
        Status::OtherOwner => tr(
            "vault.status.other_owner",
            "belongs to another configuration or preferences file",
        ),
        Status::OwnerUnknown => tr(
            "vault.status.owner_unknown",
            "stored by an older version; Cuotai cannot tell who uses it",
        ),
        Status::Unverifiable => tr(
            "vault.status.unverifiable",
            "cannot be checked: its file is missing, damaged or has accounts Cuotai could not read",
        ),
        Status::Unclassified => tr(
            "vault.status.unclassified",
            "of an unknown kind; Cuotai cannot tell what uses it",
        ),
    }
}

/// What uses an entry, or why Cuotai cannot tell.
fn owner_label(entry: &AuditEntry) -> String {
    if entry.status != Status::InUse {
        return status_label(entry.status).to_owned();
    }
    match (entry.kind, &entry.account, entry.provider) {
        (Some(Kind::Channel), _, _) => tr(
            "vault.owner.channel",
            "a notification channel (--channel-list)",
        )
        .to_owned(),
        (Some(Kind::ExtensionSecret), _, _) => {
            tr("vault.owner.extension", "an extension's secret").to_owned()
        }
        (_, Some(account), Some(provider)) => {
            // PRD 0029 RF3.4 (V-07): an unnamed account by its provider once.
            let provider = crate::ui::provider_label(provider);
            let name = if entry.named { account.as_str() } else { "" };
            match cuotai_core::display_title(&provider, name) {
                (name, Some(provider)) => format!("{} / {provider}", crate::ui::safe(name)),
                (provider, None) => provider.to_owned(),
            }
        }
        _ => status_label(Status::InUse).to_owned(),
    }
}

/// The narrowest kind column; the owner starts two spaces after the widest
/// kind label of the list, measured by display width (V-08: CJK labels are
/// two cells wide per character).
const KIND_COLUMN: usize = 10;

fn kind_column(entries: &[AuditEntry]) -> usize {
    entries
        .iter()
        .map(|entry| kind_label(entry.kind).width())
        .max()
        .unwrap_or(0)
        .max(KIND_COLUMN)
}

fn audit_text(config: &std::path::Path, entries: &[AuditEntry]) -> String {
    let mut out = format!(
        "{}\n{}\n",
        tr(
            "vault.audit.title",
            "CUOTAI • credentials Cuotai keeps in the vault"
        ),
        trf(
            "vault.audit.checked",
            "Checked against {path}",
            &[("path", &crate::ui::safe(&config.display().to_string()))],
        )
    );
    if entries.is_empty() {
        out += &format!(
            "\n{}\n",
            tr(
                "vault.audit.empty",
                "No indexed credential. The index records what Cuotai stores from this version on."
            )
        );
        return out;
    }
    let column = kind_column(entries);
    let indent = " ".repeat(column + 2);
    for entry in entries {
        let kind = kind_label(entry.kind);
        let pad = " ".repeat(column.saturating_sub(kind.width()));
        let gone = if entry.present {
            ""
        } else {
            tr("vault.entry.gone", " · no longer in the vault")
        };
        let detail = trf(
            "vault.entry.detail",
            "key {key} · created {created}",
            &[
                ("key", &entry.key),
                ("created", &crate::ui::local_time(entry.created_at)),
            ],
        );
        out += &format!(
            "\n{kind}{pad}  {}{gone}\n{indent}{detail}\n",
            owner_label(entry)
        );
    }
    out
}

/// The counts under the list (V-08: real plurals).
fn summary(entries: &[AuditEntry]) -> String {
    let unused = entries
        .iter()
        .filter(|e| e.status == Status::Unused)
        .count();
    let kept = entries
        .iter()
        .filter(|e| !matches!(e.status, Status::Unused | Status::InUse))
        .count();
    let mut out = String::new();
    if unused > 0 {
        out += &format!(
            "\n{}\n",
            trp(
                "vault.audit.unused",
                "{count} credential is proven unused: cuotai --vault-prune deletes it.",
                "{count} credentials are proven unused: cuotai --vault-prune deletes them.",
                unused,
                &[],
            )
        );
    }
    if kept > 0 {
        out += &format!(
            "\n{}\n",
            trp(
                "vault.audit.kept",
                "{count} credential is kept because Cuotai cannot prove it unused; remove it with cuotai --vault-remove KEY.",
                "{count} credentials are kept because Cuotai cannot prove them unused; remove one with cuotai --vault-remove KEY.",
                kept,
                &[],
            )
        );
    }
    out
}

fn skip_label(reason: SkipReason) -> &'static str {
    match reason {
        SkipReason::Gone => tr("vault.skip.gone", "no longer listed"),
        SkipReason::Changed(Status::InUse) => tr("vault.skip.in_use", "now in use"),
        SkipReason::Changed(_) => tr("vault.skip.changed", "it can no longer be proven unused"),
    }
}

/// Ask for the typed confirmation word on stderr; `Ok(false)` when the
/// answer differs.
fn confirmed(question: &str) -> io::Result<bool> {
    let mut stderr = io::stderr().lock();
    write!(stderr, "\n{question}")?;
    stderr.flush()?;
    let mut typed = String::new();
    io::stdin().lock().read_line(&mut typed)?;
    Ok(typed.trim().to_lowercase() == answer())
}

fn interactive() -> bool {
    io::stdin().is_terminal() && io::stderr().is_terminal()
}

pub(crate) async fn prune(
    session: &OAuthSession,
    file: &ConfigFile,
) -> Result<u8, Box<dyn std::error::Error>> {
    let Some(index) = session.vault_index_path() else {
        println!(
            "{}",
            tr(
                "vault.prune.no_index",
                "No indexed credential; nothing to delete."
            )
        );
        return Ok(0);
    };
    let paths = SourcePaths::for_file(file);
    let plan = vault_index::audit(session, index, &paths).await;
    let unused: Vec<AuditEntry> = plan
        .entries
        .iter()
        .filter(|entry| entry.status == Status::Unused)
        .cloned()
        .collect();
    if unused.is_empty() {
        println!(
            "{}",
            tr(
                "vault.prune.none",
                "No credential is proven unused; nothing to delete."
            )
        );
        print!("{}", summary(&plan.entries));
        return Ok(0);
    }
    let mut stderr = io::stderr().lock();
    write!(stderr, "{}", audit_text(&plan.config_path, &unused))?;
    write!(stderr, "{}", summary(&plan.entries))?;
    drop(stderr);
    if !interactive() {
        return Err(tr(
            "vault.prune.needs_terminal",
            "--vault-prune asks for a confirmation in a terminal; run it interactively.",
        )
        .into());
    }
    let question = trp(
        "vault.prune.confirm",
        "Delete the {count} credential above from the vault? Type \"{answer}\" to confirm: ",
        "Delete the {count} credentials above from the vault? Type \"{answer}\" to confirm: ",
        unused.len(),
        &[("answer", &answer())],
    );
    if !confirmed(&question)? {
        return Err(tr("vault.prune.cancelled", "Nothing was deleted.").into());
    }
    // Exactly the keys shown, judged again from disk by the core.
    let keys: Vec<String> = unused.into_iter().map(|entry| entry.key).collect();
    let outcome = vault_index::prune_confirmed(session, index, &paths, &keys)
        .await
        .map_err(|note| note.resolve(crate::ui::tui_locale()))?;
    println!(
        "{}",
        trp(
            "vault.prune.done",
            "{count} credential deleted from the vault.",
            "{count} credentials deleted from the vault.",
            outcome.deleted.len(),
            &[],
        )
    );
    for skipped in &outcome.skipped {
        println!(
            "{}",
            trf(
                "vault.prune.skipped",
                "Kept {key}: {reason}.",
                &[
                    ("key", &skipped.key),
                    ("reason", &skip_label(skipped.reason))
                ],
            )
        );
    }
    Ok(0)
}

pub(crate) async fn remove(
    session: &OAuthSession,
    file: &ConfigFile,
    key: &str,
) -> Result<u8, Box<dyn std::error::Error>> {
    let not_indexed = || {
        cuotai_core::text::Localized::fixed(
            "vault.remove.not_indexed",
            "That key is not in the list of stored credentials.",
        )
        .resolve(crate::ui::tui_locale())
    };
    let Some(index) = session.vault_index_path() else {
        return Err(not_indexed().into());
    };
    let paths = SourcePaths::for_file(file);
    let plan = vault_index::audit(session, index, &paths).await;
    let Some(entry) = plan.entries.iter().find(|entry| entry.key == key) else {
        return Err(not_indexed().into());
    };
    let mut stderr = io::stderr().lock();
    write!(
        stderr,
        "{}",
        audit_text(&plan.config_path, std::slice::from_ref(entry))
    )?;
    drop(stderr);
    if !interactive() {
        return Err(tr(
            "vault.remove.needs_terminal",
            "--vault-remove asks for a confirmation in a terminal; run it interactively.",
        )
        .into());
    }
    let question = trf(
        "vault.remove.confirm",
        "Delete this credential from the vault? Type \"{answer}\" to confirm: ",
        &[("answer", &answer())],
    );
    if !confirmed(&question)? {
        return Err(tr("vault.prune.cancelled", "Nothing was deleted.").into());
    }
    vault_index::remove_one(session, index, &paths, key)
        .await
        .map_err(|note| note.resolve(crate::ui::tui_locale()))?;
    println!(
        "{}",
        trf(
            "vault.remove.done",
            "Deleted {key} from the vault.",
            &[("key", &key)],
        )
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn entry(key: &str, status: Status, kind: Option<Kind>) -> AuditEntry {
        let at = Utc.with_ymd_and_hms(2026, 10, 25, 12, 0, 0).unwrap();
        AuditEntry {
            key: key.into(),
            created_at: at,
            kind,
            provider: None,
            account_id: None,
            account: None,
            named: false,
            status,
            orphan: status == Status::Unused,
            present: true,
        }
    }

    #[test]
    fn the_text_names_owners_statuses_and_plurals_without_values() {
        let mut used = entry("key-a", Status::InUse, Some(Kind::Oauth));
        used.provider = Some(cuotai_core::Provider::ChatGpt);
        used.account = Some("Work".into());
        used.named = true;
        // PRD 0029 V-07: an unnamed account names its provider once.
        let mut unnamed = entry("key-u", Status::InUse, Some(Kind::Oauth));
        unnamed.provider = Some(cuotai_core::Provider::Claude);
        unnamed.account = Some("Claude".into());
        let owner = crate::l10n::with_locale("en-US", || owner_label(&unnamed));
        assert_eq!(owner, "Claude");
        let entries = [
            used,
            entry("key-b", Status::Unused, Some(Kind::Session)),
            entry("key-c", Status::OtherOwner, Some(Kind::Oauth)),
            entry("key-d", Status::Unverifiable, Some(Kind::Channel)),
            entry("key-e", Status::Unclassified, None),
        ];
        let path = std::path::Path::new("/tmp/cuotai.toml");
        let text =
            crate::l10n::with_locale("en-US", || audit_text(path, &entries) + &summary(&entries));
        assert!(text.contains("Checked against /tmp/cuotai.toml"), "{text}");
        assert!(text.contains("Work / ChatGPT"), "{text}");
        assert!(text.contains("another configuration"), "{text}");
        assert!(text.contains("1 credential is proven unused"), "{text}");
        assert!(text.contains("3 credentials are kept"), "{text}");
        let text = crate::l10n::with_locale("pt-BR", || summary(&entries[..2]));
        assert!(text.contains("1 credencial"), "{text}");
    }

    /// V-08: the owner column lines up by display width in Japanese.
    #[test]
    fn the_owner_column_is_padded_by_display_width() {
        let entries = [
            entry("key-a", Status::Unclassified, Some(Kind::Session)),
            entry("key-b", Status::Unused, Some(Kind::Oauth)),
        ];
        crate::l10n::with_locale("ja", || {
            let text = audit_text(std::path::Path::new("/c.toml"), &entries);
            let kind = kind_label(Some(Kind::Session));
            let column = kind.width().max(KIND_COLUMN) + 2;
            for (label, owner) in [
                (kind, status_label(Status::Unclassified)),
                ("OAuth", status_label(Status::Unused)),
            ] {
                let line = text.lines().find(|l| l.starts_with(label)).unwrap();
                let at = line.find(owner).unwrap();
                assert_eq!(line[..at].width(), column, "{text}");
            }
        });
    }
}
