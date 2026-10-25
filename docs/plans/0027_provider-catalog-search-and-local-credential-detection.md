# PRD 0027 — Provider catalog, search and local-credential detection

| Field | Value |
|---|---|
| ID | 0027 |
| Status | In progress |
| Owner | `cuotai-core` (`account_edit` catalog and search, presence detection), desktop and TUI account editors |
| Consumers | `apps/desktop/ui/js/account-dialog.js`, `crates/cuotai-tui/src/wizard.rs`, PRD 0025 (RF5.4, RF6) |
| Depends on | PRD 0005 (RF1: `access::describe` credential class, level and metering) |
| Benefits from | PRD 0016 (paste flow), PRD 0017 (extension catalog entries), PRD 0022 (UI refresh: picker grid and tiles), PRD 0025 (first run), PRD 0036 (aggregator providers in the catalog) |
| Extends | PRD 0005 RF3 (the level shown in the picker becomes part of a searchable catalog entry) |
| Date | 2026-10-22; amended 2026-10-23 (catalog grouped by sign-in method per DS-CMP-LIST-6; owner decision Q: wider presence-only detection table, consent-gated) |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. **Presence detection** means checking that a known file or directory exists, without opening or reading it.

## Implementation status (2026-10-22)

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT | Core `account_edit::catalog` (`catalog.rs`): one entry per provider and mode, hint and sign-in method derived from the access descriptor, recommended mode, generated table in `docs/providers.md`. RF1.3: `catalog::search_with_extensions` and `account_edit::picker_search_with_extensions` add each discovered extension that can still be added (`Manager::picker_entries`: valid, not yet an account, not an override) as an `Extension` entry "Extension: {name}" with its description's first line as the hint, one result per extension; the TUI wizard and the desktop picker list them and open the extension's disclosure when chosen. |
| RF2 | vNEXT | Core `account_edit::search`: case- and accent-insensitive ranking, one result per provider unless the query names a mode, empty query grouped by sign-in method. |
| RF3 | vNEXT | Desktop catalog picker (`js/provider-picker.js`) over the core `search_providers` (`account_edit::picker_search`, keeping only providers with a usage query): sign-in method groups (grid at 560 px and above), ranked list for a query, hint, "Billed per query" and "Found on this device" chips (the "Billed per query" chip also stays on the account card after creation, from the report's `access.metered`, vNEXT), combobox/listbox keyboard, read-only header when editing, "Request a provider" via the allow-listed issue template. The desktop provider list (`list_providers`) follows the core picker order (`account_edit::picker_providers`, the TUI wizard's order) and the dialog never re-sorts it; a test keeps both orders equal. No level chip: the owner's 2026-10-25 rule removes integration-status labels from every UI surface. |
| RF4 | vNEXT | TUI wizard type-to-filter through `picker_search`, method headings with an empty filter, hint line, empty state; Models moved from `m` to `Tab`. |
| RF5 | vNEXT | Core `recommended_mode`/`other_modes`; both editors open the recommended mode, the desktop shows the others under "Other ways to connect", the TUI credential step names them (Tab). |
| RF6 | vNEXT | Core `detect::local_sources` (presence via `symlink_metadata`, reviewed table with evidence). The "to verify" rows were checked against each tool's own documentation on 2026-10-25 and enabled: Qwen Code, Kiro (CLI and IDE), Kimi CLI, Grok CLI and Zed (macOS/Linux); Antigravity stays out (no official path found). See RF6.4. Opt-in `detect.enabled` in `desktop.json` and Settings > Data & privacy; the TUI asks once per session (F6/F7). |
| RF7 | vNEXT | Offers open the existing flows (Cursor import, native login, AWS profile step, OpenCode key) with the core's consent line first; `detect.dismissed` (desktop) or F8 for the session (TUI). |

## 1. Context

Cuotai supports 36 providers (the `Provider` enum; the same 36 slugs are listed by hand in `.github/ISSUE_TEMPLATE/provider-problem.yml`), several with more than one way to connect (`auth_mode`: API key, OAuth/native login, pasted session, local CLI). The desktop picker is a plain `<select>` (`apps/desktop/ui/js/account-dialog.js`, about line 62) whose options read "label · level"; the TUI wizard is a list without filtering. Neither tells, before choosing, what each provider needs ("an admin API key", "sign in with your browser", "AWS CLI profile").

