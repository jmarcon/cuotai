# System widgets (PRD 0014)

Cuotai publishes what it already measured into a small JSON file, `widget.json`, that system widgets and third-party integrations read. The extension never queries a provider, never reads a credential and never computes usage: it draws what `cuotai-core` has already computed (the project's rule — see [architecture.md](architecture.md)).

The file is a projection of [`export::v1`](export-v1.md) with **its own** `schema`, so the export can evolve without breaking widgets.

## Where the file lives

| System | Path | Read by |
|---|---|---|
| macOS with the *app group* entitlement | `~/Library/Group Containers/WL9XSC6H36.dev.cuotai.desktop/widget.json` | the app and the signed WidgetKit extension |
| macOS without it | `~/Library/Application Support/cuotai/widget.json` | third-party panels and scripts only |
| Windows | `%LOCALAPPDATA%\cuotai\widget.json` | third-party panels and scripts |
| Linux | `~/.local/share/cuotai/widget.json` | third-party panels and scripts (Waybar, GNOME, KDE) |

The app resolves the *app group* container first and falls back to this user's data directory when the entitlement is missing (an unsigned or unprovisioned build has no container). Settings › Advanced › Integrations › Widgets shows the resolved path and states which of the two it is, so the fallback is never a surprise.

With the feature off, **no file is written and an existing one is deleted** (RF2.2). Settings › Advanced › Integrations › Widgets also has a "Delete widget data" button.

> macOS: reading the file from a WidgetKit extension needs the Developer ID-signed release, whose app and extension both carry the *app group* entitlement `WL9XSC6H36.dev.cuotai.desktop` ([PRD 0019](plans/0019_packaging-and-distribution.md)). Development and ad-hoc builds lack it, so only third-party integrations read their snapshot.

## Widgets

Every image below is drawn from the same example `widget.json` ([`fixtures/widget.json`](../apps/desktop/tools/widget-shots/fixtures/widget.json), `example.com` only, no real account) by the scripts in [`apps/desktop/tools/widget-shots`](../apps/desktop/tools/widget-shots/README.md), which also explain how to regenerate them.

### macOS (WidgetKit)

<p align="center">
  <img src="assets/screenshots/widgets/macos-gallery-light.png" alt="Cuotai macOS widgets, light mode: Summary (large and small), History (medium) and Account (small)" width="100%" />
</p>

<p align="center">
  <img src="assets/screenshots/widgets/macos-gallery-dark.png" alt="Cuotai macOS widgets, dark mode: Summary (large and small), History (medium) and Account (small)" width="100%" />
</p>

> **Rendered, not captured.** These are the shipped SwiftUI entry views (`Sources/*.swift`) drawn off-screen with `ImageRenderer`, because they were rendered before the signed `.appex` existed (see [Building and packaging](#building-and-packaging)). The widget background is approximated with a solid card, and the relative age is frozen at render time ("2 min, 0 secs"); on the desktop the system advances it. The e-mail appears because the example file was written with "include identity" on. The live-widget check on a clean Mac is still pending.

| Widget | Family | Light | Dark |
|---|---|---|---|
| Summary | small | [png](assets/screenshots/widgets/macos-summary-small-light.png) | [png](assets/screenshots/widgets/macos-summary-small-dark.png) |
| Summary | medium | [png](assets/screenshots/widgets/macos-summary-medium-light.png) | [png](assets/screenshots/widgets/macos-summary-medium-dark.png) |
| Summary | large | [png](assets/screenshots/widgets/macos-summary-large-light.png) | [png](assets/screenshots/widgets/macos-summary-large-dark.png) |
| Account | small | [png](assets/screenshots/widgets/macos-account-small-light.png) | [png](assets/screenshots/widgets/macos-account-small-dark.png) |
| Account | medium | [png](assets/screenshots/widgets/macos-account-medium-light.png) | [png](assets/screenshots/widgets/macos-account-medium-dark.png) |
| Account, pinned account removed | small | [png](assets/screenshots/widgets/macos-account-unavailable-light.png) | [png](assets/screenshots/widgets/macos-account-unavailable-dark.png) |
| History | medium | [png](assets/screenshots/widgets/macos-history-medium-light.png) | [png](assets/screenshots/widgets/macos-history-medium-dark.png) |
| Empty state (no snapshot) | small | [png](assets/screenshots/widgets/macos-empty-small-light.png) | [png](assets/screenshots/widgets/macos-empty-small-dark.png) |

The widgets declare no accessory (Lock Screen) family.

### Linux (Waybar)

<p align="center">
  <img src="assets/screenshots/widgets/linux-waybar.png" alt="Waybar with the Cuotai example module: Work 80%, Team 65% (stale), Personal 42%" width="100%" />
</p>

A real screenshot: Waybar running in a headless sway session (in a container) with the example module [`cuotai-waybar.sh`](../apps/desktop/tools/widget-shots/linux/waybar/cuotai-waybar.sh). The module only reads `widget.json` with `jq` and prints Waybar's JSON protocol: the first three accounts as `text`, one line per account as `tooltip`, and a `class` after the most restrictive account (`normal`, `warning` from 75 %, `critical` from 90 %, `exhausted`, `unavailable`, or `off` when the file is absent). To use it, save the script, make it executable and add the module to your Waybar config:

```jsonc
"custom/cuotai": {
    "exec": "~/.config/waybar/cuotai-waybar.sh",
    "return-type": "json",
    "interval": 30
}
```

Read right after the example file was written, the tooltip says:

```text
Work (claude): 80% Week, 2 min ago
Team (copilot): 65% Premium requests, 3 h ago, stale
Personal (codex): 42% Session (5h), 4 min ago
Lab (gemini): unavailable
```

GNOME and KDE: Cuotai ships no GNOME Shell extension or Plasma widget, and neither desktop has a light headless capture path, so there is no image. Panel tools that run a command and show its output can run the same script and use its `text` field.

### Windows

Windows has no Cuotai widget: PRD 0014 delivers no native widget there. The app only writes `%LOCALAPPDATA%\cuotai\widget.json` for third-party tools; at-a-glance usage lives in the notification-area popover (screenshots in [gui-windows.md](gui-windows.md#notification-area)).

## Privacy

The file can be read by any process of the same user and by the extension. Therefore it carries **no credentials, no credential variable names, no provider notes, no response bodies and no authenticated URLs** — notes are not projected at all. The e-mail of an account appears only when Settings › Advanced › Integrations › Widgets has "include identity (e-mail)" turned on, which is off by default. On Unix the file is written `0600` (readable by its owner only); on Windows the per-user data directory provides the equivalent.

The same rules that apply to [`export::v1`](export-v1.md#what-never-appears) apply here, and `widget_snapshot` inherits the redacted fields it projects.

## Format

Produced by `cuotai_core::widget_snapshot`:

```rust
use cuotai_core::widget_snapshot::write;

// `export` is the `export::v1` the front end already builds; `max_accounts`
// is the user's limit from Settings (default 6).
write(&export, std::path::Path::new("widget.json"), 6)?;
```

### Top level

| Field | Type | Meaning |
|---|---|---|
| `schema` | integer | Always `1`. Its own version, independent of `export::v1`. |
| `generated_at` | RFC 3339 | When the snapshot was built. |
| `accounts` | array | The published accounts, in binding order (the most restrictive first). |
| `truncated` | boolean | `true` when accounts were dropped by the user's limit or by the size cap. |

### Account

| Field | Type | Meaning |
|---|---|---|
| `id` | string | The stable account id (PRD 0029), the same `id` as in `export::v1`. A pinned widget matches on it, so a rename keeps the pin; when the account is gone the widget shows it as unavailable and never another account. |
| `key` | string | `<provider slug>:<name>` (`claude:Work`), the same key as in `export::v1`; a rename changes it. Widgets pinned before ids still match on it. |
| `name`, `provider` | string | Display name (the configured name, or the provider's name for an unnamed account) and provider slug. |
| `email` | string | Only when the user turned identity on; absent otherwise. |
| `state` | `ready` / `stale` / `unavailable` | `stale`: the current query failed (or is pending) and the metrics are the preserved reading. `unavailable`: nothing to show, current or preserved. |
| `checked_at` | RFC 3339 or `null` | When the reading shown was **measured**. While stale it is the original time of the preserved reading, never the time the file was written. `null` while `unavailable`. |
| `freshness` | `fresh` / `aging` / `stale` or `null` | Freshness of the reading shown; `stale` for a preserved one. |
| `binding` | object or `null` | The metric that restricts the account most (`index`, `label`, `window`, `used_ratio`, `remaining`, `resets_at`, `reason`, `blocked_until`), same shape as `export::v1`. Recomputed over the preserved metrics while stale, so an old bar can be drawn dimmed. `null` while `unavailable`. |
| `metrics` | array | Up to **4** metrics that have a percentage (the binding one first): `label`, `used_fraction`, `limit`, `unit`, `window`, `resets_at`, and `pace` (PRD 0007) when there is one. A metric without a finite positive `limit` is never published: a percentage is not invented. |
| `overage` | object or absent | Paid extra usage of the current reading (PRD 0023), same shape as `export::v1` (`state`, `spent`, `unit`, `currency`, `cap`, `cycle_ends_at`), so a widget can show "on extra". Absent while stale or unavailable and when the provider exposes no overage data. Overage amounts never appear in `metrics`. |
| `history` | array or absent | Up to **96** points of the binding metric's recent series (`taken_at`, `used_fraction`), present only while this device records history (PRD 0006). The span follows the metric's window. |
| `parent` | string or absent | Only for a connection of an aggregator account (PRD 0036, for example one connection of a 9router instance): the aggregator's key. Each connection is an item of its own (`key` = `<aggregator key>/<sub id>`, `provider` = the mapped provider), so a widget can pin it. |

### What the file never contains

- An absent value stays `null` (or the metric is left out); it never becomes `0`.
- Notes, source URLs, credentials, credential variable names, e-mails (unless identity is on) and response bodies.
- Another account's numbers: an account that failed with no earlier reading is `unavailable` with no metrics at all.
- A pace on preserved data: the pace belongs to the current reading, so a stale account has none.

### Truncation and size

Accounts are sorted by the binding metric — those with a binding first, the most restrictive (highest `used_ratio`) ahead, ties and the rest by `key`, so the order is total and deterministic. The user's limit drops the tail; if the file would still exceed **256 KiB**, accounts keep being dropped from the tail until it fits. Both steps are deterministic: the same state always produces the same file.

### Atomicity

The write is a temporary file in the same directory followed by a `rename`, with `0600` on Unix (enforced after the rename, so a pre-existing looser file does not survive). A reader therefore never sees a half-written file.

## How often it is written

Every state change of the refresh cycle (a cycle, an account, a preference) marks the snapshot dirty; a task of its own does the write. It coalesces the changes inside the window and writes **at most once every 15 seconds**, so a busy refresh never waits for a file write and never produces a write per account. While the switch is off, the task stays quiet and the file stays deleted.

## The macOS WidgetKit extension

Sources live in [`apps/desktop/widgets/macos`](../apps/desktop/widgets/macos):

| File | What it holds |
|---|---|
| `Sources/CuotaiSnapshot.swift` | The file's model and its location (*app group* first, per-user data directory otherwise). |
| `Sources/WidgetViews.swift` | Shared drawing: the age in the system's relative text, dimmed bars for stale data, the empty state. |
| `Sources/SummaryWidget.swift` | Summary: each account's binding metric (small, medium, large). |
| `Sources/AccountWidget.swift` | One account pinned by an App Intent, by its stable id; never falls back to another account. |
| `Sources/HistoryWidget.swift` | An area chart of the binding metric's series, only when the file carries one. |
| `Sources/CuotaiWidgets.swift` | The `WidgetBundle` with the three widgets. |
| `typecheck.sh` | Type-checks the sources with the Command Line Tools. |

### Type-checking without Xcode

The Command Line Tools carry the macOS SDK with WidgetKit, SwiftUI, Charts and AppIntents, so the sources compile without a full Xcode install:

```sh
apps/desktop/widgets/macos/typecheck.sh            # arm64, macOS 14 target
apps/desktop/widgets/macos/typecheck.sh x86_64-apple-macosx14.0
```

Type-checking proves the code compiles; it does not prove the bundle is packaged or signed.

### Building and packaging

Tauri cannot build a Swift app extension, so the release builds it beside the bundle (`desktop-macos` job of `release.yml`):

1. `cargo tauri build` produces `Cuotai.app`.
2. `scripts/release/macos-embed-widgets.sh` runs `apps/desktop/widgets/macos/build-appex.sh`, which compiles the sources for arm64 and x86_64 with `swiftc -application-extension`, joins them with `lipo`, writes `Info.plist` (extension point `com.apple.widgetkit-extension`, identifier `dev.cuotai.desktop.widgets`) and the App Intents metadata the Account picker needs (`appintentsmetadataprocessor`, full Xcode), into `Cuotai.app/Contents/PlugIns/CuotaiWidgets.appex`.
3. `scripts/release/macos-sign-and-verify.sh` signs inside-out: the extension with `CuotaiWidgets.entitlements` (sandbox plus the app group), then the app with `Cuotai.entitlements` (the app group only), both with the hardened runtime, then recreates the DMG. Notarization covers the extension as part of the app.
4. `smoke-desktop-macos.sh` checks that the extension is embedded, signed the way `signing-state.json` records, sandboxed, and (Developer ID) in the app group.

The app group is `WL9XSC6H36.dev.cuotai.desktop`: on macOS a group prefixed with the Team ID needs no provisioning profile and no registration in the Apple portal, unlike the iOS-style `group.` prefix. A full rehearsal has no Developer ID, so it embeds the same extension with an ad-hoc signature and only the sandbox entitlement; such a build falls back to the per-user directory and its widgets show the empty state.

A local build: `VERSION=0.0.0 scripts/release/macos-embed-widgets.sh <path/to/Cuotai.app>`, then sign as step 3.

## For third-party integrations

Read the file, watch its `generated_at` (or use the file's modification time) and respect `state`:

- `ready` — draw the binding metric with `checked_at` as the age;
- `stale` — draw it dimmed and label it as old; the age belongs to `checked_at`, which is the original measurement time;
- `unavailable` — say the account is unavailable; never substitute another account.

The local server ([PRD 0012](plans/0012_local-http-server-and-export-contract.md), [local-server.md](local-server.md)) offers the same state over HTTP for integrations that prefer polling with authentication; the file needs none.