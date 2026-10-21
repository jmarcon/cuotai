# PRD 0016 — User-pasted session

| Field | Value |
|---|---|
| ID | 0016 |
| Status | In progress |
| Owner | `cuotai-core`, TUI and desktop |
| Consumers | `crates/cuotai-tui` (`wizard.rs`), `apps/desktop` (`account-dialog.js`) |
| Depends on | PRD 0005 (RF1: the `UserSession` level) |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-21)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.60.0 | `cuotai_core::user_session`. RF1.1: `descriptors(provider)` declares each session slot (`SessionDescriptor`: `slot`, the legacy parameter it supersedes; `required` and `optional` cookie names; the only `hosts`; `delivery` `cookie_header`; a localized `label`) for Xiaomi MiMo (`session_env`: `serviceToken` plus the `userId`/`mimo<region>_ph`/`_slh` cookies 9router keeps, the five `mimo-server-*.xiaomimimo.com` hosts; `platform_session_env`: `api-platform_serviceToken` and `userId`, `platform.xiaomimimo.com`), Alibaba Coding Plan (`session_env`, the four console hosts) and Alibaba Token Plan (`session_env`, `home.qwencloud.com` and `cs-data.qwencloud.com`); front ends never branch on the provider. `access::describe` and `account_edit::draft_access` (new `stored_slots` argument) count a vault slot as set, so a migrated account keeps the `UserSession` class; `account_edit::build_account` keeps the account's session reference on edits and drops it on a provider switch. RF1.2: `[accounts.session]` (`SessionConfig`: a random UUID `credential_ref` and the stored `slots`), validated by `Config::validate` (`ConfigError::Session` for an invalid UUID, an empty or undeclared slot, or a reference shared by two accounts); `migration_candidates` lists the `session_env`-style variables of a saved account with a localized `consent` text, and `stage_migration` reads them through `Credentials` (refusing, without writing anything, when one is unset or empty and naming only the variable), writes the vault, removes the parameters and adds the reference; the front end persists `StagedMigration::config` and calls `rollback_migration` when that fails. TUI: F3 on the wizard summary shows the consent and a second F3 migrates (any other key cancels), then the draft no longer names the variable; desktop: the account dialog's "Web session" box with "Move to the vault" clicked twice, the `migrate_session` command and `get_account`'s `session_slots`/`session_migration`. RF1.3: the vault entry uses the `TokenStore` of OAuth tokens (Keychain, Credential Manager, Secret Service, or the debug-only SQLite store with its warning; release builds refuse it), keyed by a UUID v5 of the reference, the provider and the slot, so a reference copied to another provider or slot finds nothing; `Monitor` loads the account's slots for each query and the adapter reads them through the query's `Credentials` under the slot's parameter, so accounts that keep `session_env` work unchanged. A reference without its vault entry is `NeedsConfiguration` (`user_session.not_found`). Tests: storage and read through `Credentials`, the migration (TOML without the value or the variable, reference reloaded, rollback on a failed vault write and on a refused save in the desktop backend), the development store across reopen, and the value absent from the TOML, `Debug`, errors and `Report`. |

**Decision (2026-10-21, owner):** sessions are pasted manually only; Cuotai never imports cookies from browsers. The Alibaba cookie names are the ones the adapters read plus the Aliyun login family, none marked required; RF2/RF5 must confirm them against a live session before extraction filters a paste by them.

## 1. Context

`docs/providers.md` establishes that no cookie, Keychain entry or credential file of another app is extracted automatically. Still, some providers only expose usage behind a web session, and Cuotai already accepts cookies exported by the user in specific cases:
- Xiaomi MIMO: `platform_session_env` (console balance and Token Plan) and `session_env`/`region` (Desktop weekly quota);
- Alibaba Token Plan: `params.session_env = "QWEN_CLOUD_COOKIE"`;
- Alibaba Coding Plan: `params.session_env` with an explicit cookie when the gateway requires login;
- personal Cursor: an **explicit** import of the local SQLite session.

Today each case has its own field, help text and validity handling. The user has to export the cookie on their own, put it in an environment variable and reference it, which is fragile and leaves the value in shell profiles.

In the 2026-10-19 market analysis, CodexBar covers dozens of providers by cookie (about 43 of 106 entries), with `auto` (imports from the browser), `manual` (pasted header) and `off` modes; this requires Full Disk Access (Safari) and Keychain access (Chromium). WhatsMyUsage takes the opposite path: the user pastes cookies into an app window, **only the session keys** go to the Keychain and the rest of the paste is discarded; two sessions of the same provider are two rows.

## 2. Problem

- Providers that only have a web query stay out of reach without automatic cookie import, which contradicts the project's policy and opens a broad permission surface.
- The current manual mechanism is inconsistent across providers and leaves the secret in environment variables.
- A pasted session is a full login: storing it carelessly is a real risk.

## 3. Goals

- Offer a single, explicit, opt-in way for the user to supply a web session for the providers that have no other route, without scanning browsers.
- Store the secret in the system vault, never in the TOML, logs, snapshots or events.
- Make the risk clear (the `UserSession` level of PRD 0005) and detect expiry.

