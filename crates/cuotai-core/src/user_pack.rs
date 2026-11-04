//! PRD 0050 RF3.3: what a user language pack may not translate.
//!
//! A pack (`<user folder>/locales/<code>.json`) may add or replace ordinary
//! interface and core texts, but never the words of credential warnings,
//! consent, confirmations or the local server: those always use the
//! reviewed built-in or core text. A front end removes the protected ids
//! with [`strip_protected`] before the pack reaches its interface and
//! reports how many it ignored.

/// Id prefixes a pack may not replace.
pub const PROTECTED_PREFIXES: [&str; 12] = [
    "credential_help.",
    "oauth_error.",
    "edit_error.",
    "remove_error.",
    "user_session.",
    "access.risk.",
    "access.credential_hint.",
    "hooks.",
    "extensions.consent.",
    "serve.",
    "update.",
    "execution.",
];

/// Whether a pack may not replace `id`: a [`PROTECTED_PREFIXES`] id, the
/// delete confirmation (`dialog.delete…`), or any id with a `.confirm.` or
/// `.consent.` part.
pub fn is_protected(id: &str) -> bool {
    PROTECTED_PREFIXES
        .iter()
        .any(|prefix| id.starts_with(prefix))
        || id == "dialog.delete"
        || id.starts_with("dialog.delete.")
        || id.contains(".confirm.")
        || id.contains(".consent.")
}

/// The pack `text` without its protected ids, and those ids (sorted).
/// `None` when `text` is not a JSON object (the front end reports it as
/// invalid as before).
pub fn strip_protected(text: &str) -> Option<(String, Vec<String>)> {
    let mut object: serde_json::Map<String, serde_json::Value> = serde_json::from_str(text).ok()?;
    let mut removed: Vec<String> = object
        .keys()
        .filter(|id| *id != "_meta" && is_protected(id))
        .cloned()
        .collect();
    if removed.is_empty() {
        return Some((text.to_owned(), removed));
    }
    for id in &removed {
        object.remove(id);
    }
    removed.sort();
    Some((serde_json::to_string(&object).ok()?, removed))
}

#[cfg(test)]
mod tests {
    use super::*;

    // security-invariant: SEC-USER-PACK
    #[test]
    fn every_protected_pattern_is_refused_and_ordinary_ids_pass() {
        for id in [
            "credential_help.openai_api",
            "oauth_error.store.sqlite_dev_only",
            "edit_error.option_not_valid_for_auth_mode",
            "remove_error.oauth",
            "user_session.slot.secret",
            "access.risk.undocumented",
            "access.credential_hint.extension",
            "hooks.confirm.command",
            "extensions.consent.risk",
            "serve.allow_browsers.warning",
            "update.verify.failed",
            "execution.review.approve_hooks.title",
            "dialog.delete",
            "dialog.delete.title",
            "settings.hooks.confirm.title",
            "dialog.detect.consent.body",
        ] {
            assert!(is_protected(id), "{id}");
        }
        for id in [
            "tray.ok",
            "settings.page.general",
            "card.connectSession",
            "dialog.deleted_banner",
            "access.summary.claude",
            "extensions.title",
            "settings.serve.origins.label",
            "confirm",
        ] {
            assert!(!is_protected(id), "{id}");
        }
    }

    #[test]
    fn a_pack_loses_only_its_protected_ids() {
        let pack = r#"{"_meta":{"name":"Français"},"tray.ok":"Tout va bien","serve.allow_browsers.warning":"Sans danger !","hooks.confirm.command":"rien","x.consent.y":{"one":"a","other":"b"}}"#;
        let (kept, removed) = strip_protected(pack).unwrap();
        assert_eq!(
            removed,
            [
                "hooks.confirm.command",
                "serve.allow_browsers.warning",
                "x.consent.y"
            ]
        );
        let kept: serde_json::Value = serde_json::from_str(&kept).unwrap();
        assert_eq!(kept["tray.ok"], "Tout va bien");
        assert_eq!(kept["_meta"]["name"], "Français");
        assert!(kept.get("serve.allow_browsers.warning").is_none());
        // A pack with nothing protected is handed on byte for byte.
        let clean = r#"{"_meta":{"name":"X"},  "tray.ok":"ok"}"#;
        assert_eq!(strip_protected(clean).unwrap(), (clean.to_owned(), vec![]));
        assert!(strip_protected("[1]").is_none());
        assert!(strip_protected("not json").is_none());
    }
}
