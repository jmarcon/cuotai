# PRD 0054 — Desktop lifecycle and refresh resilience

| Field | Value |
|---|---|
| ID | 0054 |
| Status | In progress (RF4.1 delivered; RF4.2–RF4.4 and RF7.3 partial; the rest proposed) |
| Owner | `cuotai-core` (refresh clock, tool resolution, hook claims), desktop shell and Settings |
| Consumers | `apps/desktop`, `crates/cuotai-tui` (`--watch`, `--serve`, hooks), packaged desktop users on macOS, Windows and Linux |
| Depends on | PRD 0032 RF4 (cross-process notification claims), PRD 0033 RF5.3 (metered-query lock), PRD 0051 RF3/RF4 (`desktop.json` schema and writer), PRD 0052 RF2/RF3 (startup errors and diagnostics) |
| Benefits from | PRD 0021 RF5/RF6 (installed-channel and artifact smokes), PRD 0025 RF3 (platform tray behavior), PRD 0031 RF9 (exact local-server port) |
| Extends | PRD 0001 RF6 and PRD 0033 RF5 (resume-aware deadlines), PRD 0005 RF1/RF4 and PRD 0028 RF1/RF2 (CLI-tool availability), PRD 0013 RF3 (cross-process hook claims), PRD 0019 RF5 (no-tray support), PRD 0020 RF1 (main-window geometry) |
| Date | 2026-10-27 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **clock pair** is one wall-clock UTC instant plus one monotonic `Instant` sampled together. A **lifecycle wake** is an OS resume or network-available signal; it is only a scheduling hint, never permission to bypass cadence. Code references are to `origin/main` at `7a1d93c3`.

## Implementation status (2026-10-28)

Written on 2026-10-27 as PRD 0044 by the 2026-10-25 PRD audit against `origin/main` (0.80.0) and renumbered 0054 when it moved onto `integration/next`, whose 0040–0049 are other plans. This table was re-evaluated on 2026-10-28 against `integration/next` at `18009a04`; the §1 code references still point at `origin/main` at `7a1d93c3`, and the current location is named here where it moved.

| RF | Version | Delivery |
|---|---|---|
| RF1 | — | Proposed. No single-instance plugin; every launch runs the full setup. |
| RF2 | — | Proposed. Hooks still use an in-process lock only. Its prerequisite exists: PRD 0032 RF4's `notified.json` ledger claims (`0f778f1d`). |
| RF3 | — | Proposed. `RefreshCycle` still keeps `Instant` deadlines only. RF3.5's prerequisite, the PRD 0033 RF5.3 metered lock (`metered_record.rs`), exists. |
| RF4 | 0.81.0 (RF4.1; RF4.2–RF4.4 partial) | RF4.1 delivered by `778e00d9`: the official autostart plugin pinned, typed `autostart_status`/`autostart_set` commands, OS state as the source of truth, off by default, Settings › General. RF4.2 partial: an `--autostart` launch hides the main window, without the RF5 no-tray override. RF4.3 partial: enabling rewrites the entry; no Repair or stale-target detection. RF4.4 partial: `docs/gui-{macos,windows,linux}.md` updated; `docs/platforms.md` and `docs/releasing.md` not. `1a9b9fd7` (Show in Dock) is related but answers no RF. |
| RF5 | — | Proposed. A tray failure still fails setup; `docs/platforms.md` still says the app aborts. |
| RF6 | — | Proposed. `aws` and `bl` are still spawned by bare name; RF6.4 partial: login validates the profile, queries do not, and the two `bl` validators are separate copies. |
| RF7 | — | Proposed. Only the Settings window keeps geometry. RF7.3 partial: the tray popover is placed per click on the clicked screen in physical pixels, using the popover's own scale factor and no mixed-DPI fixture. |

## 1. Context

