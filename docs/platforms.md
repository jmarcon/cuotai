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
| Tray (desktop) | Yes | Yes | Yes; on GNOME it may need an AppIndicator extension (the `appindicator` library is loaded at run time); without a tray the app runs in its window only |
| Notifications | Desktop: system notification center, with permission prompt and test notification; TUI: header banner + terminal bell, plus OSC 9 only on terminals known to show it | Same as macOS | Same as macOS |
| Native browser login | `open` | `rundll32 url.dll,FileProtocolHandler` | `xdg-open` |
| `aws` CLI (AWS Bedrock) | AWS CLI v2 required for queries and SSO login | Same as macOS | Same as macOS |
| `bl` CLI (Alibaba Token Plan) | Optional: Token Plan usage and console login; `params.session_env` is the documented alternative | Same as macOS | Same as macOS |
| Local cost sources | Claude Code logs (`CLAUDE_CONFIG_DIR`, `~/.config/claude`, `~/.claude`) and Codex CLI logs (`CODEX_HOME`, `~/.codex`) | Same as macOS | Same as macOS |
| Local server (`--serve`, desktop Integration) | Loopback `127.0.0.1` only | Same as macOS | Same as macOS |
| Cursor import | `state.vscdb` under `~/Library/Application Support` | Under `%APPDATA%` | Under `~/.config` |
| Install source (`cuotai --install-source`; shared with desktop updater) | Direct copied app, mounted DMG, App Translocation, Homebrew Caskroom/Cellar, Cargo | Direct/current-user NSIS, exact Scoop component, winget package cache, Cargo | Direct/AppImage, `/usr/{bin,sbin,lib}` package roots, Cargo |

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

### Headless sessions, SSH and WSL (PRD 0052 RF6.3)

| Session | Supported |
|---|---|
| Native macOS, Windows or Linux desktop session with its system vault | Everything: OAuth and pasted sessions in the vault, the desktop app, the TUI and the CLI |
| SSH, headless Linux, or WSL without a D-Bus Secret Service session | Only accounts that read their credential from an environment variable (`credential_env`, `session_env`), and `--serve` with `--token-file`; OAuth sign-in, sessions saved in the vault and the desktop app are unavailable |

There is no plaintext fallback. When the Linux vault is missing, the
`oauth_error.store.linux_secret_service_unavailable` error applies; the
direct remedy on a headless machine is to configure the account with
`credential_env = "VARIABLE"` (see [`cuotai.example.toml`](../cuotai.example.toml))
and export that variable in the shell or service that runs `cuotai`.

## Network: trust store and proxies (PRD 0052 RF6)

Every core client (provider APIs, model lists, status pages, OAuth and the
terminal update check) is built by `cuotai_core::network::builder`:

- **Trust:** only the operating system's root certificates are trusted
  (`rustls-native-certs`: the macOS Keychain, the Windows certificate store,
  the Linux system bundle). Cuotai does not ship or fall back to its own root
  list, so a root your administrator installed (a corporate TLS-inspecting
  proxy, for example) is trusted exactly as the system trusts it. When no
  root can be loaded at all, no secure connection is made and the error
  `network.trust_store_empty` says to repair the system certificates. The
  9router client keeps its own pinned or first-use certificate (PRD 0036).
- **Proxies:** `HTTPS_PROXY`, `HTTP_PROXY` and `ALL_PROXY` (upper or lower
  case) select a proxy and `NO_PROXY` lists hosts that bypass it, with the
  rules of the `reqwest` library. HTTPS traffic goes through the proxy with
  `CONNECT`, so the proxy sees the destination host and port but not the
  request, unless it intercepts TLS with a root the system trusts. Proxy
  credentials in the URL (`http://user:pass@host:port`) are accepted and
  never written to the app log or to diagnostics, which show only the
  variable, the scheme, host and port, and whether a bypass list is set. The
  loopback 9router detection never uses a proxy. Variables are read when a
  client is created: restart Cuotai after changing them.
- **Clock:** the `Date` header of provider, OAuth and status responses is
  compared with the local clock. More than five minutes apart, `--once`
  prints a warning on standard error and the TUI shows it in its warning
  line, advising automatic date and time; two agreeing responses clear it.
  Readings, `--json`, `export::v1`, histories and schedules never change.

## Tray (desktop only)

