# PRD 0047 — Typed secrets in the desktop, environment variables in the terminal

| Field | Value |
|---|---|
| ID | 0047 |
| Status | In progress |
| Owner | `cuotai-core` (`secret_input` module, `user_session` vault slots, `account_edit::build_account_with_secrets`, `suggest_env_for`, `detect::desktop_sources_here`), desktop backend (`save_account`, `get_account`, `form_schema`, `change_session`), desktop UI and TUI (follow-up agents) |
| Consumers | Desktop users adding or editing an account; terminal (TUI/CLI) users with one or several accounts per provider |
| Depends on | PRD 0016 (vault reference `[accounts.session]`, `stage_pasted`, SES-02 test before save), PRD 0036 RF2.1 (`Delivery::Secret` slots) |
| Amends | PRD 0001 RF2.7 and RF11, PRD 0016 RF1.2 and RF5.1, PRD 0027 RF6.5, PRD 0036 RF2.1, PRD 0037 RF1.4, PRD 0038 RF1.3 and the RF12.2 note (see §8) |
| Date | 2026-10-27 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement. In this plan **G** numbers the GUI requirements and **T** the terminal requirements; **E** numbers the core enablers, which are not user-facing.

## Implementation status (2026-10-27)

| Item | Version | Delivery |
|---|---|---|
| E1–E7 (core and desktop backend contract) | vNEXT | Delivered on `feat/gui-keys-core`: `secret_input` module, generalized vault slots, `build_account_with_secrets`, typed `secrets` in `save_account`, `secret_states`/`credential_stored`/`vault_migration` in `get_account`, `secret_inputs`/`terminal_only_fields` in the form schema, generalized migration, desktop detection without variables, `suggest_env_for`. |
| G1–G6 (desktop UI) | vNEXT | Delivered on `feat/gui-keys-ui`: the add/edit dialog renders `secret_inputs` as masked inputs (show/hide, never prefilled) in place of the variable-name row and the terminal-only fields, sends `secrets` with `credential: ""`, shows "Saved in the system vault" with Replace (blank keeps), "Key set up in the terminal" with **Move key into Cuotai** (`vault_migration`, two-step confirmation, unreadable setups never name the variable), checks missing required secrets in its own words, and the detection consent no longer mentions variables; demo fixtures and the GUI guides follow. |
| T1–T2 (TUI) | vNEXT | Delivered on `feat/gui-keys-tui`: the wizard prefills `suggest_env_for` for a new account, on provider switch and when leaving the name step (a typed variable is kept). |
| T3–T4 | vNEXT | T3: `credential_help.*` and the `query.*`/`note.*` ids stay terminal-worded (the desktop renders `SecretInput.help`); the GUI-neutral split of the remaining shared query notes stays a follow-up. T4: no change needed, vault and `credential_env` accounts load as before. |

## 1. Owner decision (2026-10-27)

In the desktop GUI, adding an account never mentions environment variables. The user types the API key and any other secret the provider needs (secret, token, web session) in the add-account flow; Cuotai stores it in the OS credential vault, as it already does for pasted web sessions and OAuth tokens. Environment variables are a terminal concept: each provider has a default variable for the TUI, and a terminal user with several accounts of one provider names a different variable per account.

## 2. Problem

- The desktop credential row is a variable-name field (`credential_env`): a desktop app does not inherit the shell environment on macOS or Windows, so people paste the key, `resolve_credential_env` refuses it, and they are told to "export" something.
- Parameter secrets (`admin_key_env`, `secret_env`, `token_env`, ...) are variable-name text fields too.
- Only 9router, LiteLLM and the cookie sessions could keep their secret in the vault.
- In the terminal, a second account of one provider prefills the same default variable and silently collides.

## 3. GUI requirements (desktop user)

- **G1 — No environment variables.** The add and edit flows, their help texts and errors never name a variable or ask for one. Fields that name a variable (`*_env`) are not rendered; device detection in the desktop checks tool folders only, never variables.
- **G2 — Type the secret.** Every API key, token, secret and web session the provider needs is a masked input in the flow, labelled in user terms ("API key or access token", "AccessKey secret", "Platform session").
- **G3 — Test before save.** Where the provider can be queried, the typed secret is tested by one read-only query before it reaches the vault (PRD 0016 SES-02): a rejection blocks the save, an unreachable provider offers "Save anyway". The secret is never echoed.
- **G4 — Stored in the vault.** The secret goes to the OS vault; the configuration file keeps only a reference. Editing an account shows "Saved in the system vault" with a Replace action, never the value; leaving the input blank keeps the stored secret.
- **G5 — Accounts set up in the terminal.** An account whose secret comes from the environment shows plainly "Key set up in the terminal" (read-only, no variable name) and a one-click **Move key into Cuotai** that copies the value into the vault and drops the variable from the file, after a consent line in user terms.
- **G6 — Failures without secrets.** No secret, variable name, URL or response body appears in an error, the TOML, a report, a snapshot or the logs; a failed vault write or file save leaves nothing behind (rollback).

