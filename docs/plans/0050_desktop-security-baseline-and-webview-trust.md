# PRD 0050 — Desktop security baseline and webview trust

| Field | Value |
|---|---|
| ID | 0050 |
| Status | In progress (RF8.1 delivered on Unix; RF1 partial; the rest proposed) |
| Owner | `cuotai-core` (execution consent, server policy, filesystem and transport invariants), desktop Rust shell (native confirmation and user content), desktop UI, TUI/CLI, `xtask` |
| Consumers | `apps/desktop`, `cuotai --serve`, hook and extension users, user-theme and language-pack authors, security reviewers |
| Depends on | PRD 0012 RF2/RF3/RF5 (local server and desktop controls), PRD 0013 RF3/RF4 (hooks and consent), PRD 0017 RF1/RF3/RF4/RF5 (extensions) |
| Benefits from | PRD 0020 RF2–RF5 (Settings surfaces), PRD 0031 RF6/RF9/RF11 (server contract), PRD 0035 RF11/RF12 (critical translations and user packs) |
| Extends | PRD 0012 RF2.4/RF3/RF5, PRD 0013 RF3/RF4, PRD 0017 RF3/RF4/RF5, PRD 0020 RF2.5/RF3/RF4, PRD 0031 RF6/RF11.2, PRD 0035 RF12.3 |
| Date | 2026-10-27 |

