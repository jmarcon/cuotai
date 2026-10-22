# Platform support

PRD 0019 RF5.1: what works on each operating system, grounded in the code
(`cfg(target_os)` gates and the cited modules). This matrix is reviewed on
every release; see the checklist in [`docs/releasing.md`](releasing.md).

Verification legend: **CI** = exercised on hosted runners of that OS
(fmt/clippy/tests plus the no-network TUI smoke of RF5.2); **code** =
implemented and unit-tested, not yet re-verified on a clean machine for the
current release. No cell is claimed from a real-machine run unless it says
so.

| Feature | macOS | Windows | Linux |
|---|---|---|---|
| Credential vault | Keychain (`keyring` crate; first save may prompt) | Credential Manager | Secret Service (GNOME Keyring or KWallet, with a D-Bus session) |
| Tray (desktop) | Yes | Yes | Yes; on GNOME it may need an AppIndicator extension (the `appindicator` library is loaded at run time) |
| Notifications | Desktop: system notification center, with permission prompt and test notification; TUI: header banner + terminal bell, plus OSC 9 only on terminals known to show it | Same as macOS | Same as macOS |
| Native browser login | `open` | `rundll32 url.dll,FileProtocolHandler` | `xdg-open` |
| `aws` CLI (AWS Bedrock) | AWS CLI v2 required for queries and SSO login | Same as macOS | Same as macOS |
| `bl` CLI (Alibaba Token Plan) | Optional: Token Plan usage and console login; `params.session_env` is the documented alternative | Same as macOS | Same as macOS |
| Local cost sources | Claude Code logs (`CLAUDE_CONFIG_DIR`, `~/.config/claude`, `~/.claude`) and Codex CLI logs (`CODEX_HOME`, `~/.codex`) | Same as macOS | Same as macOS |
| Local server (`--serve`, desktop Integration) | Loopback `127.0.0.1` only | Same as macOS | Same as macOS |
| Cursor import | `state.vscdb` under `~/Library/Application Support` | Under `%APPDATA%` | Under `~/.config` |

## Credential vault

`crates/cuotai-core/src/oauth/store.rs` (`SystemTokenStore`) writes through
the `keyring` crate to the platform-native store: macOS Keychain, Windows
Credential Manager, Linux Secret Service. There is deliberately **no**
plaintext fallback. When the store is unavailable the user gets a
platform-specific, localized error instead of a silent failure:
`oauth_error.store.macos_keychain_unavailable`,
`oauth_error.store.windows_credential_manager_unavailable` or
`oauth_error.store.linux_secret_service_unavailable` (the Linux text names
GNOME Keyring/KWallet and the D-Bus session). The debug-only SQLite store
(`--dev-credentials-sqlite`) is rejected by release builds. The serve token
uses the same vault (`crates/cuotai-core/src/serve/token.rs`,
`SystemTokenVault`).

Verification: **CI** (unit tests) on all three OSes; Keychain/Secret Service
prompt flows are **code**-level only.

## Tray (desktop only)

`apps/desktop/src-tauri/src/lib.rs` builds the tray icon and menu with Tauri.
On GNOME the tray only appears with an AppIndicator extension installed
(e.g. "AppIndicator and KStatusNotifierItem Support"); CI installs no tray
dependency because the library is loaded at run time. If tray creation fails
the desktop app aborts startup with the Tauri error — it never runs without
telling the user. The TUI has no tray.

Verification: **code**; real GNOME/KDE session behavior is not exercised in
CI.

## Notifications

Desktop: `apps/desktop/src-tauri/src/notify.rs` uses
`tauri-plugin-notification`; the permission state is surfaced
(granted/prompt/denied) and Settings offers a test notification
(`send_test_notification`, error `notify_off`) so a refusal is visible, not
silent. TUI: `crates/cuotai-tui/src/notify.rs` renders a header banner and
the terminal bell everywhere, and emits OSC 9 only on terminals known to
display it, so no unknown sequence dirties other screens.

Verification: **CI** (logic tests) on all three OSes; actual notification
center delivery is **code**-level only.

## Native browser login

`cuotai_core::login::open_browser` (`crates/cuotai-core/src/oauth/login.rs`)
opens only plain `https://` URLs, via `open` (macOS), `xdg-open` (Linux) or
`rundll32 url.dll,FileProtocolHandler` (Windows), with a 5 s budget. A
failure produces the `LoginEvent::BrowserUnavailable` event and the
interface shows the URL for manual copy — the login is never lost silently.

Verification: **CI** (fallback logic) on all three OSes.

## Command-line tools (`aws`, `bl`)

- AWS Bedrock requires AWS CLI v2 and an explicitly configured profile
  (`crates/cuotai-core/src/providers/cloud.rs`, `oauth/aws.rs`). A missing
  `aws` binary is a configuration error with an install hint
  (`query.cloud.aws_cli_not_found`, `oauth_error.aws.cli_not_found`), never
  a silent skip. Note: Cost Explorer bills per query.
- Alibaba Token Plan uses the official `bl` CLI for usage
  (`providers/alibaba_plans.rs`) and console login (`oauth/bailian.rs`).
  Missing `bl` is a configuration error naming the `params.session_env`
  alternative (`query.alibaba_plans.cli_not_found`,
  `oauth_error.bailian.cli_not_found`).

Verification: **CI** (fake-CLI fixture tests, `#[cfg(unix)]` for shell fakes)
plus error-classification tests on all three OSes.

## Local cost sources

`cuotai-core`'s `cost` module reads Claude Code and Codex CLI session logs
from home-relative directories on all three OSes (opt-in via
`--cost-enable`, after printing exactly what is read). Energy-saving
detection is per-OS: Low Power Mode via `pmset` on macOS, the
`low-power` platform profile on Linux, battery saver on Windows
(`cost/system.rs`); an unknown state counts as "not saving". Background
thread priority uses the Darwin background band (macOS), the lowest nice
value (Linux) or background processing mode (Windows).

Verification: **CI** on all three OSes.

## Local server

`cuotai --serve` and the desktop's Settings → Integration run the same
read-only loopback server on `127.0.0.1` with an optional vault-stored
bearer token. No platform restriction; behavior is identical on all three
OSes.

Verification: **CI** on all three OSes.

## TUI smoke (PRD 0019 RF5.2)

CI builds the TUI binary on Ubuntu, macOS and Windows and runs it without
network access to any provider:

- `cuotai --list-providers` — exits 0;
- `cuotai --config crates/cuotai-tui/tests/fixtures/smoke.toml --once --no-status`
  — the fixture names only environment variables that are never defined, so
  every account is classified as missing configuration before any HTTP
  request and the command exits 2.

The desktop manual verification checklist per release is in
[`docs/releasing.md`](releasing.md).