## 4. Terminal requirements (TUI/CLI user)

- **T1 — Default variable per provider.** The TUI keeps suggesting `account_edit::default_env(provider)` (for example `XIAOMI_MIMO_API_KEY`) for the first account of a provider.
- **T2 — One variable per account.** For a further account of the same provider the TUI suggests a distinct name, `<DEFAULT>_<ACCOUNT NAME>` (then a numeric suffix), and the user may type any valid name.
- **T3 — Terminal wording.** Terminal help and errors keep naming variables; they are separate texts from the GUI ones, so changing one surface never changes the other.
- **T4 — Vault accounts in the terminal.** An account created in the desktop (vault reference) works unchanged in the TUI and CLI; existing TOML files with `credential_env` work unchanged everywhere.

## 5. Core enablers (contract, not user-facing)

- **E1 — Vault slots for every secret.** `user_session::vault_descriptor(provider, slot)` accepts, besides the PRD 0016/0036 session descriptors, every secret input of E2 as a `Delivery::Secret` slot. The TOML keeps `[accounts.session]` (`credential_ref` + `slots`); a vault-held main credential is the slot `credential_env` and leaves `credential_env` unset; a parameter secret is the slot of its `*_env` key. At query time the vault value stands in for the variable through the query credentials, as for sessions. `Config::validate` keeps rejecting a vault `credential_env` slot together with a `credential_env` name.
- **E2 — Secret inputs.** `secret_input::secret_inputs(provider) -> Vec<SecretInput>`: the main credential (for every `CredentialKind::EnvVar` provider with a default variable), each `*_env` parameter secret in field order, and each web session slot. Labels and help never mention a variable (tested).
- **E3 — Typed values.** `SecretValue { slot, value }` (no `Debug` value, wiped on drop) and `reduce_secrets(provider, &[SecretValue])` (unknown or duplicate slot refused, blank skipped, keys kept whole and checked, cookie sessions reduced to their declared cookies). The result feeds the existing `user_session::test_before_save` and `stage_pasted` (vault first, `rollback` on a failed save).
- **E4 — Drafts.** `account_edit::build_account_with_secrets(draft, config, editing, typed_slots)`: a vault-held main credential needs no variable name; a vault-held parameter secret is not required in the draft and, when typed, is not written to `params`. `build_account` is unchanged (it calls it with no slots).
- **E5 — State and migration.** `secret_states(account)` (`vault` / `environment` / `unset`, with a GUI label, never a name), `vault_migration_candidates(account)` and `user_session::stage_vault_migration` (sessions and keys alike, consent in user terms). The classic `migration_candidates`/`stage_migration` (web sessions only) stay for the TUI.
- **E6 — Desktop surface helpers.** `account_edit::is_terminal_only_field(provider, key)`; `detect::desktop_sources_here` (no variable detection).
- **E7 — Terminal helper.** `account_edit::suggest_env_for(provider, config, name, editing)` for T1/T2.

## 6. Non-goals

- Settings-level variables (`CUOTAI_CONFIG`, `CUOTAI_USER_DIR`, `CUOTAI_UPDATER`, hook variables): out of scope.
- Rewording every provider's `credential_help.*` and the `query.*` errors that cite a variable for the GUI: the desktop renders `SecretInput.help` instead of `credential_help`; the per-surface split of the remaining shared query notes is a follow-up (T3).
- OAuth client secrets (`client_secret_env` of the registration options): follow-up.
- Extension manifests' declared variables (PRD 0017): unchanged.

## 7. UI contract

All shapes are JSON as serialized by the desktop backend (snake_case, as the existing commands).

**`form_schema(provider)`** gains:

```json
"secret_inputs": [
  { "slot": "credential_env", "label": {"id": "secret_input.credential.label", "fallback": "API key or access token"},
    "help": {"id": "secret_input.help", "fallback": "..."}, "required": true, "cookie": false,
    "field": null, "default_env": "GROQ_API_KEY" }
],
"terminal_only_fields": ["secret_env", "security_token_env"]
```