> **Note on terminology:** “RF” (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **native confirmation** is an operating-system dialog created by Rust, outside the webview DOM. A **protected text id** is a catalog key that a user pack cannot replace. Code references are to `origin/main` at `7a1d93c3`.

## Implementation status (2026-10-28)

Written on 2026-10-27 as PRD 0040 by the 2026-10-25 PRD audit against `origin/main` (0.80.0) and renumbered 0050 when it moved onto `integration/next`, whose 0040–0049 are other plans. This table was re-evaluated on 2026-10-28 against `integration/next` at `18009a04`; the §1 code references still point at `origin/main` at `7a1d93c3`, and the current location is named here where it moved.

| RF | Version | Delivery |
|---|---|---|
| RF1 | vNEXT (partial) | RF1.1 partial: `e330ba82` sets a CSP (`apps/desktop/src-tauri/tauri.conf.json`) with no `*`, `unsafe-eval`, `https:`, `data:` or `blob:`, but `connect-src` also allows `'self'` and `frame-ancestors 'none'` is missing. RF1.2 open: the inline first-paint script stays in `index.html`, `settings.html` and `tray.html` (Tauri hashes it); no `js/first-paint.js`. RF1.3 partial: `withGlobalTauri: true` is kept but not yet recorded in `docs/security-baseline.md`. RF1.4 partial: a Rust unit test in `apps/desktop/src-tauri/src/lib.rs` pins the directives; no `cargo xtask security-baseline`, HTML parsing or three-page CSP harness (`e7f9a928` only adapted the dev components page). |
| RF2 | — | Proposed. `confirm_hooks` still takes the webview `digest`; no `NativeConfirmer`; `Hooks::digest` still hashes only the rule strings; `enable_extension` still takes the `Candidate` from the webview. |
| RF3 | — | Proposed. Theme checks are still regex-based in `usercontent.js`; no protected ids. RF3.4 partial: the 256 KiB and 64-file caps exist per folder (`user_content.rs`), but `take(64)` runs before filtering and sorting, symlinks are not refused and excess files are not reported. |
| RF4 | — | Proposed. `ServeConfig::validate` still requires a token only off loopback; `Access-Control-Allow-Origin: *` is still sent. |
| RF5 | — | Proposed. `Bounds` is still one set of limits; only PRD 0031's per-peer `/v1/stats` limit exists. |
| RF6 | — | Proposed. RF6.2 partial: access-log entries already omit the query and headers, but keep the full peer address and the raw path; no `AccessLogMode`, flag or desktop file. |
| RF7 | — | Proposed. Groundwork predates this PRD: most clients set `redirect(Policy::none())` one by one (no shared seam), the dev SQLite store is refused in release builds with tests, `transport::is_secure` has unit tests, one server no-secrets test exists; no registry, sentinel table test or xtask check. |
| RF8 | vNEXT (RF8.1, Unix) | RF8.1 delivered as PRD 0017 FIX-05's completion (`10a0bfc5`): the extensions root and every folder up to it are checked at discovery and by `Approved::verify` before each spawn, with swap tests; the Windows branch waits for the §13 decision. RF8.2 and RF8.3 proposed (`management.rs` compares only account names). |

## 1. Context

- The desktop currently disables CSP and exports the global Tauri API (`apps/desktop/src-tauri/tauri.conf.json:39-42`). The UI depends on it: `lib.js` reads `window.__TAURI__` and routes every command through `T.core.invoke` (`apps/desktop/ui/js/lib.js:55-65`), including hook approval and extension enablement (`:158-161`, `:63-67`).
- User themes are inserted as a `<style>` element after regex checks (`apps/desktop/ui/js/usercontent.js:18-29`, `:41-52`). User language packs accept every string/plural key (`:55-71`) and win the merge over core and built-in catalogs (`apps/desktop/ui/js/i18n.js:88-103`). Rust already caps each file at 256 KiB and each directory scan at 64 entries (`apps/desktop/src-tauri/src/user_content.rs:6-7`, `:35-76`).
- Desktop hook commands are saved, confirmed, enabled and tested through webview-callable commands (`apps/desktop/src-tauri/src/lib.rs:656-700`). `Hooks::digest` hashes the rule strings (`crates/cuotai-core/src/hooks.rs:328-335`), while a bare `command[0]` is resolved only when `Command::new` spawns it through the inherited `PATH` (`:771-784`, `:1363-1381`).
- Extension disclosure already binds an opaque `Candidate` to a core snapshot (`crates/cuotai-core/src/extensions/management.rs:150-188`, `:472-484`), but the desktop sends that candidate directly to `enable_extension` (`apps/desktop/src-tauri/src/lib.rs:69-76`). The disclosure lists the command, directory and requested environment names (`crates/cuotai-core/src/extensions/consent.rs:540-590`).
- The local server permits `allow_browsers = true` without a token because `ServeConfig::validate` requires one only for non-loopback binds (`crates/cuotai-core/src/serve.rs:217-229`). Browser mode answers every origin with `Access-Control-Allow-Origin: *` (`:780-786`). Sixteen five-second connection slots are acquired before headers and a token are read (`:610-680`).
- Existing security barriers have no single owner document: credential URLs pass `transport::is_secure` (`crates/cuotai-core/src/transport.rs:13-24`); monitor clients disable redirects (`crates/cuotai-core/src/lib.rs:261-273`, `:306-317`); the plaintext SQLite store is excluded from release builds (`crates/cuotai-core/src/oauth/store.rs:22-39`) and checks regular-file, no-symlink, `0600` and single-link properties on Unix (`oauth/store/sqlite_dev.rs:14-55`).
- `private_fs` creates `0700` directories and `0600` files and checks Unix ownership (`crates/cuotai-core/src/private_fs.rs:10-53`, `:87-101`), but `writable_by_others` and `open_to_others` always return false outside Unix (`:56-84`). Extension scanning checks the extension directory and descendants, not the extension root or intervening parents (`crates/cuotai-core/src/extensions/tree.rs:91-115`; discovery starts with an unchecked `read_dir(root)` at `extensions.rs:182-205`).

## 2. Problem

- A webview injection has a direct IPC route to operations that persist or execute local commands; a DOM confirmation is not an independent trust boundary.
- User CSS has no parser-defined contract, and a user pack can rewrite the words used for credential warnings and consent.
- Browser access to the local server is all-or-nothing, can be tokenless, and unauthenticated slow clients consume the same scarce slots as legitimate clients.
- Security properties shipped across several releases can regress without one inventory that names their owning tests and platform limits.
- Extension consent does not expose when a requested environment name is another account's credential variable, and the delivered parent-directory promise is not enforced end to end.

## 3. Goals

- Make the Rust shell, not the webview, the final authority for every local-code execution approval.
- Establish an enforceable CSP and parser-defined contracts for themes and user language packs.
- Require authenticated, explicitly origin-scoped browser access and bound work before authentication.
- Turn transport, redirect, secret-store, private-filesystem and redacted-error rules into named regression invariants.
- Close the extension disclosure and filesystem gaps without claiming a full operating-system sandbox.

## 4. Non-goals

- Sandboxing hook or extension network/filesystem access; PRD 0017's residual-risk statement remains.
- Adding a JavaScript package manager, remote assets, a theme marketplace or downloaded language packs.
- Authenticating local-server clients by Unix uid, Windows user token or browser cookies.
- Replacing the OS credential vault or permitting the SQLite development store in a release build.
- Making user themes capable of layout changes or arbitrary component CSS; the contract is theme tokens only.

## 5. Functional requirements

### RF1 — Webview CSP and Tauri bridge inventory (critical priority; first barrier)

- **RF1.1** Set `app.security.csp` in `apps/desktop/src-tauri/tauri.conf.json` to `default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; font-src 'self'; connect-src ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'; frame-ancestors 'none'`. No directive may contain `*`, `unsafe-eval`, `https:`, `data:` or `blob:`; `http:` is forbidden except for the exact Tauri IPC source `http://ipc.localhost`, and both IPC sources are allowed only in `connect-src`.
- **RF1.2** Move the first-paint inline scripts in `index.html`, `settings.html` and `tray.html` to one bundled `js/first-paint.js`, so `script-src` needs no nonce, hash or `unsafe-inline`. `style-src 'unsafe-inline'` remains narrowly necessary for token-only user themes, dynamic bar positions and sizes; RF3 and CSP together constrain it. Provider logos remain bundled files under `ui/img`, so `img-src data:` is not needed and data images are refused.
- **RF1.3** Keep `withGlobalTauri: true` in vNEXT: the unbundled modules currently call `window.__TAURI__.core.invoke` and events directly, and the repository has no package build that can import `@tauri-apps/api`. Record this exception in `docs/security-baseline.md`; removal requires migrating every direct use to an official module bridge, not `__TAURI_INTERNALS__`.
- **RF1.4** `cargo xtask security-baseline` parses the JSON and every desktop HTML file, checks the exact directives, rejects inline scripts/event handlers and forbidden sources, and fails if a new remote URL-bearing CSP source appears. It also loads all three production pages in the browser-free UI harness and treats CSP console violations as failures.
- **Acceptance criterion:** an xtask test rejects fixtures with `csp: null`, `unsafe-eval`, wildcard/data image access or an inline script; a browser-free desktop UI script loads main, tray and Settings with no CSP violation and proves an `https://example.com/pixel` theme URL is blocked.

### RF2 — Rust-owned confirmation for local code execution (critical priority; trust boundary)

- **RF2.1** Use the existing `tauri-plugin-dialog` from Rust (`apps/desktop/src-tauri/Cargo.toml:31`, initialized at `lib.rs:1422-1424`) through one injectable `NativeConfirmer`. The webview supplies neither dialog text nor an “accepted” boolean. Rust builds a localized title/body from core-owned typed data and receives the OS dialog result; Cancel returns a fixed `execution_declined` error with no write, approval, enablement or spawn.
- **RF2.2** `save_hooks` validates drafts in the core, then a native “Save commands” review shows each canonical JSON argv, the private hooks working directory, inherited environment **names** (`PATH`, `HOME`, `LANG`) plus generated `CUOTAI_*` names, events and timeout. Saving never approves. `confirm_hooks` drops its webview `digest` parameter: Rust reloads `hooks.toml`, builds the listing and digest, asks “Approve commands”, then reloads and re-derives both before committing consent.
- **RF2.3** `set_hooks_enabled(true)` asks natively before enabling the current confirmed set; `false` never prompts. `test_hook(name)` asks natively for that rule immediately before its sample spawn. A rule/file change between review and action returns the existing changed-consent error and executes nothing. Concurrent clicks share one Rust-side confirmation transaction and cannot replay an answer.
- **RF2.4** Core hook consent resolves a bare `command[0]` against the exact captured `PATH`, canonicalizes the absolute executable, hashes the regular file, and includes `{resolved_program, sha256}` in the consent digest/listing. A script also pins its resolved shebang interpreter. Every spawn uses the pinned absolute path and rechecks the hashes; a path or byte change suspends the rule and requires fresh consent.
- **RF2.5** Extension enable/re-approve and `test_extension` use the same native boundary. The body comes from `management::Disclosure` and names exact argv, resolved program/interpreter, cwd, requested environment names, interval/timeout and residual network/filesystem powers—never values. After OK, Rust consumes/re-derives the opaque `Candidate`; stale files, digest or generation execute nothing.
- **Acceptance criterion:** pure unit tests with a fake `NativeConfirmer` cover Cancel and file/PATH races for every command above; fixture-process tests prove a swapped PATH executable never runs; a packaged desktop smoke confirms that only the native OS dialog can approve, enable or test a hook and an extension.

### RF3 — User theme and language-pack contract (high priority; defense in depth)

- **RF3.1** Move validation to a core-owned parser used before bytes reach the webview. A theme is UTF-8, has the existing optional `/*! cuotai-theme … */` metadata object, and contains exactly one dark and one light `:root[data-theme="<file-id>"][data-mode="…"]` rule. Only `color-scheme` and the documented `--bg`, `--surface*`, `--border`, `--fg*`, `--accent*`, status, track, typography, radius, shadow, surface, motion, focus and chart custom properties are accepted; all other selectors, properties, `!important`, at-rules, escapes in identifiers and URL tokens are rejected.
- **RF3.2** Values are parsed tokens, not regex-matched text. The grammar permits colors, lengths, system-font lists and the documented gradient/motion functions, but no `url`, `src`, `image`, `image-set`, `cross-fade`, `element`, `paint`, external font or unknown function. CSP remains the authoritative remote-fetch barrier: a parser bug still cannot fetch an HTTP(S), data or blob resource.
- **RF3.3** A language pack remains a flat object of strings or plural-form string objects with `_meta.name`; it may add ordinary UI/core ids. It may not replace protected prefixes `credential_help.`, `oauth_error.`, `edit_error.`, `remove_error.`, `user_session.`, `access.risk.`, `access.credential_hint.`, `hooks.`, `extensions.consent.`, `serve.`, `update.`, `dialog.delete`, or any id containing `.confirm.` or `.consent.`. Protected entries are ignored and reported in `userContent.errors`; the built-in/core fallback supplies their text.
- **RF3.4** Keep the existing 256 KiB per-file and 64-entry caps, but apply the 64 cap independently to `themes/` and `locales/`, count only eligible regular files, refuse symlinks, sort before truncating, and report excess files deterministically. `docs/themes.md` and the locale README publish the grammar, protected-id policy and migration errors.
- **Acceptance criterion:** pure parser tests cover CSS escapes, `image-set`, nested functions, duplicate modes, a scoped valid theme and all protected-id patterns; a browser-free desktop UI script loads a valid theme/pack, preserves built-in consent text and observes no remote request.

### RF4 — Authenticated origin allow-list for browser clients (critical priority; extends PRD 0012/0031)

- **RF4.1** `ServeConfig::validate` returns `TokenRequired` whenever `allow_browsers` is true, including loopback. Desktop cannot turn browser access on or revoke its token while it is on; the inline error points to **Generate token**. CLI/config constructors cannot bypass the core check.
- **RF4.2** Add `allowed_origins: Vec<Origin>` to `ServeConfig` and `desktop.json.serve`. Browser mode requires 1–16 unique exact origins. An origin is `https://host[:port]`, or `http://localhost|loopback-ip[:port]`, with no credentials, path beyond `/`, query, fragment, wildcard or `null`; comparison uses normalized scheme/host/effective port.
- **RF4.3** A request/preflight with an allowed `Origin` receives that exact value plus `Vary: Origin`; a missing/disallowed/multiple Origin receives `403 origin_not_allowed` and no CORS header. Remove `Access-Control-Allow-Origin: *`. `Sec-Fetch-Site` remains an additional signal, never an authorization substitute; non-browser clients without `Origin` keep the PRD 0012 rules.
- **RF4.4** Settings › Advanced › Integrations replaces the broad warning-only browser switch flow with the switch plus an origin editor, core validation and token state. An old `allow_browsers: true` with no list migrates fail-closed to off and shows a one-time “add trusted origins” notice; no origin is guessed.
- **Acceptance criterion:** core server tests cover tokenless refusal, exact/default-port normalization, wildcard/null/multiple/disallowed origins, preflight and `Vary`; a browser-free desktop script proves the UI cannot persist browser mode without a token and a valid origin.

### RF5 — Bounds before authentication (high priority; local DoS hardening)

- **RF5.1** Split `Bounds` into pending-head and authenticated-serving limits: defaults `max_pending_heads = 16`, `max_pending_heads_per_peer = 4`, `head_timeout = 1 s`, `max_connections = 16`, `request_timeout = 5 s`, with validated ranges `4..=128`, `1..=16`, `100 ms..=2 s`, `4..=128`, `1..=30 s`. Header byte/count and URL caps stay 8 KiB/32/2 KiB.
- **RF5.2** Acquire the global and per-peer pending permits at accept, apply `head_timeout` while reading, validate target/Host/Origin/peer/token, then release pending permits before acquiring an authenticated-serving permit and building an export. Rejected, timed-out and oversized heads never call `Source::export` or read history/cost state.
- **RF5.3** Add a pre-auth token bucket per peer (60 completed heads/minute, burst 20; loopback is one peer by design) and a global bucket (240/minute, burst 40). Exhaustion answers `429 rate_limited` with bounded `Retry-After` when a head is available; an incomplete head is simply closed at its deadline. Buckets use an injected monotonic clock and retain at most 256 peers for 10 minutes.
- **Acceptance criterion:** Tokio tests with a paused clock hold every per-peer pending slot yet a different peer remains serviceable; slow heads free slots within one second; 61 rejected tokens do not build an export; bounds and bucket tests are pure/deterministic.

### RF6 — Explicit, privacy-preserving access logs (medium priority)

- **RF6.1** Add `AccessLogMode::{Off, Errors, All}`. `cuotai --serve --access-log off|errors|all` defaults to `errors`; desktop Settings defaults to `off`. `Errors` records status `>= 400`, parse refusal and timeout; `All` also records successful requests. Changing mode is a hot policy update.
- **RF6.2** An entry contains UTC second, peer address class (`loopback` or `non_loopback`, not the address), method, route template (`/v1/accounts/:key`, never the account key), status and elapsed milliseconds. It never contains query, headers, Origin, token, account/provider names, request/response bodies or raw malformed bytes. Timeout is status 408 and unknown method/route is `-`.
- **RF6.3** CLI entries go to stderr and retention belongs to the invoking supervisor. Desktop entries go to `<data>/cuotai/logs/serve-access.log` through `private_fs`, rotate at 1 MiB with one 1 MiB predecessor, and can be erased from the same row; no log is created in `Off` mode.
- **Acceptance criterion:** pure redaction/route-template tests with sentinel e-mail, token, query and account key; server tests for all three modes including parse timeout; a desktop smoke toggles logging, makes one failing request, erases the private file and confirms Off recreates nothing.

### RF7 — Test-owned security invariant registry (high priority; regression gate)

- **RF7.1** Add `docs/security-baseline.md` with columns invariant id, boundary, fail-closed result, owning test/CI command and platforms. At minimum it lists CSP (`SEC-WEBVIEW`), HTTPS-or-loopback transport (`SEC-TRANSPORT`), no redirects (`SEC-REDIRECT`), release refusal of dev SQLite (`SEC-DEV-STORE`), dev-store file checks (`SEC-DEV-FILE`), private filesystem (`SEC-PRIVATE-FS`) and fixed/redacted errors (`SEC-ERRORS`).
- **RF7.2** All credential-bearing provider, OAuth, user-session and updater clients use one core client-builder seam that sets `redirect(Policy::none())`; direct builders outside tests fail an xtask source check. `wiremock` tests prove a 3xx never forwards Authorization/Cookie to a second host and the returned error contains neither URL nor body.
- **RF7.3** CI runs the existing pure `transport::is_secure` matrix plus callsite tests, a release-profile test where `development_store(path)` fails before filesystem access, and debug tests for regular file, no symlink/hardlink and Unix `0600`. `.gitignore` retaining `cuotai-dev.sqlite3` is an xtask assertion.
- **RF7.4** Every security-boundary failure is a stable `Localized` id/fixed fallback or fixed server error code. A table test injects sentinel URL, body, token and path strings into transport, OAuth, storage, private-fs and server failures and proves none appears in `Display`, localization params, logs or serialized responses.
- **RF7.5** `cargo xtask security-baseline` parses the document, requires one existing test marker per invariant and fails on an undocumented marker or a missing platform declaration. It runs in CI on Linux, macOS and Windows; Unix mode/owner cases remain explicitly `#[cfg(unix)]` pending the owner decision in section 13.
- **Acceptance criterion:** xtask fixture tests detect an undocumented invariant and a stale test name; unit/`wiremock`/release-profile suites above pass, and deleting redirect, transport or redaction protection makes its owning test fail.

### RF8 — Extension path and credential-name disclosure gaps (high priority; extends PRD 0017)

- **RF8.1** PRD 0017 FIX-05 remains the owner of filesystem ancestry. Its correction checks the extensions root and every real parent from the candidate directory up to and including that root for foreign ownership or group/other writability on Unix, after canonical containment. Run it during discovery, disclosure/approval and immediately before spawn; a failed recheck revokes/suspends the candidate with a fixed localized reason.
- **RF8.2** `management::disclosure(id, config)` compares requested `env` names with every other configured account's explicit `Account::credential_env` and the conventional `account_edit::default_env` for configured providers, using case-insensitive matching on Windows and exact matching elsewhere. It returns typed `{name, accounts, conventional}` risk rows, never values.
- **RF8.3** The webview and native dialog visually separate those names under “May expose another account credential”; approval remains possible because extensions intentionally consume user-approved environment variables. The PRD 0017 RF3.2 absolute wording is superseded only for names explicitly highlighted and approved; vault/session values are still never injected.
- **Acceptance criterion:** Unix tests reject a writable root and each writable intermediate parent, including a rename/swap before spawn; pure disclosure tests flag explicit/default collisions without values and preserve non-colliding approved env; a browser-free desktop script renders the warning and a native-confirmation fake proves Cancel does not approve.

## 6. Non-functional requirements

- **Fail closed:** missing CSP, dialog failure, stale digest/candidate, invalid Origin, exhausted pre-auth bounds, unreadable permissions and malformed user content disable the action; none silently relaxes a boundary.
- **Architecture:** security decisions, canonical command/origin/theme parsing and error classification live in `cuotai-core`; desktop and TUI/CLI only present typed results and invoke core operations.
- **Privacy:** secrets never enter TOML, snapshots, exports, dialogs or logs; dialogs show environment names only. Missing security values never become zero, empty allow-lists with browser mode, or permissive defaults.
- **Performance:** CSP/contract validation happens at load/change; command hashes at consent and pre-spawn only; request admission does no export allocation before authentication; disabled logging allocates no entry or file.
- **Accessibility/i18n:** native dialogs use core localized text and OS buttons; warnings are not color-only; built-in pt-BR, en-US, es and it catalogs cover every new protected id, with PRD 0035 languages following its review gate.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `tauri.conf.json` / desktop HTML | strict CSP; external first-paint script; global Tauri exception documented | Security tightening; offline assets only |
| Hook consent record | resolved program/interpreter paths and SHA-256 in digest | Existing bare-name approvals require one re-confirmation |
| `desktop.json.serve` | `allowed_origins`, `access_log`; browser mode requires token | Additive read; old broad browser mode migrates off |
| `ServeConfig` / `Bounds` | origin list, log mode, pending/per-peer limits, head timeout and buckets | Rust API change; migrate every constructor |
| User themes/packs | token-only parsed CSS; protected translation ids | Existing nonconforming user files are skipped with a reason |
| Extension disclosure | typed credential-name risks; ancestry recheck | Additive serialized fields; candidate stays opaque |
| `docs/security-baseline.md` | invariant-to-test registry | New generated/checked documentation contract |
| `export::v1`, events, account TOML | no change | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | execution review data, resolved-program pins, theme/pack/origin parsers, server admission/log contracts, invariant tests, extension risk and ancestry rules |
| Desktop Rust | CSP configuration, native `DialogExt` adapter, private access-log file, fail-closed command transactions |
| Desktop UI | origin/log controls and presentation of typed user-content/extension errors; never final consent |
| TUI/CLI | `--access-log`; existing typed hook/extension confirmation consumes the strengthened core listing |
| Docs/xtask | user-content contracts, local-server migration, security-baseline registry and drift checks |

## 9. Dependencies and sequencing

RF1 lands first. RF2 then establishes the execution boundary before RF3 permits user content in the same webview. RF4 precedes RF5/RF6 so admission and logs use the final Origin policy. RF7 codifies each barrier with RF1–RF6. RF8 can run beside RF4–RF6, but its native extension dialog uses RF2. Coordinate server edits with PRD 0031 and protected-id semantics with PRD 0035. Suggested control wave: a security lane in **5b**, before desktop wave 5c and before the rest of PRD 0031 in 5e.

## 10. Risks

| Risk | Mitigation |
|---|---|
| CSP breaks first paint or platform IPC | external boot script, exact Tauri IPC sources and three-page browser-free smoke |
| Native prompts become repetitive | prompt only at save/approve/enable/test boundaries named in RF2; disable/cancel is prompt-free; typed review data is reused |
| Theme contract breaks an advanced custom theme | deterministic reason, documented token migration and bundled example updated in the same delivery |
| A local attacker still shares loopback identity | short per-peer pending cap plus global reserve/rate; document that this is bounded availability, not peer authentication |
| Hashing an updated executable disables automation | explicit “program changed” state and one fresh review; never silently trust the replacement |
| Origin spelling surprises users | canonical parser, displayed normalized origin and exact CORS echo tests |

## 11. Order of execution and verification

1. **RF1:** CSP, external first paint and xtask config/HTML tests.
2. **RF2:** core execution review/pinning, native adapter, all hook/extension callsites.
3. **RF3:** core user-content parser, protected ids, docs and desktop loader cutover.
4. **RF4**, then **RF5** and **RF6:** browser policy, pre-auth admission and access logs.
5. **RF8:** FIX-05 correction, credential-name risk and desktop presentation.
6. **RF7:** registry and CI aggregation, written alongside each preceding step and completed last.

At each step run formatting, workspace and desktop Clippy/tests, touched JavaScript syntax checks, the named xtask tests and the changed-surface smoke. Security-sensitive deliveries also run the repository CodeQL script. Delivery requires the packaged smoke to demonstrate that a webview click without native acceptance cannot execute code and that a disallowed Origin cannot read any response.

## 12. Open questions

| Question | Recommendation |
|---|---|
| Should `withGlobalTauri` be removed now? | No. Keep the audited exception until an official module bridge can replace every direct global call without remote/package-manager runtime code. |
| Should CSP allow `data:` provider images? | No. All current logos are bundled; add a source only with a concrete offline asset test and security review. |
| May user packs translate consent and credential warnings? | No. Protected ids always use reviewed built-in/core text; ordinary product text remains overrideable. |
| Should an env-name collision block extension approval? | Warn prominently but allow explicit native approval; the value is inherited from the user's process, not read from Cuotai's vault. |
| What is the browser access-log default? | Errors for CLI, Off for desktop; All is explicit because polling can produce tens of thousands of lines. |

## 13. Decisions needed from the owner

| Topic | Options | Recommendation |
|---|---|---|
| Windows ACL parity for `private_fs`, extension trees and token/dev files | **A. Parity now:** inspect/create DACLs so only the current user and SYSTEM have the required read/write/traverse rights, reject unverifiable or permissive ACLs, and add Windows tests. **B. Document limitation:** keep the current profile-boundary assumption and mark Windows ACL checks unsupported in the baseline. | **A.** Returning false without inspection makes a stated invariant platform-dependent. This decision gates only RF7's Windows parity and the Windows branch of corrected FIX-05; all fail-closed webview/server work can proceed. |

## 14. Integration notes (for 0000_control.md)

**Proposed §2 row:**

| ID | Plan | Status | Delivered | Depends on (strong) | Depends on (weak) | Notes |
|---|---|---|---|---|---|---|
| 0050 | Desktop security baseline and webview trust | Proposed | — | 0012, 0013, 0017 | 0020, 0031, 0035 | Strict CSP, native code-execution approval, parsed user content, authenticated Origin allow-list, pre-auth bounds, private logs, tested security invariants and extension disclosure hardening. |

**Suggested wave:** 5b security lane, before 5c desktop UI and the remaining 0031 server work in 5e.

**Existing PRDs extended:**

- **PRD 0012 RF2.4/RF3/RF5:** adds mandatory browser tokens, exact Origin allow-list, pre-auth admission bounds and explicit access-log policy; supersedes wildcard CORS.
- **PRD 0013 RF3/RF4:** adds resolved-program/interpreter pinning and a Rust-native confirmation boundary; a webview dialog no longer counts as execution consent.
- **PRD 0017 RF3/RF4/RF5:** adds native desktop approval and credential-env collision disclosure; its “never other accounts' credentials” claim is narrowed only for explicitly disclosed/approved inherited names.
- **PRD 0020 RF2.5/RF3/RF4:** adds origin, logging and user-content error rows while preserving its Settings component/save rules.
- **PRD 0031 RF6/RF11.2:** applies bounded pre-auth work and redacted log templates to every route, not only `/v1/stats`.
- **PRD 0035 RF12.3:** user packs still override ordinary keys, but its override-every-key assumption is superseded for the protected security-id set.

**Fix requirement owned by an existing delivered plan (decision J):** the audit asked to reopen **PRD 0017 FIX-05** rather than assign a new 0050 requirement id to the defect (inspect the extensions root and every parent through the candidate directory at discovery, approval and pre-spawn). Re-evaluated on 2026-10-28: `integration/next` already completed FIX-05 in vNEXT (`10a0bfc5`, recorded in PRD 0017), which delivers RF8.1 on Unix; only the Windows branch waits for the §13 decision.
