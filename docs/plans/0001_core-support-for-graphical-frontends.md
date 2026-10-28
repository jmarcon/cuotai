# PRD 0001 — Core support for graphical front ends

| Field | Value |
|---|---|
| ID | 0001 |
| Status | Implemented (RF1–RF11 in 0.1.1 to 0.7.1, internal versions; FIX-08 in 0.80.0; the RF8 catalog wording and the FIX-08 acceptance were later amended by PRDs 0033, 0034 and 0035, see the tables) |
| Owner | Backend (`cuotai-core`) |
| Consumers | `apps/desktop` (Tauri), `cuotai-tui` |
| Extended by | PRD 0029 (stable account `id` in the TOML and the account operations), PRD 0034 (English core fallback of `Localized`), PRD 0051 (tolerant/strict TOML parsing with field-position diagnostics; fix requirement COMP-01), PRD 0054 (RF3: resume- and reconnect-aware refresh deadlines) |
| Date | 2026-10-01 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement. The acronym is kept so that references in the code, commits and other plans stay stable.

## Implementation status (2026-10-23)

All RFs were implemented in the core, the TUI and the desktop app, one at a time, with validation (`cargo fmt`, `clippy -D warnings`, workspace and desktop tests, TUI and desktop UI smoke tests) before each version. FIX-08 was delivered as the core-only correction listed below.

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.1.1 | `wiremock` acceptance: saving 1 of 3 accounts makes 1 request; removing makes 0. |
| RF2 | 0.2.0 | Generic login modes described by the core (registration, storage, binding, warnings); reauthentication reuses the `credential_id` and removes the replaced key only after the TOML is written. |
| RF3 | 0.3.0 | The desktop app redoes the vault purge through an opaque handle, without depending on the removed index. |
| RF4 | 0.4.0 | The desktop app reloads the configuration after a conflict or external change and points errors to fields. |
| RF5 | 0.4.1, 0.7.1 | Obsolete `credential_modes` removed; account identity (badges, e-mail, ARN) derived in the core. |
| RF6 | 0.4.2 | Backoff never shortens configured intervals above 1 h. |
| RF7 | 0.4.3 | `AccountClass`, `MetricState` (`state` tag) and `CredentialKind` serialized directly from the core. |
| RF8 | 0.5.0 | Core catalogs for every emitted id; the desktop app merges them, the TUI resolves through `CUOTAI_LANG`. Amended by PRDs 0034 and 0035: `en-US` is the embedded English fallback of `Localized` (there is no `en-US.json` in the core), and the core ships `pt-BR`, `es`, `it` and ten more catalogs (13 files, 14 languages with `en-US`). |
| RF9 | 0.5.1 | Shared order without a spurious `params = {}` in the TOML. |
| RF10 | 0.6.0 | `Metric.kind` and `Metric.window` filled in by the adapters; text-based recognition is gone. |
| RF11 | 0.7.0 | Applicability per option and authentication type, replacement with a warning and validation in `build_account`. |
| FIX-08 | 0.80.0 | Metered refresh floors are validated per account: an account override is independent of the global interval, and a rejection identifies the field and account through a localized error. |

Section 2 and the "Technical assessment" describe the situation of 2026-10-01, before the implementation.

## 1. Context

`cuotai-core` already holds the account-editing rules (`account_edit`), TOML persistence (`config_file`), metric classification (`metrics`) and scheduling (`refresh`). The desktop app (`apps/desktop`) started consuming these APIs and works for listing, adding, editing and removing accounts.

While integrating, gaps appeared that only the core can solve. Today the desktop app works around some of them with its own code in `apps/desktop/src-tauri/src/backend.rs`, which duplicates policy and costs money on providers billed per query.

## 2. Problem