`apps/desktop/src-tauri/src/lib.rs` builds the tray icon and menu with Tauri.
On GNOME the tray only appears with an AppIndicator extension installed
(e.g. "AppIndicator and KStatusNotifierItem Support"); CI installs no tray
dependency because the library is loaded at run time. If tray creation fails
(PRD 0054 RF5) the app keeps running in its main window only: the window is
shown even for a launch at login, it stays in the Dock or taskbar, closing it
quits the app instead of hiding it, and a banner that cannot be dismissed says
there is no tray icon (with the GNOME extension's name on Linux). Without a
tray there is no popover, no tray menu and no menu-bar title or meter; the
notices, refreshes, hooks, local server and widgets work as usual. A failure
that stops the app altogether is shown in a native dialog with a fixed
category (`startup`) and the app exits with status 1; the error text itself is
never shown. Verified by unit tests of the decisions only; a real desktop
without AppIndicator is a manual release check. The TUI has no tray.

Verification: **code**; real GNOME/KDE session behavior is not exercised in
CI.

### What the menu-bar item shows (PRD 0025 RF3)

`apps/desktop/src-tauri/src/tray_bar.rs` draws the item from the core's
`tray::summary` with the style chosen in Settings › Menu bar. It changes only
when the summary changes, and at most once per second (a burst of changes is
drawn once, after the second).

| Platform | Text styles (Tightest %, Logos + %) | Meters | Icon only | Outage or alert | Screen readers |
|---|---|---|---|---|---|
| macOS | Template image (follows the light and dark menu bar) plus the title as system text | Monochrome template image with up to four bars | Template image | Cut-out notch in the template image (alert: hollow); colored variant: a dot in the level color | The summary is the tooltip and the status item's accessibility label (VoiceOver) |
| Windows | No tray title: the tightest figure (two digits, `!` for an active alert, `-` when unknown) is drawn white on a dark plate in the icon's corner; the full summary is the tooltip | The icon plus the tooltip | The icon | Colored dot | Tooltip (Narrator reads it) |
| Linux, KDE Plasma | Title shown next to the icon (StatusNotifierItem label) | Meter image | The icon | Colored dot | Tooltip |
| Linux, GNOME with the AppIndicator extension | Title shown as the indicator label | Meter image | The icon | Colored dot | Tooltip |
| Linux, other trays (XFCE, Cinnamon, MATE, plain XEmbed) | The label is ignored where the tray does not support it; the full summary stays in the tooltip | Meter image | The icon | Colored dot | Tooltip |

"Use color in the menu bar" (off by default) draws the meters and the mark in
the level colors on every platform; figures, the stale mark and the alert mark
carry the same meaning without color.

Verification: **code** (shell tests map a summary to the title, image and
tooltip of each platform); the light/dark menu-bar, Windows 11 taskbar,
GNOME and KDE checks are owner smokes on real machines.

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

## Launch at login (desktop only)

**Settings › General › Startup › Open at login** (PRD 0054 RF4) uses the
official `tauri-plugin-autostart`, off by default; no installer or package
manager turns it on. The switch always shows what the system reports.

| | macOS | Windows | Linux |
|---|---|---|---|
| Mechanism | `~/Library/LaunchAgents/Cuotai.plist` (LaunchAgent) | `Run` value under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run` | `~/.config/autostart/Cuotai.desktop` (XDG autostart) |
| Launch | `--autostart`: hidden in the menu bar; the window opens when there is no tray | Same, notification area | Same, status notifier |
| Moved app | The entry's program is read back; **Repair** rewrites it | Not read back: turn the switch off and on | The entry's `Exec=` is read back (AppImage path when run from one); **Repair** rewrites it |
| Verification | Unit tests and the browser-free Settings script; log out/in is a manual release check | Same | Same, on one GNOME or KDE session |

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

`cuotai --serve` and the desktop's Settings › Advanced › Integrations run the same
read-only loopback server on `127.0.0.1` with an optional vault-stored
bearer token. No platform restriction; behavior is identical on all three
OSes.

Verification: **CI** on all three OSes.

## Installation source

`cuotai --install-source` reports `source`, `updater_disabled_reason` and
`upgrade_command` as stable `key=value` lines. It reads only the executable
path, the relevant process environment and local directory writability: it
does not load account configuration, query a provider or invoke a package
manager. The desktop updater consumes the same
`cuotai_core::install_source::detect_install_source` result, so a managed copy
cannot be classified differently by the two frontends.

Durable paths identify Homebrew Caskroom/Cellar, Cuotai's exact Scoop
component, winget's package cache, Linux distribution/AUR roots, Cargo's
configured or default `bin` directory, AppImage, mounted DMGs and App
Translocation. A normal winget NSIS install and a manual current-user NSIS
install both use `%LocalAppData%\Cuotai`; neither is called winget without a
future installer-owned marker. Packagers can use `CUOTAI_UPDATER=off` until
that owner-blocked marker exists.

Verification: **CI** path/boundary tests on all three OSes and an offline CLI
process smoke; live package-manager installations remain **code**-level only.

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
