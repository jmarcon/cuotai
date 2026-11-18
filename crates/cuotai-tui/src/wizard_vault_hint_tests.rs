//! Re-validation PRD 0047 S-03 (G-0047-4): a parameter whose slot the
//! edited account keeps in the vault says so on the parameters step.

use super::*;

fn screen(wiz: &mut Wizard) -> String {
    use ratatui::{backend::TestBackend, Terminal};
    let (width, height) = (120, 50);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| wiz.draw(frame, Path::new("cuotai.toml")))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol().to_string())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn mistral(session: bool) -> Config {
    Config {
        accounts: vec![Account {
            name: "Mistral".into(),
            provider: Provider::Mistral,
            credential_env: Some("MISTRAL_API_KEY".into()),
            session: session.then(|| user_session::SessionConfig {
                credential_ref: "6f1d3c1e-8a52-4a3c-9f7e-2a8b5b1c0d11".into(),
                slots: ["admin_key_env".to_string()].into(),
            }),
            ..Default::default()
        }],
        ..Config::default()
    }
}

#[test]
fn a_vault_held_parameter_shows_the_vault_hint() {
    // The hint is the core's vault state, in the active language.
    let vault = crate::ui::resolve_l10n(&cuotai_core::text::Localized::fixed(
        "secret_input.state.vault",
        "Saved in the system vault",
    ));
    let mut wiz = Wizard::new(&mistral(true), Some(0));
    wiz.step = Step::Params;
    assert_eq!(
        wiz.vault_hint("admin_key_env").as_deref(),
        Some(vault.as_str())
    );
    assert!(screen(&mut wiz).contains(&vault));

    let mut wiz = Wizard::new(&mistral(false), Some(0));
    wiz.step = Step::Params;
    assert_eq!(wiz.vault_hint("admin_key_env"), None);
    assert!(!screen(&mut wiz).contains(&vault));
}
