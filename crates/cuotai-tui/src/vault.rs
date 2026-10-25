//! PRD 0029 RF6.2: `--vault-audit [--json]` lists the vault entries Cuotai
//! created (from the core's index) and marks the orphans; `--vault-prune`
//! deletes the orphans after a confirmation typed in a terminal. Nothing
//! read from an entry is ever printed.

use crate::l10n::{tr, trf};
use cuotai_core::oauth::vault_index::{self, AuditEntry, Kind};
use cuotai_core::oauth::OAuthSession;
use cuotai_core::Config;
use std::io::{self, BufRead, IsTerminal, Write};

/// The word `--vault-prune` asks for, in the CLI locale.
fn answer() -> &'static str {
    tr("vault.prune.answer", "delete")
}

/// Exit code 0; 2 when the audit finds orphans (so a script can tell).
pub(crate) async fn audit(
    session: &OAuthSession,
    config: &Config,
    json: bool,
) -> Result<u8, Box<dyn std::error::Error>> {
    let entries = match session.vault_index_path() {
        Some(path) => vault_index::audit(session, path, config).await,
        None => Vec::new(),
    };
    let mut stdout = io::stdout().lock();
    if json {
        serde_json::to_writer_pretty(
            &mut stdout,
            &serde_json::json!({ "schema": 1, "entries": entries }),
        )?;
        writeln!(stdout)?;
    } else {
        write!(stdout, "{}", audit_text(&entries))?;
    }
    Ok(if entries.iter().any(|entry| entry.orphan) {
        2
    } else {
        0
    })
}

fn audit_text(entries: &[AuditEntry]) -> String {
    let mut out = format!(
        "{}\n",
        tr(
            "vault.audit.title",
            "CUOTAI • credentials Cuotai keeps in the vault"
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
    for entry in entries {
        let kind = match entry.kind {
            Some(Kind::Oauth) => "OAuth",
            Some(Kind::Session) => tr("vault.kind.session", "web session"),
            None => tr("vault.kind.unused", "unused"),
        };
        let owner = match (&entry.account, entry.provider) {
            (Some(account), Some(provider)) => {
                format!(
                    "{} / {}",
                    crate::ui::safe(account),
                    crate::ui::provider_label(provider)
                )
            }
            _ => tr("vault.owner.none", "no configured account uses it (orphan)").into(),
        };
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
        out += &format!("\n{kind:<10}  {owner}{gone}\n            {detail}\n");
    }
    let orphans = entries.iter().filter(|entry| entry.orphan).count();
    if orphans > 0 {
        let line = trf(
            "vault.audit.orphans",
            "{count} orphan(s): cuotai --vault-prune deletes them.",
            &[("count", &orphans)],
        );
        out += &format!("\n{line}\n");
    }
    out
}

pub(crate) async fn prune(
    session: &OAuthSession,
    config: &Config,
) -> Result<u8, Box<dyn std::error::Error>> {
    let Some(path) = session.vault_index_path() else {
        println!(
            "{}",
            tr(
                "vault.prune.no_index",
                "No indexed credential; nothing to delete."
            )
        );
        return Ok(0);
    };
    let orphans: Vec<AuditEntry> = vault_index::audit(session, path, config)
        .await
        .into_iter()
        .filter(|entry| entry.orphan)
        .collect();
    if orphans.is_empty() {
        println!(
            "{}",
            tr(
                "vault.prune.no_orphans",
                "No orphan credential; nothing to delete."
            )
        );
        return Ok(0);
    }
    let mut stderr = io::stderr().lock();
    write!(stderr, "{}", audit_text(&orphans))?;
    if !(io::stdin().is_terminal() && io::stderr().is_terminal()) {
        return Err(tr(
            "vault.prune.needs_terminal",
            "--vault-prune asks for a confirmation in a terminal; run it interactively.",
        )
        .into());
    }
    let question = trf(
        "vault.prune.confirm",
        "Delete the {count} credentials above from the vault? Type \"{answer}\" to confirm: ",
        &[("count", &orphans.len()), ("answer", &answer())],
    );
    write!(stderr, "\n{question}")?;
    stderr.flush()?;
    let mut typed = String::new();
    io::stdin().lock().read_line(&mut typed)?;
    if typed.trim().to_lowercase() != answer() {
        return Err(tr("vault.prune.cancelled", "Nothing was deleted.").into());
    }
    let deleted = vault_index::prune(session, path, config)
        .await
        .map_err(|note| note.resolve(crate::ui::tui_locale()))?;
    println!(
        "{}",
        trf(
            "vault.prune.done",
            "{count} orphan credential(s) deleted from the vault.",
            &[("count", &deleted)],
        )
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    #[test]
    fn the_text_names_owners_and_orphans_without_values() {
        let at = Utc.with_ymd_and_hms(2026, 10, 25, 12, 0, 0).unwrap();
        let entries = [
            AuditEntry {
                key: "key-a".into(),
                created_at: at,
                kind: Some(Kind::Oauth),
                provider: Some(cuotai_core::Provider::ChatGpt),
                account_id: Some("acc_0123456789ab".into()),
                account: Some("Work".into()),
                orphan: false,
                present: true,
            },
            AuditEntry {
                key: "key-b".into(),
                created_at: at,
                kind: None,
                provider: None,
                account_id: None,
                account: None,
                orphan: true,
                present: true,
            },
        ];
        let text = audit_text(&entries);
        assert!(text.contains("Work / ChatGPT"), "{text}");
        assert!(text.contains("órfã"), "{text}");
        assert!(text.contains("1 órfã(s)"), "{text}");
    }
}
