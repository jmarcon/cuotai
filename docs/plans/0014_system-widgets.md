# PRD 0014 — System widgets

| Field | Value |
|---|---|
| ID | 0014 |
| Status | In progress (RF1, RF4 and RF5 delivered in 0.71.0; RF2.1/RF2.3 and RF3.1 wait for the Apple developer account; RF5 `widget.json` for Windows/Linux kept, owner decision G of 2026-10-23) |
| Owner | Desktop (macOS) and `cuotai-core` (snapshot file) |
| Consumers | `apps/desktop`, widget extension (Swift), third-party integrations (Waybar, GNOME, KDE) that read the same file |
| Depends on | PRD 0012 (RF1: the `export::v1` contract); PRD 0019 (signing and developer profile); it uses PRDs 0006, 0007, 0009 and 0010 |
| Extended by | PRD 0023 (RF6.2: overage object in the widget snapshot) |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-22)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.71.0 | Implemented — `widget_snapshot::write(export, path)` in the core (RF1.1: key/name/provider, binding metric, up to N eligible metrics with fraction/unit/window/`resets_at`/`checked_at`/freshness/pace, no credentials, notes or e-mail by default; RF1.2: atomic write, `0600`, 256 KiB cap with deterministic truncation, own `schema` field; RF1.3: stale keeps the original `checked_at`, failed-without-reading stays `unavailable`; RF1.4: a decoupled publisher task in the desktop writes after each relevant change, coalesced to one write every 15 seconds). |
| RF2 | 0.71.0 | Partial — RF2.2 implemented (the file is written only with widgets on in Settings → Widgets; turning it off deletes the file). RF2.1 and RF2.3 owner-blocked: the *app group* container `group.dev.cuotai.desktop`, entitlements and the provisioning profile need the Apple developer account (PRD 0019); without it the per-OS path resolution falls back to the documented local-data path. |
| RF3 | 0.71.0 | Partial — RF3.2–RF3.5: the WidgetKit sources in `apps/desktop/widgets/macos` (Summary, Account with an App Intent picker that never shows another account, History) are written and `swiftc -typecheck` passes; they read only the snapshot, show the system relative age, dim stale rows, keep the empty state and carry VoiceOver labels. RF3.1 packaging (`.appex` embedding, `bundle.macOS.files` or a post-processing step) is deliberately unwired: an unsigned extension would fail notarization of the whole app, so it waits for the owner signing prerequisites. Visual acceptance needs a signed build on a clean Mac. |
| RF4 | 0.71.0 | Implemented — RF4.1: Settings → Widgets with the global switch, include-identity (off by default), maximum published accounts (default 6), the file location and "Delete widget data", in pt-BR/en-US/es. RF4.2: the section explains that macOS decides when to redraw and the data may lag. |
| RF5 | 0.71.0 | Implemented — RF5.1: the same snapshot is written to `<local data>/cuotai/widget.json` on Windows and Linux for third-party integrations (Waybar, GNOME, KDE), documented in `docs/export-v1.md` and `docs/widgets.md`. |

Owner prerequisites (PRD 0019): a paid Apple developer account, the registered app group, a provisioning profile with the app-group entitlement, Developer ID signing of the app and the `.appex`, notarization of the bundle containing the extension, and the live-widget visual acceptance on a clean Mac.

## 1. Context

The Cuotai desktop app is a Tauri 2 application (`apps/desktop`, identifier `dev.cuotai.desktop`) with a main window and a tray popover. There is no system widget. The backend keeps the `RefreshCycle` and the state of every account in memory; the webview may be suspended when the window is hidden.

In the 2026-10-19 market analysis, CodexBar publishes 8 types of macOS widgets (WidgetKit): Switcher, Usage, Account Usage, Accounts, History, Metric, Burn Down and combined Burn Down, in small, medium and large. They work like this: the app writes a compact JSON snapshot in the *app group* container and the extension only reads and draws. There are explicit safeguards: never show another account when the chosen one fails, keep the original measurement age on old data and do not invent a percentage when the capacity is unknown. AIQuota and Quotio (iOS) also have widgets.

## 2. Problem

Seeing the quota requires opening the app or the popover. Anyone who keeps the macOS widget panel in view does not find Cuotai there, even though the data already exists in the backend.

## 3. Goals

- Publish Cuotai's state in a stable, small snapshot file, readable by widget extensions and by third-party integrations.
- Deliver macOS widgets (WidgetKit) that only read that file.
- Keep the project's rule: no business logic in the extension; it draws what the core has already computed.

## 4. Non-goals

- Widgets for iOS/iPadOS, Windows and Linux in this version. For those systems, the snapshot file and the PRD 0012 server allow third-party integrations.
- Querying providers from the extension: the extension never touches the network or reads credentials.
- Interactive widgets that change accounts or configuration.

## 5. Functional requirements

### RF1 — Snapshot file for widgets (high priority; prerequisite)