- The desktop registers dialog, notification and updater plugins, but no single-instance plugin (`apps/desktop/src-tauri/src/lib.rs:1422-1427`; dependencies at `apps/desktop/src-tauri/Cargo.toml:24-50`). Every successful setup starts cost scanning, hooks, the refresh scheduler, notices, incidents, the local server, widgets and the updater (`lib.rs:1598-1611`). Two processes can therefore duplicate side effects.
- `notified.json` is a shared local-data file (`crates/cuotai-core/src/resets.rs:343-366`) whose current load/save has no process lock (`:368-388`). PRD 0032 RF4 owns notification claiming; this PRD does not define a second notification ledger.
- Desktop hooks and `cuotai --watch --hooks` read the same rules and append to the same run log (`crates/cuotai-core/src/hooks.rs:1049-1103`), but each process independently submits every observed event (`apps/desktop/src-tauri/src/hooks.rs:217-237`; `crates/cuotai-tui/src/watch.rs:72-89`).
- `RefreshCycle` stores only monotonic `Instant` deadlines (`crates/cuotai-core/src/refresh.rs:9-11,37-43`), initialized at `now + interval` (`:129-154`). The desktop checks them every five seconds (`apps/desktop/src-tauri/src/backend.rs:151-157,1912-1969`); the headless core driver is stepped every 250 ms (`crates/cuotai-core/src/headless.rs:25-30,67-76`). No lifecycle or connectivity wake exists.
- Tray construction propagates failure from both icon creation and `TrayIconBuilder::build` (`apps/desktop/src-tauri/src/lib.rs:1526-1559`), and `run` ends in `expect` (`:1614-1615`). `docs/platforms.md:44-54` consequently says a GNOME tray failure aborts the app.
- AWS queries spawn bare `aws` and do not validate the profile before `--profile` (`crates/cuotai-core/src/providers/cloud.rs:555-583,688-710`), while login validates it (`crates/cuotai-core/src/oauth/aws.rs:29-49,83-95`). Alibaba queries and login similarly spawn bare `bl` (`providers/alibaba_plans.rs:1566-1608`; `oauth/bailian.rs:85-118`). A Finder/Dock launch can have no Homebrew directory in `PATH`.
- Only the Settings window saves and restores geometry (`apps/desktop/src-tauri/src/settings_window.rs:113-160,175-193`). The main window always starts at 1200 × 820 (`apps/desktop/src-tauri/tauri.conf.json:11-23`). Existing monitor conversion divides each monitor origin by that monitor's own scale (`settings_window.rs:251-270`), which is ambiguous on a mixed-DPI virtual desktop.
- The official Tauri 2 single-instance and autostart plugins support desktop Tauri 2. Both are `MIT OR Apache-2.0`, already allowed by `deny.toml:10-33`; implementation must still pin them in the desktop lockfile and pass both `cargo deny` graphs.

## 2. Problem

- A second GUI process can send a second notice, execute a hook twice, race state files and contend for the configured local-server port.
- Sleep can leave readings stale for nearly another full interval; reconnecting does not wake an already-due offline account.
- A tray-dependent monitor neither starts at login nor remains usable when the tray is unavailable.
- GUI launches cannot reliably find provider CLIs that terminal launches find.
- The main window forgets its place, and an unplugged or differently scaled monitor can make restored geometry unreachable.

## 3. Goals

- One desktop process per OS user/session, with deterministic focus behavior and no duplicated startup side effects.
- At-most-once hook execution across the desktop and `--watch --hooks`, without changing event payload schema 1.
- Core-owned, testable resume/reconnect scheduling that preserves every configured and metered cadence.
- Opt-in launch at login and a usable, honest window-only mode without a tray.
- One core resolver for every `aws`/`bl` invocation and visible, redacted diagnostics.
- Main-window geometry that remains reachable across monitor and DPI changes.

## 4. Non-goals

- Making the desktop and `cuotai --serve` one process, forwarding files/URLs/arguments from a second launch, or choosing another server port; PRD 0031 RF9 owns exact-port behavior.
- Replacing PRD 0032 RF4's `notified.json` claim or PRD 0033 RF5.3's metered-query record.
- Guaranteed hook delivery after a claiming process crashes; PRD 0013 still defines hooks as non-durable actions.
- Starting at system boot before login, installing a system service, or enabling launch at login without an explicit switch.
- Persisting tray-popover coordinates; they are derived from the clicked tray icon every time.

## 5. Functional requirements

### RF1 — One desktop instance and one set of side effects (high priority; prerequisite)

- **RF1.1** Add official `tauri-plugin-single-instance = "2"` as the first registered plugin. Its callback discards `argv` and `cwd`, hides the tray popover, then shows, unminimizes and focuses `main` through the same `show_main_window` path used today. It forwards no file, URL, command or environment value.
- **RF1.2** The losing process exits before `setup`: it starts no refresh, notification, hook, updater, widget, cost or local-server task and writes no `desktop.json`, `notified.json` or run log. A callback failure is logged through PRD 0052 RF2 and never starts a second process as a fallback.
- **RF1.3** The guard covers desktop processes only. A concurrent `cuotai --serve` stays valid. If it owns the desktop's configured port, the desktop remains open and shows the existing exact-port conflict from `local_server::State`; it never falls back to another port. Notification de-duplication with CLI processes remains PRD 0032 RF4.
- **Acceptance criterion:** a desktop process integration test starts two packaged binaries against one temporary profile and proves one PID owns all startup counters and the second focuses the first; a real desktop smoke repeats it with the local server enabled and a `cuotai --serve` port conflict.