## 4. Non-goals

- Automatically importing cookies from browsers, reading other apps' Keychain entries or asking for Full Disk Access.
- Automating web login, solving CAPTCHAs or using passwords.
- Covering providers without evidence that a session-based query is viable and permitted; each one enters through its own PRD or review.

## 5. Functional requirements

### RF1 — `UserSession` credential type in the core (high priority; prerequisite)

- **RF1.1** Add the `UserSession` credential class to `account_edit` and `access` (PRD 0005). The provider descriptor declares which cookies/headers are required (a list of accepted names, for example the session keys), the destination host and how the session is sent. The interface contains no per-provider rules.
- **RF1.2** A reference in the TOML by **vault identifier** (`credential_ref`, the same convention as OAuth tokens), never by value. Old accounts with `session_env` keep working; the wizard offers "migrate to the vault" with explicit consent.
- **RF1.3** Storage in a Cuotai-owned vault entry (Keychain, Credential Manager or Secret Service), with the same mechanism as `TokenStore`. In the development mode with plain-text SQLite, the existing warning still holds; release builds refuse it.
- **Acceptance criterion:** storage and read tests through `Credentials`, migration from `session_env` to the vault, absence of the value in the TOML, `Debug`, logs and `Report`.

### RF2 — Guided paste and reduction to the minimum (high priority)

- **RF2.1** A "Paste session" step in the account wizard: localized instructions per provider ("where to copy", always generic and without captures of real accounts), a masked paste field, and a test button that makes a read-only query.
- **RF2.2** The core accepts pastes in common formats (a `Cookie:` line, a `name=value; ...` list, a table copied from the inspector, a copied `cURL`) and **extracts only the names allowed** by the descriptor, discarding everything else before writing. Cookies whose name is not on the list never reach the vault. Normalization reassembles cookies fragmented into numbered parts (for example, `.0`, `.1`) in the right order, so as not to silently generate an invalid session.
- **RF2.3** Nothing of the pasted content is logged: the field is cleared on close, and error messages describe the problem ("cookie X is missing") without repeating values.
- **Acceptance criterion:** table tests for each format, out-of-order fragments, missing names, extra names discarded, and a check that no value appears in errors or logs.

### RF3 — Validity, expiry and use (high priority)

- **RF3.1** A query with a rejected session produces an `AuthenticationError` with a specific message "session expired or revoked; paste a new one", without exposing the body or URL.
- **RF3.2** When the cookie carries a known expiry date, the core reads it (only the date, never the value) and reports "expires in N days" on the card; the near-expiry warning (under 3 days) joins the alerts (PRD 0008) as an event of its own when it exists.
- **RF3.3** The session is only sent to the host declared in the descriptor, over HTTPS, and is never reused for another provider or account. Redirects to another host do not carry the session.
- **RF3.4** Updating an existing account's session replaces the value in the same vault entry (reauthentication), without duplicating accounts.
- **Acceptance criterion:** `wiremock` tests: 401, a redirect to an external host (session not sent), known and unknown expiry, and session replacement.

### RF4 — Transparency and risk warning (medium priority)

- **RF4.1** The card and the paste step show the `UserSession` level and a short localized warning: "This is a full login session. Cuotai stores it in the system vault and uses it only to query this service."
- **RF4.2** A "Forget this session" option on the account, which removes the vault entry.
- **RF4.3** `docs/providers.md` and `docs/oauth.md` describe the flow, what is stored, what is discarded and the risks (the provider's terms of use, revocation, leakage).
- **Acceptance criterion:** a smoke test in the TUI and the desktop app in three languages; removal deletes the vault entry.

### RF5 — First providers (medium priority)

- **RF5.1** Migrate the existing cases (Xiaomi MIMO, Alibaba Token Plan, Alibaba Coding Plan) to the `UserSession` descriptor, keeping `session_env` as an alternative.
- **RF5.2** New providers only enter with evidence (the vendor's documentation or a contract verified in 9router/a community client) that the query is read-only and that the session is enough, and they go through PRD 0005.
- **Acceptance criterion:** the migrated adapters' current tests keep passing; the `docs/providers.md` matrix reflects the change.

## 6. Non-functional requirements

- **Security:** a session is a high-value secret. Never in the TOML, snapshots, events, logs, exports (PRD 0012), hooks (PRD 0013) or error messages; zero the buffer when possible; no telemetry.
- **Privacy and terms:** use depends on the provider's terms. The documentation says so and the access level exposes the risk.
- **Compatibility:** existing accounts and TOML do not change without consent.

## 7. Order of execution and verification

1. **RF1:** credential, vault and migration, with tests.
2. **RF2 and RF3:** paste, minimal extraction, expiry and redirects.
3. **RF4:** warnings and removal.
4. **RF5:** migration of the three existing providers.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when the user pastes a session once, only the allowed cookies reach the vault, the value never appears in the TOML, logs or errors, an expired session is identified with a clear message, and forgetting it removes the vault entry.
