//! PRD 0034 RF7: the backend's own error texts (the ones that are not core
//! `Localized` values). Each one travels as the JSON of a `Localized`: a
//! `desktop.error.*` id that the interface resolves in the UI catalogs of the
//! chosen language, and an English fallback for a catalog that lacks the id.

use cuotai_core::text::Localized;

/// The JSON envelope the interface parses (`errText`): `{ id, params, fallback }`.
fn error(id: &'static str, fallback: &str) -> String {
    let note = Localized::fixed(id, fallback);
    serde_json::to_string(&note).unwrap_or_else(|_| note.fallback.to_string())
}

macro_rules! texts {
    ($($name:ident => ($id:literal, $fallback:literal);)*) => {
        $(
            pub fn $name() -> String {
                error(concat!("desktop.error.", $id), $fallback)
            }
        )*

        /// Every (id, English fallback) pair, for the catalog parity test.
        #[cfg(test)]
        pub const ALL: &[(&str, &str)] = &[
            $((concat!("desktop.error.", $id), $fallback),)*
        ];
    };
}

texts! {
    config_not_loaded => ("config_not_loaded", "Configuration not loaded.");
    oauth_session_missing => ("oauth_session_missing", "OAuth session not initialized.");
    no_account => ("no_account", "Account does not exist.");
    vault_unavailable => ("vault_unavailable", "Credential vault unavailable.");
    pending_missing => ("pending_missing", "Pending authorization not found for this account; sign in again.");
    login_unfinished => ("login_unfinished", "Sign-in in progress; finish or cancel it before saving.");
    no_pending_credential => ("no_pending_credential", "No pending credential for this identifier.");
    original_without_oauth => ("original_without_oauth", "The original account had no OAuth credential to delete.");
    bad_connection_param => ("bad_connection_param", "Invalid connection parameter.");
    paste_session_first => ("paste_session_first", "Paste a session before testing.");
    not_an_aggregator => ("not_an_aggregator", "This account is not an aggregator.");
    login_already_running => ("login_already_running", "A sign-in is already in progress; finish or cancel it.");
    no_manual_code => ("no_manual_code", "This flow does not accept a manual code.");
    code_pending => ("code_pending", "Wait for the previous code to be validated before sending again.");
    login_ended => ("login_ended", "The sign-in session ended.");
    no_login => ("no_login", "No sign-in in progress.");
    login_finished => ("login_finished", "The sign-in has already finished; start again if needed.");
    login_silent => ("login_silent", "The sign-in session did not respond.");
    auth_mode_unavailable => ("auth_mode_unavailable", "Authentication mode not offered for this provider.");
    finish_login_first => ("finish_login_first", "Finish the sign-in before saving (or go back to the manual credential).");
    invalid_monogram => ("invalid_monogram", "A monogram has one or two characters, without spaces.");
    config_folder_unavailable => ("config_folder_unavailable", "Configuration folder unavailable.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn catalog(text: &str) -> BTreeMap<String, serde_json::Value> {
        serde_json::from_str(text).expect("a UI catalog is valid JSON")
    }

    #[test]
    fn every_error_has_an_entry_in_each_ui_catalog_and_the_english_text_matches() {
        let en = catalog(include_str!("../../ui/locales/en-US.json"));
        let others = [
            (
                "pt-BR",
                catalog(include_str!("../../ui/locales/pt-BR.json")),
            ),
            ("es", catalog(include_str!("../../ui/locales/es.json"))),
            ("it", catalog(include_str!("../../ui/locales/it.json"))),
            (
                "zh-CN",
                catalog(include_str!("../../ui/locales/zh-CN.json")),
            ),
            ("fr", catalog(include_str!("../../ui/locales/fr.json"))),
            ("ja", catalog(include_str!("../../ui/locales/ja.json"))),
        ];
        for (id, fallback) in ALL {
            assert_eq!(
                en.get(*id).and_then(|v| v.as_str()),
                Some(*fallback),
                "en-US {id}"
            );
            for (locale, catalog) in &others {
                assert!(
                    catalog.get(*id).is_some_and(|v| v.is_string()),
                    "{locale} lacks {id}"
                );
            }
        }
    }

    #[test]
    fn the_envelope_is_the_json_of_a_localized_text() {
        let value: serde_json::Value = serde_json::from_str(&config_not_loaded()).unwrap();
        assert_eq!(value["id"], "desktop.error.config_not_loaded");
        assert_eq!(value["fallback"], "Configuration not loaded.");
    }
}