1. **Every edit redoes the query of every account.** `RefreshCycle::new` always creates a cycle without a snapshot and `Monitor` keeps the configuration immutable. Saving one account triggers a full round, including on paid providers (AWS Cost Explorer).
2. **OAuth login exists only in the TUI.** The orchestration is in `crates/cuotai-tui/src/auth_ui.rs` (`LoginTask::oauth/aws/bailian_console/store`, `paste`, `open_browser`). The core exposes the primitives (`OAuthSession::begin/finish/finish_with_codes/save_login/logout`), but no graphical front end can drive the flow.
3. **Removing an account leaves an orphaned credential in the vault.** `Config::without_account` only changes the TOML; `OAuthSession::logout` is a separate step.
4. **Configuration discovery and conflicts do not serve graphical apps.** `ConfigFile::load` tries `./cuotai.toml` in the current directory, which is unpredictable in a graphical app. `save` and `build_account` errors are `String`, so the UI cannot tell a conflict, a validation error and an I/O error apart.
5. **Provider policy duplicated in the front end.** The desktop app hard-coded `AwsBedrock → profile` and `AlibabaTokenPlan → no credential`.
6. **Incomplete scheduling.** `RefreshCycle` computes deadlines, but each front end rewrites the loop, the in-flight account tracking and the protection against results from an old configuration. After `RateLimited` or an authentication error, rescheduling uses the same interval. The 30 s minimum does not distinguish providers billed per query.
7. **Repeated DTOs.** `MetricState` and `AccountClass` are not `Serialize`, and the desktop app maps them by hand.

## 3. Goals

- Let any front end (TUI, desktop, future ones) edit accounts, sign in and schedule refreshes using **only** core APIs, with no policy of its own.
- Avoid unnecessary provider queries after edits.
- Leave no orphaned secrets and expose no secrets in events, errors or snapshots.

## 4. Non-goals