### RF2 — Cross-process claims for hooks (high priority; side-effect integrity)

- **RF2.1** `cuotai_core::hooks::HookClaims` owns `<OS local data dir>/cuotai/hooks/claims.json` and `claims.lock`. Before enqueueing a real hook, both frontends claim `(config_identity, hooks_digest, rule_name, transition_id)` under the advisory lock, re-read, prune, append and atomically save; the lock is released before a command starts. Files are private (`0700` directory, `0600` files) and retain claims for eight days.
- **RF2.2** `watch::Watcher` returns an internal `ObservedEvent { event, transition_id }`. The ID hashes the stable event kind, account key, metric/window instance and source observation (`Report.checked_at`, or provider-status check time), not `occurred_at`; equal transitions observed seconds apart in two processes therefore match. The ID is never serialized into schema-1 NDJSON.
- **RF2.3** A claim exists per matching rule: two different rules still each run once. The explicit Settings **Test** action bypasses claims and runs on every press. If the claim store or lock is unavailable, hooks fail closed with a localized status/diagnostic and no command runs; no payload, account name, command, URL or token enters the claim file.
- **Acceptance criterion:** a two-process integration test feeds the same checked reading to desktop and `--watch --hooks` and observes one execution per rule, then a later distinct reading executes again; pure tests cover key stability, pruning and privacy, and a CLI smoke proves NDJSON is unchanged.

### RF3 — Resume- and reconnect-aware refresh clocks (high priority; core policy)

- **RF3.1** `cuotai_core::refresh::ClockPair { wall: DateTime<Utc>, monotonic: Instant }` is injected into construction, completion and tick decisions. `RefreshCycle` keeps a wall deadline beside each monotonic deadline, and one helper updates both whenever `new`, `finish_*`, backoff, edit or PRD 0033 `bring_forward` schedules work.
- **RF3.2** `RefreshCycle::observe_clock(sample, expected_tick, signal) -> Vec<usize>` compares elapsed wall and monotonic time. An explicit `Resumed`, or positive wall-minus-monotonic drift greater than `max(2 × expected_tick, 2 s)`, is a suspension. A forward wall adjustment uses the same conservative path; a backward adjustment never postpones a monotonic deadline. Only accounts whose wall deadline elapsed become due, and busy accounts are not duplicated.
- **RF3.3** `LifecycleSignal::NetworkAvailable` wakes the scheduler and makes an account eligible only when its latest failure category is `Offline` **and** its normal wall or monotonic retry deadline elapsed. It never clears authentication/rate-limit backoff and never refreshes an account early merely because connectivity changed.
- **RF3.4** `apps/desktop/src-tauri/src/lifecycle.rs` maps macOS `NSWorkspaceDidWakeNotification`/`NWPathMonitor`, Windows `WM_POWERBROADCAST` (`PBT_APMRESUMEAUTOMATIC`)/Network List Manager, and Linux logind `PrepareForSleep(false)`/NetworkManager `StateChanged` callbacks into a `tokio::sync::Notify`; unavailable Linux D-Bus services degrade to clock-drift polling. TUI and headless loops call `observe_clock` on every existing tick. After wake, due desktop accounts start within one `TICK` (five seconds).
- **RF3.5** Every metered candidate still enters PRD 0033 RF5.3's per-account cross-process lock before a provider call. A resume, forward clock jump or network signal cannot bypass `Provider::METERED_MIN_REFRESH_SECONDS` (900 s), the effective interval, or a record written by another process.
- **Acceptance criterion:** a pure unit test with an injected clock pair advances wall time by eight hours while monotonic time advances one second and proves only elapsed accounts are due on the next tick; tables cover backward/forward jumps, reconnect categories, busy slots and backoff, plus a two-`Monitor` `wiremock` test proves one metered call.

### RF4 — Launch at login, off by default (medium priority; packaged desktop)