- Render one masked input per `secret_inputs` entry; when `field` is set, show it only while that field descriptor is visible (`visible_field_indices`); `cookie: true` is a session paste box (the existing session preview/test applies).
- Never render a field listed in `terminal_only_fields`, nor `default_env` (terminal only), nor the old `credential` text row for providers whose `secret_inputs` start with `credential_env`. `credential_kind`, `default_env` and `credential_help` stay in the schema only until the UI switches; the GUI must not show them.
- The existing `sessions` array is unchanged; its slots also appear in `secret_inputs`.

**`save_account` / `test_session` / `test_aggregator` (`AccountInput`)** gain:

```json
"secrets": [ { "slot": "credential_env", "value": "<typed secret>" } ]
```

- Send `credential: ""` when the main credential is typed. A blank `value` (or an absent entry) keeps the stored secret. A slot may come once, either in `secrets` or in `sessions`.
- `save_account` runs the test before save on `sessions` + `secrets` together; a refusal is the core note (`user_session.save.rejected`, or `user_session.save.unreachable` with `save_anyway: true` allowed on retry). Errors never carry the value.

**`get_account(index)` (`AccountForm`)** gains:

```json
"credential_stored": true,
"secret_states": [ { "slot": "credential_env", "source": "vault", "label": {"id": "secret_input.state.vault", ...} } ],
"vault_migration": [ { "slot": "credential_env", "label": {...}, "consent": {"id": "secret_input.migrate.consent", ...} } ]
```

- `source` is `vault`, `environment` (show "Key set up in the terminal", read-only) or `unset`. Do not prefill any secret input; show "Saved" plus Replace when `source = vault`.
- `credential` still echoes the variable name for terminal-created accounts until the UI switches; the GUI must not display it. `session_migration` (classic, with `env`) is superseded by `vault_migration` for the GUI.

**`change_session(index, "migrate")`** now moves every entry of `vault_migration` (sessions and keys) into the vault; `forget` is unchanged.

**Device detection** in the desktop (`detect_sources`) no longer reports variables.

New core catalog ids (all 13 languages): `secret_input.help`, `secret_input.credential.label`, `secret_input.slot.<slot>.label` (`admin_key_env`, `token_env`, `session_env`, `management_key_env`, `analytics_key_env`, `secret_env`, `security_token_env`, `platform_session_env`), `secret_input.unknown_slot`, `secret_input.duplicate_slot`, `secret_input.state.{vault,environment,unset}`, `secret_input.migrate.consent`, `user_session.slot.secret`.

## 8. Superseded or amended requirements

| Requirement | Amendment |
|---|---|
| PRD 0001 RF2.7 | Authentication modes are presented per frontend: the GUI shows "type the key" (vault), the TUI shows "environment variable". |
| PRD 0001 RF11 | Option applicability per authentication type is unchanged, but "environment credential" is a TUI mode; the GUI's equivalent is the vault-held typed credential. |
| PRD 0016 RF1.2 | `session_env` stays a terminal alternative only; the GUI offers the paste box and the generalized move into the vault (G5). |
| PRD 0016 RF5.1 | Keeping `session_env` "as an alternative" applies to the TUI; the GUI hides the field (`terminal_only_fields`). |
| PRD 0027 RF6.5 | The desktop consent line says only what is checked in user terms and the desktop never checks variables; the TUI keeps its wording. |
| PRD 0036 RF2.1 | The descriptors are shared data with per-frontend presentation, not one form shared by the TUI wizard and the desktop. |
| PRD 0037 RF1.4 | The credential is a vault field in the GUI and a variable name (`ELEVENLABS_API_KEY` default) in the TUI. |
| PRD 0038 RF1.3 | The same: vault field in the GUI, variable name in the TUI. |
| PRD 0038 RF12.2 note | Any note telling the user to set a variable is terminal wording; the GUI says to add the key to the account. |

## 9. Acceptance criteria

- Core tests (`secret_input_tests.rs`): no GUI label/help names a variable; a typed Groq key is refused when wrong, tested, written to a `MemoryStore` vault, absent from the TOML, the report JSON and `Debug`, read by the query; editing keeps it; a failed vault write stores nothing and the rollback undoes a staged write; an environment account works unchanged, reads `environment`, moves to the vault with its consent naming no variable; a typed parameter secret is not written to `params`; `suggest_env_for` yields distinct valid names.
- Desktop tests (`backend.rs`): a typed key without the test clearance stores nothing; with it, only the vault holds the key and the TOML and `get_account` JSON carry no value; editing with a blank key keeps it; the schema lists the secret inputs and terminal-only fields; an environment key moves into the vault on `migrate` without its name reaching the form.
- Backward compatibility: existing TOML with `credential_env`, `*_env` params and `[accounts.session]` loads, validates and queries as before; `build_account` and `migration_candidates` behave as before.