- **RF1.1** In `cuotai-core`, `widget_snapshot::write(export: &Export, path)` writes a compact subset of the `export::v1` contract of PRD 0012: per account, `key`, `name`, `provider`, the binding metric (PRD 0009), up to N eligible metrics with fraction, `unit`, `window`, `resets_at`, `checked_at`, `freshness` and the pace (PRD 0007, when it exists). No credentials, no free-form notes, no e-mail by default.
- **RF1.2** Atomic write (a temporary file and `rename`), `0600` permission, a maximum size of 256 KiB with deterministic truncation (accounts sorted by the binding metric), a versioned `schema` field of its own.
- **RF1.3** Stale data keeps the original `checked_at`; accounts failing with no earlier reading stay as `unavailable`, never with another account's values.
- **RF1.4** The write happens after each relevant change in the `RefreshCycle` (a cycle or an account) and is limited to one write every 15 seconds, in a separate task, so as not to delay the refresh.
- **Acceptance criterion:** tests for atomic writing, the size limit, no secrets, an unavailable account with no borrowed data and rate limiting.

### RF2 — File location and access on macOS (high priority)

- **RF2.1** The file lives in the *app group* container (`group.dev.cuotai.desktop`, name to be confirmed), shared between the app and the extension. The app (not sandboxed) writes; the extension (sandboxed, as WidgetKit requires) reads.
- **RF2.2** That path is only used when widgets are turned on in **Settings → Widgets**; with the feature off, no file is written and any existing one is deleted.
- **RF2.3** Document the need for an Apple developer account, *app group* entitlements and a *provisioning profile* (a PRD 0019 dependency). Without them, the feature appears disabled with an explanation.
- **Acceptance criterion:** a test of the path and of the on/off cycle; manual verification of the entitlements on the signed app.

### RF3 — WidgetKit extension (high priority)

- **RF3.1** A minimal Swift project in `apps/desktop/widgets/macos`, with the widget extension packaged in the bundle (`PlugIns/`). The way to embed the `.appex` in the Tauri package (for example, the `bundle.macOS.files` setting) must be validated at implementation; if unsupported, describe the post-processing step in PRD 0019's pipeline.
- **RF3.2** Initial set of widgets:
  - **Summary**: each account's binding metric (small, medium, large);
  - **Account**: one account pinned by the user's choice (*App Intent*), without falling back to another account if it disappears;
  - **History**: an area chart with PRD 0006's data, only if history is on (medium).
- **RF3.3** Each widget shows the measurement age (native relative text, which the system advances by itself) and the `Stale` state when applicable; dimmed bars for old data; no animation.
- **RF3.4** Texts in the three languages, coming from the snapshot file itself (already localized by the core) or from a minimal Swift catalog for fixed labels. Accessibility: labels readable by VoiceOver and contrast in the light and dark themes.
- **RF3.5** A clear empty state ("Open Cuotai to get started") when there is no file; never sample data disguised as real.
- **Acceptance criterion:** verification on a clean Mac: add the three widgets, change an account in the app, observe the update, close the app (widgets keep the last data with the right age), remove a pinned account (the widget shows unavailable), turn the feature off (widgets show the empty state).

### RF4 — Settings (medium priority)

- **RF4.1** **Settings → Widgets**: a global switch, "include identity (e-mail)" (off by default), "maximum published accounts" (default 6), the file location and a "Delete widget data" button.
- **RF4.2** Updating the widgets depends on the system; the text explains that macOS decides when to redraw and that the displayed data may lag.
- **Acceptance criterion:** a smoke test in three languages; preferences persist in `desktop.json`.

### RF5 — Compatibility outside macOS (low priority)

- **RF5.1** On Windows and Linux, the same `widget_snapshot` can be written to `<local data>/cuotai/widget.json` with `0600`, for third-party integrations (Waybar, GNOME and KDE panels, status bars), documented in `docs/export-v1.md`. No native widget is delivered on those platforms by this PRD.
- **Acceptance criterion:** a test of the file on the three systems; documentation of the format.

## 6. Non-functional requirements

- **Privacy:** the file can be read by any process of the same user (and by the extension); for that reason it contains no credentials, no e-mails by default and no free-form notes.
- **Honesty:** old data keeps its original age and is never presented as current.
- **Energy:** rate-limited writing; no polling in the extension.
- **Main risk:** the combination of Tauri + a Swift extension + *app group* requires a paid Apple account and signing adjustments; validate it in a prototype before committing to the scope.

## 7. Order of execution and verification

1. **RF1:** the snapshot file, contract and secrets tests.
2. **Packaging prototype** (part of RF3.1): prove that the `.appex` survives bundle generation and signing.
3. **RF2 and RF3:** file location and widgets.
4. **RF4 and RF5:** settings and the file for other systems.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when, on a clean macOS with the signed app, the summary widget shows the binding account with the measurement age, keeps showing the last data with the right age with the app closed, never shows another account's data, and turning the feature off deletes the file and empties the widgets.