- **RF4.1** Add official `tauri-plugin-autostart = "2"` and use its Rust `ManagerExt` behind typed shell commands; no broad webview plugin permission. macOS uses `MacosLauncher::LaunchAgent`, Windows the current-user registry/startup mechanism, and Linux XDG autostart. The OS registration is absent by default and the Settings row reads off; failed enable/disable leaves the prior OS state and returns a localized error.
- **RF4.2** Settings → General adds **Launch Cuotai at login**. `is_enabled` from the OS registration is the displayed source of truth, not a cached boolean. Autostart passes `--autostart`; that launch keeps `main` hidden after successful tray creation, while a manual launch shows it. RF5's no-tray mode always overrides background launch and shows the window.
- **RF4.3** Re-enabling rewrites the registration to the current executable. Direct/AppImage moves that invalidate the target are reported with a **Repair** action; Cuotai never silently re-enables an entry removed in OS settings. No installer or package-manager channel enables the entry. PRD 0021 RF6 artifact smokes verify the installed path and uninstall docs tell users to disable the switch before removal where the OS leaves user login entries.
- **RF4.4** Replace the manual-only guidance in `docs/gui-macos.md:99-101`, `docs/gui-windows.md:93-95` and `docs/gui-linux.md:94-96`; update `docs/platforms.md` and the release checklist with actual per-platform mechanisms and verification level.
- **Acceptance criterion:** a browser-free desktop UI script covers the row, off default, error rollback and Repair state; installed-artifact smokes on macOS, Windows and one GNOME/KDE session enable, log out/in, observe one hidden instance, disable and observe none.

### RF5 — Window-only fallback and visible startup failures (high priority; availability)

- **RF5.1** Split tray initialization from fatal setup. If icon/menu/build fails, continue in `WindowMode::NoTray`: show `main`, keep it in Dock/taskbar, disable tray/popover actions and show a non-dismissible session notice `desktop.tray_unavailable` with platform guidance. Closing the last main window exits instead of hiding an inaccessible process.
- **RF5.2** The main window starts invisible in `tauri.conf.json`; setup restores geometry, decides manual/autostart mode and tray availability, then shows it when required. A no-tray autostart launch shows the window and notice rather than running invisibly.
- **RF5.3** Any genuinely fatal setup/run failure goes through PRD 0052 RF2's crash/startup reporter and native dialog, then exits nonzero; remove the terminal-only `.expect`. The dialog exposes a fixed localized category and diagnostics location, never an error body, URL, path containing a token or provider response.
- **RF5.4** `docs/platforms.md` changes GNOME's behavior from “aborts startup” to the supported fallback and identifies which tray capabilities are unavailable; `docs/gui-linux.md` and `docs/releasing.md` add a real no-AppIndicator smoke.
- **Acceptance criterion:** desktop shell tests inject tray-icon and tray-build failures and assert `WindowMode::NoTray`, visible main window, notice and exit-on-close; an installed Linux smoke without AppIndicator remains usable, and an injected fatal startup failure displays the native PRD 0052 dialog.

### RF6 — Core resolver for CLI-backed providers (high priority; provider availability)

- **RF6.1** `cuotai_core::external_tool::{Tool, Resolver, ResolvedTool}` is the only way to launch `Tool::Aws` (`aws`/`aws.exe`) and `Tool::Bailian` (`bl`/`bl.exe`) for query, model and login paths; login receives the current draft's `cli_path`. Resolution order is: provider `params.cli_path` (absolute regular file; executable bit on Unix and `.exe` on Windows; an invalid explicit path fails without fallback), documented well-known paths, then executable lookup in injected `PATH` directories.
- **RF6.2** Well-known candidates are deterministic and unit-tested: macOS `/opt/homebrew/bin/{tool}` and `/usr/local/bin/{tool}`; Linux `/usr/local/bin/{tool}`, `/usr/bin/{tool}` and `$HOME/.local/bin/{tool}`; Windows `%ProgramFiles%\Amazon\AWSCLIV2\aws.exe` for AWS and `%USERPROFILE%\.local\bin\{tool}.exe` for either tool. Nonexistent candidates are skipped. The resolver accepts injected home/program-files/PATH/filesystem inputs and never shells out to `which`/`where`.
- **RF6.3** A missing tool returns `NeedsConfiguration` with tool name and the configured/well-known candidate list plus the literal word `PATH`, never the environment's PATH value. Settings/details and PRD 0052 RF3 text diagnostics emit `tool.aws: found (well_known, /opt/homebrew/bin/aws)` or `tool.aws: missing`; JSON uses `tools.aws { status, source, path }`. Home prefixes render as `~`, and no full environment is printed.
- **RF6.4** Move AWS profile validation to a shared core validator (1–128 bytes, nonempty, no leading `-`, no control character) called by `Config::validate` and immediately before every query/model/login spawn. Consolidate the existing `bl` profile validators around the current 1–32 ASCII `[A-Za-z0-9_-]`, no-leading-`-` rule. Invalid values never start a process and identify only `params.profile`/`params.cli_profile`.
- **RF6.5** Add `params.cli_path` to the core field descriptors for AWS Bedrock and Alibaba Token Plan, behind “Other ways to connect”; it is a local executable path, never a credential, and is preserved in TOML. Update `docs/providers.md`, `docs/platforms.md` and GUI guides with GUI-launch PATH behavior and the override.
- **Acceptance criterion:** pure resolver/profile table tests cover every precedence, OS path, missing/non-executable file and redaction; fake-process integration tests prove query, model and login use the same resolved binary and invalid AWS profiles never spawn; a Finder/Dock smoke finds Homebrew `aws` with a minimal PATH.