- Screens, UI texts or logos (the front ends' responsibility).
- Changing the semantics of the provider fetchers or the `cuotai.toml` format.
- Creating a daemon or an external service.
- Writing secrets to the TOML (still forbidden).

## 5. Functional requirements

### RF1 — Partial refresh after an edit (high priority)

- **RF1.1** `RefreshCycle` offers an operation to apply an edit, for example `apply_edit(&mut self, new_config: &Config, edit: Edit, now: Instant) -> Vec<usize>`, with `Edit::Upsert(i)` and `Edit::Remove(i)`.
  - It preserves the snapshot and deadlines of unaffected accounts.
  - It reindexes the snapshot and deadlines after a removal.
  - It returns the indices that need a new query (the edited or new account).
- **RF1.2** A report may be pending. An explicit state is defined (for example `Status::Pending` or `Option<Report>` in the cycle), so the edited account does not show data from the previous configuration.
- **RF1.3** `Monitor::with_config(&self, config) -> Result<Monitor, ConfigError>` reuses the HTTP client and the `OAuthSession`.
- **Acceptance criterion:** saving one account with N configured accounts makes exactly 1 query; removing one makes 0. A `wiremock` test counts requests.

**Edit contract:** the core derives the structural difference between the previous and the new configuration (or validates that it matches exactly the requested operation), including the identity and order of the preserved accounts. Indices supplied by the front end do not authorize associating old reports with reordered accounts. `Monitor::with_config` only reuses the HTTP client when `timeout_seconds` stays the same: the current reqwest client is built with that timeout; when it changes, create a new client or move the timeout entirely to each query before reusing.

### RF2 — OAuth login without a UI (high priority)

- **RF2.1** A flow API in `cuotai_core::oauth` that starts the login and returns a handle with:
  - serializable events: `Prompt { url, user_code, instructions }`, `WaitingForCode`, `Progress(text)`, `Completed(PublicParameters)`, `Error(kind)`;
  - `submit_code(text)` (a manually pasted code) and `cancel()`.
- **RF2.2** It covers every flow currently in the TUI: browser OAuth and device code, the Cursor import, AWS SSO (CLI) and the Bailian console.
- **RF2.3** `open_browser(url)` moves to the core, with the same behavior as in the TUI.
- **RF2.4 (revised)** The OS vault and the TOML are independent stores; there is no atomic commit across them. The core coordinates preparation/validation, write order, safe compensation and a typed result `PartialFailure { stage, config_saved, credential_saved, compensation }` when a stage or its compensation fails. Never delete, as compensation, a previous token that was replaced without successfully restoring it. The TUI presents the exact state and a recovery option, without inventing policy. Cancelling before the commit keeps the token in memory only. AWS/`bl` write outside the Cuotai vault and promise no rollback.
- **RF2.5 (revised)** The public device-authorization `user_code` may be shown to the user in the prompt event. The authorization code received in the browser callback, tokens and redirect URLs with credential parameters never enter events, errors or logs. The prompt must not be serialized indiscriminately.
- **RF2.6 (principle)** Login orchestration belongs to the core; the front ends only render events and send user input. The choice of flow per provider (browser with loopback, device code, Cursor import, MiMo's pasted code, AWS SSO, Bailian console) is made in the core. No front end contains `if provider == ...` to decide how to authenticate.
- **RF2.7 (mode discovery)** The core exposes, per provider, the accepted authentication modes (environment variable, native login, AWS profile) with their descriptors: non-secret flow options (method, region, URL), whether your own registration is needed (`client_id`, tenant or the client secret variable) and the reason when login is not offered. Today the desktop app has to combine `supports_login`, `oauth_unsupported_reason`, `requires_client_registration` and `native_option_descriptors` on its own, per account.
  > **Amended by [PRD 0047](0047_typed-secrets-in-the-desktop-environment-variables-in-the-terminal.md) (2026-10-27):** modes are presented per frontend: the desktop asks for the key itself (vault), the TUI for an environment variable.
- **RF2.8 (reauthentication)** Reauthenticating an existing account reuses the `credential_id` and overwrites the token, leaving no orphaned credential. The decision lives in the core.
- **RF2.9 (opening the browser)** If the browser cannot be opened, the core emits an event with the URL for the front end to show and copy. Only `https://` URLs are opened.
- **Additional criterion (RF2.6–RF2.9):** adding a new flow in the core makes it available in the TUI and the desktop app without changing either one's login logic.

- **Acceptance criterion:** the TUI starts using the same API (with no login logic of its own) and the existing OAuth contract tests keep passing.

### RF3 — Coherent removal (medium priority)

- **RF3.1 (revised)** A single operation `remove_account(&mut ConfigFile, session, index, purge_credentials: bool)`: validate the index and references, save the TOML, then attempt the purge if requested. Check the **effective vault key** derived from the provider, the OAuth configuration and the `credential_id`; if another remaining account references it, keep the secret. No TOML write failure deletes a token. If the purge fails after the write, return a typed result `PartialFailure { stage, config_saved: true, credential_saved: true, compensation }` or a dedicated variant indicating **removal saved, token kept**; recovery/retry belongs to the core and does not depend on an index that has already been removed.
- **Acceptance criterion:** tests with an in-memory `TokenStore` for keeping/deleting depending on the option, partial failure, and two accounts with the same effective key preserving the survivor's credential.

### RF4 — Configuration discovery and typed errors (medium priority)

- **RF4.1** A discovery mode without the current directory: an explicit path, then the system configuration directory. It includes creating the file on first use.
- **RF4.2** `ConfigFile::save` and `build_account` return an error enum, for example `EditError`, with at least `Validation { field: Option<&'static str>, message }`, `Conflict`, `Io` and `Unsupported`. Messages still do not echo typed values or TOML content.
- **RF4.3** `ConfigFile::reload()` and a cheap way to know whether the file changed on disk, so the UI can offer "reload" after a `Conflict`.
- **Acceptance criterion:** the current conflict, symlink and `0600` permission tests keep passing; a new test covers `reload` after an external edit.

### RF5 — Credential policy in the core (medium priority)

- **RF5.1** `account_edit::credential_kind(provider) -> CredentialKind` (`EnvVar`, `AwsProfile`, `None`) and an indication of which modes the provider accepts (env, OAuth, both), eliminating the desktop app's `match`.
- **Acceptance criterion:** no front end contains a `match` on `Provider` to decide the credential form.

### RF6 — Schedule driver and billed providers (medium priority)

- **RF6.1** `RefreshCycle` exposes `due_indices(now) -> Vec<usize>` and in-flight account tracking (`begin(index)` / `finish_account`), with protection against finishing an account from an old configuration (a generation).
- **RF6.2** Capped exponential backoff after `RateLimited` and `AuthenticationError`, respecting a ceiling (for example 1 h). A success restores the configured interval.
- **RF6.3** Providers billed per query are identified (`Provider::is_metered()` or equivalent). For them:
  - `Config::validate` enforces an interval minimum greater than 30 s;
  - the form receives a cost warning coming from the descriptor.
- **Acceptance criterion:** `RefreshCycle` tests for backoff, generation and the billed-provider minimum.

**RF6 execution decision:** initially only `AwsBedrock` is marked as billed per query, with an effective minimum interval of 300 s (the current default); any other provider needs evidence of billing before joining the list. For `RateLimited` and `AuthenticationError`, double the interval on each consecutive failure with a 1 h ceiling and reset after `Ready`; the user keeps explicit manual refresh. Document this behavior in the form/README and also test the global fallback.

### RF7 — Serializable types (low priority)

- **RF7.1** `MetricState` and `AccountClass` derive `Serialize`, with a stable (`snake_case`), documented representation.

### RF8 — Localizable core messages (medium priority)

- **RF8.1** Texts the core hands to the interface (labels and help of the field descriptors, cost warnings, reasons for unavailable login, edit errors, report notes) currently come out in fixed Portuguese, so the desktop app cannot translate them (the UI already translates everything else in pt-BR, en-US and es).
- **RF8.2** Each text gets a stable identifier (`field.kiro.auth_method.label`, `edit_error.duplicate_name`, …) together with parameters, and the Portuguese text stays as the default value. The front ends resolve the identifier in their catalog and use the default when it does not exist.
- **Acceptance criterion:** no front end needs to match on text; adding a new language is just adding a catalog.

### RF9 — Shared account order (low priority)

- **RF9.1** Today the order the user picks in the desktop app is only a display preference of that app (it does not change `cuotai.toml`), so the TUI and the desktop app can show different orders. For a single order, the core offers `Config::move_account(from, to)` and `RefreshCycle::apply_edit` with `Edit::Move { from, to }` (reindexing the snapshot and deadlines without a new query).
- **Acceptance criterion:** reordering makes no query and the new order persists in `cuotai.toml`, preserving comments.

### RF10 — Metric type in the model (medium priority)

- **RF10.1** `Metric` gets an explicit type (`Quota`, `RateLimit`, `Balance`, …) filled in by the adapters. Today a rate limit is identified only by free text (`period: "current rate-limit window"`, the "Rate limit" label), and the desktop app has to recognize those terms so as not to treat a window of minutes as the quota's reset (ordering by reset and % available).
- **RF10.2** `Metric` also reports the quota's **window** (`Window5h`, `Weekly`, `Monthly`, `Other`), filled in by the adapters. The main screen shows the 5-hour and weekly windows side by side, and today the desktop app recognizes them from free text (`label`/`period`: "Session (5 hours)", "M-series (5h)", "Coding Plan: 5h", "Weekly", "weekly (7d)"). With the field in the core, that text matching leaves the shell.
- **Acceptance criterion:** no front end compares `label` or `period` text; the existing adapters (Kimi, Groq, MiniMax…) classify their metrics and contract tests cover each type.

### RF11 — Option applicability per authentication type (medium priority)

> **Amended by [PRD 0047](0047_typed-secrets-in-the-desktop-environment-variables-in-the-terminal.md) (2026-10-27):** "environment variable" is a TUI authentication type; the desktop's `credential` type is the typed, vault-held credential.

**Problem.** Some provider fields offer a list of methods or credential origins that does not hold for every authentication type. The Kiro case: the `auth_method` field (a field descriptor, used with an **environment variable**) lists 7 values (`builder-id`, `idc`, `google`, `github`, `import`, `api_key`, `external_idp`), while the native login (an option of the `oauth` mode) supports only `builder-id` and `idc`. Today this works because they are two independent lists, and the front end merely swaps one for the other. But the core does not declare **which values make sense with an environment variable**: `builder-id` and `idc` issue short-lived tokens, which Cuotai does not renew from a variable, and the adapter accepts all 7 without distinction (`router_sessions.rs`: only `api_key` and `external_idp` change the header). Result: the user can pick a meaningless combination, and each front end would have to invent the rule.

**Requirements**

- **RF11.1 (metadata)** Each option of a choice field (a `Choice`-type `FieldDescriptor` and login options) declares which **authentication types** it holds for: `credential` (environment variable or profile) and/or the login mode ids (`oauth`, `aws_sso`, `bailian_console`). An option with no declaration holds for all (compatibility).
- **RF11.2 (query)** Given `provider` and the chosen authentication type, the core exposes the list of valid options for each field (for example `field_options(provider, key, auth_mode)`), with the default value for that type. The front ends stop keeping their own lists; switching the authentication type switches the list.
- **RF11.3 (switching type)** When the authentication type changes, if the field's current value stops being valid, the core indicates the substitute value (the type's default) and the front end announces the switch instead of silently keeping an invalid value.
- **RF11.4 (validation)** `build_account` rejects an invalid combination (for example, `auth_method = idc` with an environment variable) with a typed error and a stable identifier (`edit_error.option_not_valid_for_auth_mode`, parameters: field, value, authentication type), without echoing the typed value beyond what is already visible to the user. Validation also holds for existing accounts when they are edited; accounts already saved with an invalid combination keep being read and queried, and the warning appears only on edit.
- **RF11.5 (provider audit)** Review all providers' choice fields (Kiro, Gemini AI and Grok `product`, Kimi `product`, Copilot `mode`, CodeBuddy `region`, among others) and declare each option's applicability. Deliver the resulting table (provider, field, option, authentication types) in the documentation.
- **RF11.6 (texts)** The reasons an option is unavailable for an authentication type use `Localized` (RF8), with stable identifiers.

**Acceptance criteria**

- For Kiro, the `auth_method` list with an environment variable contains **neither** `builder-id` nor `idc` (see the decisions below), and the login's list contains exactly the values the native flow supports.
- Switching from an environment variable to login and back produces the right list and the substitute value, in both front ends (TUI and desktop), with no logic of their own.
- A contract test per provider: each declared option holds for the given types, and `build_account` rejects the others.
- Existing accounts with an old combination keep loading and being queried.

**Decisions taken for the implementation**

1. With an environment variable, Kiro accepts `google`, `github`, `import`, `api_key` and `external_idp`; `builder-id` and `idc` stay only in the native login.
2. `auth_method` is a single shared descriptor, with applicability declared per option. Region/start URL remain the login's own descriptors.
3. The audit found no other incompatibility per authentication type that the adapters prove. The remaining choices stay valid in every mode offered by their provider; the full matrix is in `docs/providers.md`.

## 6. Non-functional requirements

- **Security:** secrets never appear in snapshots, events, errors or logs; error messages do not echo user input.
- **Compatibility:** the TUI keeps working and starts consuming the same APIs. No format change in `cuotai.toml`.
- **UI independence:** the core does not depend on Tauri, Ratatui or any window framework.
- **Quality:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --all -- --check` pass. Provider tests use deterministic local fixtures.

### Fix requirements (2026-10-21 review)

Added on 2026-10-23 (owner decision J): defects found by the 2026-10-21 requirements review in this delivered plan, in an area no new PRD covers. They are non-functional requirements of this plan, delivered as `fix` commits.

| ID | Requirement | Acceptance |
|---|---|---|
| FIX-08 | `Config::validate` (`lib.rs`, about lines 105–115) rejects a global `refresh_seconds` below 300 even when the AWS Bedrock account sets its own interval. The metered floor applies only to metered accounts **without** their own override, is checked per account, and the error names the field and the account (`accounts[N].refresh_seconds`, or the global key) instead of the generic `ConfigError::Bounds`. The metered default and floor themselves (3600 s default, 900 s floor, a cross-process last-query lock) are set by PRD 0033. | Validation tests: a global 120 s with a Bedrock account at 3600 s is accepted; a Bedrock account without an override under a global 120 s is rejected with a message naming the field; non-metered accounts are unaffected. **Superseded in part by PRD 0033 RF5:** a metered account without an override under a short global interval now uses the metered default (3600 s) instead of being rejected; only an account override below the floor is rejected, and the error still names `accounts[N].refresh_seconds`. |

## Technical assessment — core and TUI (2026-10-01, historical)

The decisions below assess the requirements; they do not automatically approve the suggested signatures. The core keeps policy, state and persistence; the TUI keeps Tokio tasks, keys, rendering, texts and human confirmation. `Snapshot`/CLI and the TOML format stay compatible.

| RF | Decision and current gap | Proposed responsibility and acceptance criterion |
|---|---|---|
| **RF1** | **Accept, revise the pending representation.** `refresh.rs` requires recreating the cycle on edits, with no snapshot; `ui.rs` recreates the cycle and triggers a full query. `Monitor` already shares a semaphore, but not an HTTP client across configurations. | The core applies an edit/removal preserving the reports and deadlines of untouched accounts, invalidates only the edited report, and provides a monitor for the new configuration with the client/session reused. Internally, `Option<Report>` slots make the pending state explicit; a per-account view does not expose old data, and a complete `Snapshot` is produced only when every slot has a report, without changing its `Vec<Report>` or the CLI JSON. The TUI cancels/ignores old tasks, starts only the indicated query and shows the pending account. Test 1 request for an edit and 0 for a removal, including the TUI; do not query paid accounts as a side effect. |
| **RF2** | **Revise, do not block RF1/RF3/RF4.** `OAuthSession` already offers `begin`, `finish`, import and storage; `auth_ui.rs` still orchestrates OAuth, the AWS CLI, the Bailian CLI, the browser and the codes. | The core offers a cancellable operation and typed events for the flows it controls; the TUI keeps presentation, code entry, confirmation and, preferably, opening the browser through the platform. Separate OAuth stored by Cuotai from AWS/`bl`, which write outside the Cuotai vault: do not promise an impossible rollback. `LoginPrompt` today contains the URL and device code; deliver prompts only to the UI consumer that will show them, never serialize/log indiscriminately nor include URLs with credentials or authorization codes in events (RF2.1 × RF2.5). Coordinate the vault and TOML writes with explicit recovery from partial failures; there is no atomic transaction across them, nor is it safe to delete the previous token on rollback. Test cancellation, failure at each stage and absence of leaks. Resuming after the app closes: choose a restarted flow (no new persistence). |
| **RF3** | **Revise order/failures, implement in phase 1.** `without_account` only removes from the configuration; `logout` deletes from the vault; the TUI saves without purging. | The core offers coordinated removal: validate the index, save the TOML first and only then, if requested and if the derived key is not shared by another account, delete the secret. A vault failure after saving must be reported as **config saved, secret not deleted**, not as a fictitious rollback; allow another cleanup attempt. The TUI asks whether to purge and reports a partial result, without touching the vault directly. The default purge policy remains a product decision (question 4), but does not prevent the API/tests for both values. |
| **RF4** | **Accept with separate errors.** `ConfigFile::load` prioritizes `./cuotai.toml`; `save` already compares bytes and preserves comments/symlinks/permissions, but returns `String` and offers no reload. `build_account` also returns `String`. | The core adds explicit discovery without the CWD for GUIs, without changing the CLI/TUI precedence; typed validation, conflict, I/O and unsupported errors that do not echo input; reload reads again and swaps state only on success. `changed_on_disk` uses the byte comparison already required to avoid false negatives (mtime alone is not enough). The TUI uses the types to allow reloading after a conflict and to preserve the draft; it does not bring fs/TOML into `ui.rs`. Test conflict, external reload, symlink and 0600. No open product question. |
| **RF5** | **Accept, bring forward when useful.** The core already provides `default_env`, `credential_help`, `resolve_credential_env` and `supports_login`, but not a full type/modes descriptor; there is still a form `match` in the desktop app. | The core publishes the credential type and allowed modes consistent with `build_account` (AWS profile, Alibaba Token Plan `bl` profile/override, env/OAuth per provider), without turning an empty default into policy. The TUI consumes the descriptor and decides only presentation. Test all providers and prevent a policy `match` in a front end. No open decision. |
| **RF6** | **Revise/split.** `RefreshCycle` has only `is_due`/`finish_*`; the TUI keeps `Vec<JoinHandle>` and the desktop app keeps `busy`/generation. Every interval accepts 30 s; errors do not change the deadline. | The core centralizes eligibility, deadline and result generation after RF1; the TUI remains the owner of handles/cancellation and calls the core before accepting a result, including a full/manual refresh. `due_indices` is optional if it does not remove duplicated policy. Capped backoff after `RateLimited` is reasonable; `AuthenticationError` needs a re-login signal and an explicit policy before retrying. Mark billed providers and enforce the minimum by the **effective interval**, including the global fallback, only after defining the value/list (question 2); the warning comes from the core and the style from the TUI. Do not couple the driver to a runtime/framework. |
| **RF7** | **Accept, independent.** `AccountClass` and `MetricState` in `metrics.rs` do not implement `Serialize`; the TUI already uses the enums with no policy strings. | The core derives `Serialize` with `snake_case` for variants and stable structured fields, documents the format and tests representative serialization. The TUI keeps colors/labels; it does not need to serialize to render. The existing `Snapshot`/CLI contract stays intact. No open decision. |

**Recommended sequence:** phase 1 keeps RF1 + RF3 + RF4; RF5 and RF7 can be done independently, without waiting for RF2 or RF6. In phase 2, migrate the TUI with RF2 without assuming atomicity across storage systems. Split RF6: generation protection together with RF1; cost/backoff policy after the interval decision and a review of the errors. Only the pending choices (RF1), cost (RF6), resumption (RF2) and the purge default (RF3) correspond to the open questions; the other gaps do not depend on them.

**Sources in the repository:** `crates/cuotai-core/src/refresh.rs:13-90`, `lib.rs:54-81,89-169`, `config_file.rs:33-164`, `account_edit.rs:35-126,1136-1185,1419-1459`, `oauth/session.rs:51-105,175-204`, `oauth/store.rs:47-57`, `metrics.rs:25-38,132-148`; `crates/cuotai-tui/src/ui.rs:203-259,375-479,645-730`, `auth_ui.rs:21-145`, `dashboard.rs:41-94`; `apps/desktop/src-tauri/src/backend.rs:27-35,168-205,332-388,505-528` (consumer evidence only).

## 7. Phased plan

Executed in the sequence RF1 → RF11 (see "Implementation status"); the original phases (1: RF1/RF3/RF4; 2: RF2; 3: RF5–RF7/RF11) were covered.

## 8. Impact on consumers

- **Desktop:** uses `apply_edit`, the credential descriptors, login modes and options from the core; shows per-field errors, reloads after a conflict and redoes the vault purge.
- **TUI:** `auth_ui.rs` only renders `LoginDriver` events; it uses `remove_account`, typed errors and the same descriptors.

## 9. Questions (decided)

1. **Pending state:** `Option<Report>` per slot in `RefreshCycle`; `Snapshot` only exists complete, with no new `Status` and no change to the CLI JSON.
2. **Billed providers:** only AWS Bedrock (Cost Explorer), with an effective minimum interval of 300 s, including through the global fallback.
3. **Login resumption:** there is no resumption; the login is cancelled on close and restarted. Authorized tokens stay in memory only until the commit.
4. **Purge on removal:** optional; the TUI and the desktop app ask, and a credential shared by another account is never deleted.
5. **RF11:** Kiro with a variable uses `google`, `github`, `import`, `api_key` or `external_idp`; login uses `builder-id` or `idc`; both share one descriptor with applicability per option (see the RF11 section).

## 10. References in the code

- `crates/cuotai-core/src/refresh.rs` — `RefreshCycle`
- `crates/cuotai-core/src/lib.rs` — `Monitor`
- `crates/cuotai-core/src/account_edit.rs` — `build_account`, `Config::upsert_account`, `Config::without_account`
- `crates/cuotai-core/src/config_file.rs` — `ConfigFile::load`, `save`
- `crates/cuotai-core/src/metrics.rs` — `MetricState`, `AccountClass`
- `crates/cuotai-core/src/oauth/session.rs` — `OAuthSession`
- `crates/cuotai-core/src/oauth/login.rs` — `login_modes`, `start_login`, `LoginDriver`, staged commit
- `crates/cuotai-core/src/text.rs` and `crates/cuotai-core/locales/` — `Localized` and the catalogs
- `crates/cuotai-core/src/identity.rs` — the account's non-secret identity
- `crates/cuotai-tui/src/auth_ui.rs` — rendering of login events
- `apps/desktop/src-tauri/src/backend.rs` — Tauri bridge with no policy of its own