Credentials are never imported silently, by design: `providers/subscriptions.rs` states in its header that Cuotai does not extract the ChatGPT or Claude Code credentials from the keychain or CLI, PRD 0016 forbids reading browsers' cookies or other apps' Keychain entries, and the one local import that exists (`oauth/cursor_import.rs`, Cursor's `state.vscdb`, with `candidates()` for the well-known locations) runs only when the user asks for it in the wizard. As a result, a person who already uses Cursor, Claude Code or Codex on the same machine gets no hint that a faster way to add the account exists.

**Market expectation.** A paid macOS monitor (WhatsMyUsage) shows a searchable "Add account" catalog of about 69 agents, each with a one-line hint of how it connects, and promises "no data-source pickers". This PRD covers search, hints, a recommended default per provider and explicit, consented offers based on presence detection. Matching the competitor's provider count is not claimed (§4).

## 2. Problem

- Scrolling a 36-item `<select>` (and more as providers and extensions grow) is slow, and synonyms ("Anthropic", "Codex", "Copilot") do not match the labels.
- The choice of connection method comes before the person knows which one is easiest.
- Local tools already signed in are invisible to the editor, so people paste tokens they could have imported with consent.

## 3. Goals

- One core catalog entry per provider and connection mode, with display name, aliases, sign-in method, category and a one-line connection hint derived from the access descriptor; the catalog is **grouped by sign-in method** (DS-CMP-LIST-6).
- One core search used by both editors, tolerant to case, accents and aliases.
- A recommended connection mode per provider, preselected, with alternatives one click away.
- Presence-only, consent-gated detection of known local tools (Gemini CLI and the other CLIs of RF6.4) that **offers** the existing import or login flow, never reads content without an explicit click.

## 4. Non-goals

- Reading credential files, Keychain entries, browser cookies or protected (TCC/Full Disk Access) locations during detection; PRD 0016 §4 stands unchanged.
- Silent or background import, or detection at startup.
- New providers. Cuotai has no verified list of which providers the market competitor covers; additions keep going through the "Adding a provider" guide and the new-provider issue template. RF3.5 links that template from the empty search state.
- Ranking providers by popularity or telemetry.

## 5. Functional requirements

### RF1 — Core catalog descriptor (high priority; prerequisite)

- **RF1.1** `account_edit::catalog() -> Vec<CatalogEntry>`, exhaustive over `Provider` (a `match` with no wildcard, like `access::classify`) and over each provider's connection modes. `CatalogEntry` carries `provider`, `mode` (`auth_mode` id or `None`), `display_name: Localized`, `aliases: &'static [&'static str]`, `method` (`SignInMethod::{BrowserSignIn, LocalCredentials, ApiKey, CloudProfile, WebSession, Extension}`: browser sign-in, local app or CLI credentials, API or admin key, cloud profile or identity, pasted web session, extension), `category` (`Subscription`, `ApiPlatform`, `Cloud`, `CodingTool`, `Router`, `Extension`; kept as a search term only), `hint: Localized` (one line), `level` and `metered` (from `access::describe`), `native_login: bool`, `local_import: Option<LocalSource>` and `recommended: bool`.
- **RF1.2** The hint is derived from the credential class and the native-login descriptor, not written per provider: `InferenceKey` → "API key", `AdminKey` → "Admin API key", `SubscriptionSession` with native login → "Sign in with your browser", `CloudIdentity` → "Cloud credentials", `LocalCli` → "Uses the {cli} CLI", `UserSession` → "Paste a web session"; plus "billed per query" when `metered` (AWS Cost Explorer). Overrides exist only where the derived line would be wrong, listed in one table.
- **RF1.3** Extensions discovered by PRD 0017 appear as `Extension` entries with "Extension: {name}" and their own hint.
- **RF1.4** A generated catalog table in `docs/providers.md` (display name, aliases, sign-in method, category, hint), checked by the `providers_doc_matrix` drift test.
- **RF1.5** `method` is derived from the mode's credential class and native-login descriptor (the same table as RF1.2), so a provider with two modes appears under two methods (for example Cursor under "Browser sign-in" and "API key"); a group label per method comes from the core catalogs (`catalog.method.*`).
- **Acceptance criterion:** an exhaustiveness test (every `Provider` and mode has one entry; exactly one `recommended` per provider; every entry has one `method`), hint and method derivation table tests, catalog keys present in `en-US`, `es` and `it` with the Portuguese fallback.

### RF2 — Core search (high priority)

- **RF2.1** `account_edit::search(query: &str, locale) -> Vec<CatalogMatch>`: case- and accent-insensitive (Unicode NFKD folding), matching the localized display name, the slug, aliases, the sign-in method label and the category name; scoring exact > prefix > word prefix > substring > alias; ties keep catalog order; one result per provider (its recommended mode) unless the query names a mode ("cursor admin").
- **RF2.2** An empty query returns the catalog **grouped by sign-in method** (one group per method, in the RF1.1 order, providers in catalog order inside each group; DS-CMP-LIST-6); search results are always a flat list ranked by the core (DS-CMP-CAT-1); a query with no match returns an empty list (RF3.5 handles the state).
- **Acceptance criterion:** table tests in the four languages: "codex" → ChatGPT / Codex, "anthropic" → Claude and Anthropic API, "copilot", "aws" → AWS Bedrock, "nuvem"/"cloud" → the cloud category, "api key" → the API-key method, accents ("alibabá"), a typo-free empty result, and stable order.

### RF3 — Desktop catalog picker (high priority)

- **RF3.1** The `<select>` in `account-dialog.js` is replaced by a search field (DS-CMP-SRCH) over a grid of provider tiles (DS-ID-3) at 560 px and above or a list below (DS-CMP-CAT-1), each with the display name, a muted subtitle naming the sign-in method, the one-line hint, the level chip and an outlined "Connect" button (DS-CMP-LIST-6); **sign-in methods as section labels** when the query is empty (categories are no longer section labels); providers billed per query carry the "Billed per query" warning chip (DS-CMP-BADGE-2) in the row and on the account afterwards.
- **RF3.2** Keyboard: typing focuses the search, arrows move between results, `Enter` chooses, `Esc` clears then closes; results announce their count politely to screen readers.
- **RF3.3** Editing an existing account keeps the provider locked (today's `disabled` select) and shows the entry as a read-only header.
- **RF3.4** Detected local sources (RF6) show a "Found on this device" chip on their tile (DS-CMP-CAT-2); it never implies a working account.
- **RF3.5** Empty state: "No provider matches '{query}'" with "Request a provider", opening the new-provider issue template through the allow-listed `open_external`.
- **Acceptance criterion:** smoke in the four languages and reference themes; keyboard-only selection; search results identical to the core's for the RF2 table.

### RF4 — TUI type-to-filter (medium priority)

- **RF4.1** The wizard's provider list filters as the user types, using `account_edit::search`; `Backspace` edits, `Esc` clears; the hint line shows under the highlighted entry; with an empty filter the list shows the sign-in method groups as headings.
- **Acceptance criterion:** `TestBackend` tests for typing, clearing and choosing; narrow-terminal rendering.

### RF5 — Recommended connection mode (medium priority)

- **RF5.1** Choosing a provider opens its `recommended` mode directly; the other modes appear under "Other ways to connect" (a disclosure), each with its hint. The recommendation favors, in order: native login, local import, inference or admin key, pasted session.
- **RF5.2** The recommendation is a core property (RF1.1), so the TUI and the desktop agree; the TUI shows the alternatives as a second step.
- **Acceptance criterion:** tests that both editors open the recommended mode and that a saved account's `auth_mode` is unchanged when edited.

### RF6 — Presence-only detection (medium priority)

- **RF6.1** `detect::local_sources(home, os) -> Vec<Detected>` checks only the existence (`symlink_metadata`, no open) of the paths in the RF6.4 table, each mapped to a provider and the flow that can use it. The table is one reviewed list in the core with a public source reference per entry; an entry marked "to verify" ships only after its path is confirmed against the tool's own documentation or source.
- **RF6.2** Detection runs only when the Add account picker opens (desktop) or the wizard starts (TUI), never at startup or in the background, and skips providers that already have an account using that source.
- **RF6.3** `Detected` carries the provider, the source kind and a display hint ("Cursor is installed on this device"), never a path with the user name, a file size, a modification time or any content.
- **RF6.4** **Detection table (decision Q, researched 2026-10-23).** Presence-only; "verified" means the path is stated by the tool's own public documentation or source; "to verify" entries need that confirmation before they ship:

  | Tool | Presence checked (never opened) | Cuotai provider and flow | Evidence | State |
  |---|---|---|---|---|
  | Cursor (app) | `state.vscdb` at the `cursor_import::candidates()` locations | `cursor`, the explicit `cursor_import` step | existing core table | Verified |
  | Claude Code | `~/.claude/` | `claude`, native login | Claude Code documentation (settings directory) | Verified |
  | Codex CLI | `$CODEX_HOME`, else `~/.codex/` | `chat_gpt`, native login | [developers.openai.com/codex/auth](https://developers.openai.com/codex/auth) (`auth.json` under `CODEX_HOME`, or the OS store) | Verified |
  | AWS CLI | `~/.aws/config` | `aws_bedrock`, the profile step | AWS CLI configuration documentation | Verified |
  | Gemini CLI | `~/.gemini/` (`oauth_creds.json` inside) | `gemini_ai` `product = "cli"`, native login | google-gemini/gemini-cli documentation; newer versions may keep tokens in the OS keychain | Verified (directory); recheck the file name at implementation |
  | GitHub Copilot CLI | `~/.copilot/` | `copilot`, native login (the CLI's own token lives in the OS keychain under `copilot-cli` and is never read) | [docs.github.com — Copilot CLI configuration directory](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-config-dir-reference) | Verified |
  | OpenCode | `$XDG_DATA_HOME/opencode/` or `~/.local/share/opencode/` (`auth.json` inside) | `open_code_go`, the key step | [opencode.ai/docs/troubleshooting](https://opencode.ai/docs/troubleshooting) | Verified (path); to verify whether the OpenCode Go key is the one stored there |
  | Cursor CLI (`cursor-agent`) | `~/.cursor/cli-config.json` | `cursor`, native login | Cursor CLI configuration reference (the credential location is not documented) | Verified (config only) |
  | Qwen Code | `~/.qwen/settings.json` | `qwen`, native login — offered only while its access level is not `Unavailable` | [Qwen Code settings](https://qwenlm.github.io/qwen-code-docs/en/users/configuration/settings/) ("User settings: `~/.qwen/settings.json`"), read 2026-10-25 | Verified |
  | Kiro (CLI and IDE) | `$KIRO_HOME` (absolute only), else `~/.kiro/` | `kiro`, its existing login | [kiro.dev/docs/configuration](https://kiro.dev/docs/configuration/) (global scope `~/.kiro/` for the IDE and the CLI; a relative `KIRO_HOME` is ignored), read 2026-10-25. The earlier third-party paths are not checked | Verified |
  | Kimi CLI | `~/.kimi/config.toml` | `kimi`, native login | [Kimi Code CLI config files](https://moonshotai.github.io/kimi-cli/en/configuration/config-files.html) ("The default configuration file is located at `~/.kimi/config.toml`"), read 2026-10-25 | Verified |
  | Grok CLI (Grok Build) | `$GROK_HOME/config.toml`, else `~/.grok/config.toml` | `grok`, native login | [docs.x.ai/build/settings/reference](https://docs.x.ai/build/settings/reference) (`GROK_HOME`, default `~/.grok`), read 2026-10-25 | Verified |
  | Zed | `~/.config/zed/settings.json` (macOS and Linux; no Windows path documented, so none is checked there) | `zed`, native login | [zed.dev/docs/configuring-zed](https://zed.dev/docs/configuring-zed) (sample header `~/.config/zed/settings.json`), read 2026-10-25 | Verified (macOS, Linux) |
  | Amp | `~/.config/amp/` | none (no Cuotai provider yet) | [ampcode.com/docs/cli/settings](https://ampcode.com/docs/cli/settings) | Verified (path); excluded until a provider exists |
  | Antigravity | — | `antigravity` | 2026-10-25: no official statement; third-party guides disagree (`~/.gemini/antigravity-cli/`, `~/.antigravity/`, `~/.gemini/antigravity/`) | To verify (not in the table) |

  Aider is excluded: its configuration only references other providers' keys and is not a credential source. A tool without a Cuotai provider (Amp) is never offered.
- **RF6.5** **Consent-gated (decision Q):** detection itself is opt-in. The first time the picker opens, one line asks "Check this device for AI tools you already use? Cuotai only checks whether their folders exist and reads nothing." (**Check** / **Not now**); the answer is stored as `detect.enabled` in `desktop.json` and can be changed in Settings → Data & privacy; the TUI asks once per session. With no answer or **Not now**, no path is checked. (2026-10-25: the line now also names the environment-variable presence check and the local 9router check; see §12.)
- **Acceptance criterion:** tests with a temporary home directory proving that files are never opened (a `#[cfg(unix)]` test with unreadable `0000` files still detects presence; a test that the function never calls `open`); tests that already-configured sources are skipped; a test that nothing is checked before the RF6.5 consent; a table test that every entry has an evidence reference and that "to verify" entries are disabled; Windows, macOS and Linux path tables.

### RF7 — Consented offer (medium priority)

- **RF7.1** A detected tile offers one action that starts the **existing** explicit flow: Cursor → the `cursor_import` step; Claude, ChatGPT / Codex, Gemini, Copilot, Cursor CLI, Kiro, Qwen → the provider's native login or credential step; OpenCode → the `open_code_go` key step; AWS → the Bedrock profile step (which reads profile names only after the user chooses it, as today). No new import path is created by this PRD.
- **RF7.2** Before anything is read, the flow shows a consent line naming exactly what will be read and why ("Cuotai will read the sign-in token from Cursor's local database once, to query your usage. It is stored in your system's credential vault."); **Cancel** reads nothing.
- **RF7.3** "Don't suggest this again" per source kind, stored as `detect.dismissed` in `desktop.json` (the TUI keeps it per session).
- **Acceptance criterion:** smoke where a detected Cursor tile leads to the import only after confirmation, cancel reads nothing (verified by a file-access probe in tests), and a dismissed source no longer shows the badge.

## 6. Non-functional requirements

- **Privacy:** presence checks only; no content, size or timestamp read during detection; consent before every read; credentials go to the OS vault as today; nothing about detection is logged or exported.
- **Performance:** search answers in under 5 ms for the full catalog; detection completes in under 50 ms and never blocks the dialog (run off the UI thread).
- **Accessibility:** combobox/listbox semantics for the picker; hints are text; badges have accessible names.
- **i18n:** display names, aliases per language where common, hints in pt-BR, en-US, es and it.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| Core API | `account_edit::catalog`, `account_edit::search`, `detect::local_sources` | New |
| `desktop.json` | `detect.enabled`, `detect.dismissed` | Additive |
| `docs/providers.md` | generated catalog table | Drift-tested |
| TOML, `--json`, `export::v1` | none | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | catalog, sign-in methods, hints, aliases, search, recommended mode, detection table |
| Desktop | search field, tile grid, badges, consent step, dismissals |
| TUI | type-to-filter, alternatives step, consent prompt |

## 9. Dependencies and sequencing

RF1 and RF2 first (pure core). RF3 and RF4 next. RF5 changes both editors and is reused by PRD 0025 RF5.4. RF6 and RF7 ship together, since detection without the consented offer has no use. Picker visuals follow PRD 0022.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Detection perceived as snooping | presence only, on demand, explained, dismissible |
| Path tables drift as tools change | one reviewed table with references; missing paths simply detect nothing |
| Aliases collide | tie order by catalog; tests for known collisions |
| Recommendation picks a mode with weaker access | ordering rule documented; the level chip stays visible |

## 11. Order of execution and verification

1. **RF1 and RF2:** catalog and search.
2. **RF3 and RF4:** editors.
3. **RF5:** recommended mode.
4. **RF6 and RF7:** detection and consented offers.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, `node --check`, and a smoke test of the changed interface. The requirement is only delivered when typing "codex" finds ChatGPT / Codex in both editors, every tile shows a correct one-line hint, and no local file is read before the user confirms.

## 12. Open questions

| Question | Recommendation |
|---|---|
| ~~Should the Gemini CLI and AWS entries be in the first detection table~~ | **Decided on 2026-10-23 (decision Q):** yes, plus the other CLIs of RF6.4, presence-only and consent-gated. |
| ~~Group by category~~ | **Decided (design system DS-CMP-LIST-6):** group by sign-in method; categories stay as search terms. |
| Which "to verify" entries to confirm first | Qwen Code and Kiro CLI (existing providers with logins); the rest as their providers gain users. |
| Show the catalog count ("36 providers") in the picker | No; it invites a numbers race (§4). |
| ~~What the one "Check this device" consent must disclose~~ | **Decided on 2026-10-25 (owner, RF6.5):** one consent whose text names the folders, the environment variables (presence only, never values) and the local 9router check (127.0.0.1). The core text is `detect.ask` (`detect::consent_prompt`, English, pt-BR, es, it; vNEXT), shown by the TUI picker; the desktop `dialog.detect.ask` / `settings.detect.help` texts follow in the desktop text lane. |