### RF7 — Main-window and popover geometry (medium priority; desktop continuity)

- **RF7.1** Add additive `desktop.json.main_window { x, y, width, height }`. The desktop tracks outer position in the virtual desktop's physical coordinates and inner size in logical pixels, ignores minimized/zero/Windows parking values, debounces writes, and saves on close/exit through PRD 0051 RF4's sole `private_fs::update_json` writer.
- **RF7.2** Restore before first show. Select the monitor by physical title-bar intersection, clamp size to its bounds and keep at least 120 × 24 logical pixels of title bar visible; if no saved monitor remains, center on the primary monitor. Convert size with the selected monitor's scale only—never divide every monitor origin by its own scale as one shared coordinate space.
- **RF7.3** The tray popover stores no geometry. Every click recomputes its physical position from the clicked icon: below a top-half/menu-bar anchor, above a bottom-half/taskbar anchor, centered horizontally and clamped by an 8-logical-pixel margin on that monitor. Existing `popover_origin` behavior (`apps/desktop/src-tauri/src/lib.rs:1093-1173`) remains the basis and gains mixed-scale/negative-origin fixtures.
- **Acceptance criterion:** pure geometry tests cover 1×/2× mixed-DPI monitors, negative origins, unplugged monitors, oversize windows and Windows minimized coordinates; a desktop smoke moves/resizes on a secondary monitor, restarts, unplugs it and confirms a reachable main window while the popover stays on the clicked monitor.

## 6. Non-functional requirements

- **Correctness and cost:** lifecycle hints only accelerate work already due; metered claims remain the final authority before a paid query.
- **Privacy and security:** no claim or diagnostic stores secrets, event payloads, account names, full environments, response bodies, authenticated URLs or tokens; child processes still receive fixed argument vectors without a shell.
- **Availability:** single-instance, tray fallback and autostart are independent—failure of one never disables the others except where explicitly stated.
- **Accessibility and i18n:** persistent notices, Settings rows, errors and diagnostics have catalog ids in every built-in locale; the launch-at-login switch and Repair action are keyboard/screen-reader operable.
- **Compatibility:** use the Tauri 2 plugin line and Rust 1.99.1; both desktop and root dependency graphs must pass `cargo deny --config deny.toml check`.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `desktop.json` | `main_window`; written through PRD 0051 RF3/RF4 | Additive, unknown-key preserving |
| `hooks/claims.json`, `claims.lock` | hashed cross-process hook claims | New private local state |
| Account TOML | optional `params.cli_path` for AWS Bedrock/Alibaba Token Plan | Additive provider parameter; no secret |
| Core interface | `ClockPair`, `LifecycleSignal`, paired deadlines, `external_tool`, internal `ObservedEvent` | New; event JSON unchanged |
| OS user state | opt-in autostart registration | New; off by default |
| CLI, `--json`, `export::v1`, event schema 1 | no serialized change | Compatible |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | clock/drift decision, due eligibility, tool/profile validation, hook transition IDs and claims |
| Desktop shell | plugin registration, focus, lifecycle adapters, autostart commands, tray fallback/error dialog, geometry |
| Desktop UI | launch-at-login/Repair row, persistent no-tray notice, resolved-tool details |
| TUI/CLI | clock-pair stepping, hook claims, shared resolver and diagnostics; no GUI lifecycle policy |
| Docs/release | platform behavior, installed-path/autostart and no-tray smokes |

## 9. Dependencies and sequencing

RF1 lands first. RF2 can follow independently but requires one migrated submission path in both frontends. RF3 lands after PRD 0033 RF5.3 so its metered acceptance is real. RF7 waits for PRD 0051 RF3/RF4. RF5 uses PRD 0052 RF2; RF6's diagnostics use PRD 0052 RF3. RF4 and RF5 must land together before release-channel smokes claim background startup.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Plugin/platform behavior differs by OS | official Tauri 2 plugins, locked versions, installed-artifact smokes |
| A wall-clock jump causes an early query | only elapsed paired deadlines; metered lock is final gate |
| Claim-before-run loses a hook on crash | documented at-most-once trade-off; hooks are not a durable queue |
| Autostart target goes stale after a move | visible Repair action; stable packaged paths; never silent re-enable |
| No-tray process becomes unreachable | visible main window and exit-on-close |
| Geometry conversion moves a window off-screen | physical monitor selection, per-monitor scale, mixed-DPI fixtures |

## 11. Order of execution and verification

1. RF1, then RF2 and their two-process smokes.
2. RF3 after PRD 0033 RF5.3; exercise sleep, reconnect and metered lock.
3. RF6 and PRD 0052 diagnostics.
4. RF7 through the PRD 0051 writer.
5. RF4 and RF5, then docs and packaged-platform smokes.

For each delivery run workspace and desktop fmt, Clippy and tests with `--locked`; `node --check` and the browser-free script for touched JavaScript; both `cargo deny` commands for plugin changes; and the named real surface smoke. The PRD is complete only when a login launch produces one background instance, an eight-hour wake refreshes due non-metered accounts within five seconds without a second metered call, and GNOME without a tray remains usable.

## 12. Open questions

| Question | Recommendation |
|---|---|
| macOS LaunchAgent or direct `SMAppService` | Use the official plugin's LaunchAgent now; reassess `SMAppService` only if notarized-package smoke shows an OS warning. |
| Should network regain clear offline backoff early | No; wake the loop, but only a normally due offline account runs. |
| Should a second launch open Settings when Settings was last active | No; always focus the main window, with no forwarded intent. |
| Should window maximized/full-screen state persist | No in this PRD; persist normal geometry only. |

## 13. Decisions needed from the owner

| Topic | Recommended decision |
|---|---|
| Single-instance mechanism | Official Tauri 2 plugin, not a second core lock protocol. |
| Hook claim semantics | At most once, claim before enqueue; fail closed if claims cannot be persisted. |
| Login launch presentation | Hidden with a working tray; visible in no-tray fallback. |
| Explicit CLI path | One provider parameter, `params.cli_path`, authoritative when set. |

The recommendations above are the implementation defaults unless the owner records a different decision before work starts.

## 14. Integration notes (for 0000_control.md)

**Proposed §2 row:** `| 0054 | [Desktop lifecycle and refresh resilience](0054_desktop-lifecycle-and-refresh-resilience.md) | Proposed | — | 0032 RF4; 0033 RF5.3; 0051 RF3/RF4; 0052 RF2/RF3 | 0021 RF5/RF6; 0025 RF3; 0031 RF9 | Single instance, hook claims, wake recovery, autostart, tray fallback, CLI discovery and visible geometry. |`

**Suggested wave:** 5f, after PRDs 0051 and 0052 and the strong RFs of PRDs 0032/0033.

**Existing PRDs extended:**
- PRD 0001 RF6 and PRD 0033 RF5: add paired-clock resume/reconnect behavior without weakening cadence or the metered lock.
- PRD 0005 RF1/RF4 and PRD 0028 RF1/RF2: add one redacted CLI-tool resolution source to provider availability/details and diagnostics.
- PRD 0013 RF3: add cross-process, per-rule hook claims; its non-durable-delivery non-goal remains.
- PRD 0019 RF5: supersede the “tray failure aborts” support claim with a documented window-only fallback.
- PRD 0020 RF1: add equivalent persistence for the main window and correct mixed-DPI restore; Settings behavior remains.
- PRD 0021 RF5/RF6: add installed-channel autostart/no-tray smoke coverage; channels still never enable autostart.
- PRD 0051 RF3/RF4: register `main_window` and require its core-owned serialized writer; this PRD adds no competing writer.
- PRD 0052 RF2/RF3: consume its native startup-error path and add resolved external-tool diagnostics; crash capture is not duplicated.

**Fix requirement for an existing delivered/proposed plan (decision J):** PRD 0032 RF4.2 must cover re-read/merge under its lock on every `notified.json` write path (record, prune and account retention), including separate desktop/CLI processes. This PRD depends on that correction and does not restate notification-ledger semantics.
