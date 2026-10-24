# PRD 0018 — Automatic updates for the desktop app

| Field | Value |
|---|---|
| ID | 0018 |
| Status | In progress (RF1–RF4 delivered in 0.74.0–0.75.1, the shared install-source contract adopted in 0.77.0 and RF2.6's pre-request consent gate delivered in 0.80.0; the acceptance criteria that need a real signed release and native/package-manager runners stay owner-blocked, and normal winget ownership still needs a packaging marker) |
| Owner | Desktop (`apps/desktop`) and the release pipeline |
| Consumers | `apps/desktop/src-tauri`, `apps/desktop/ui` (Settings → Updates and About) |
| Depends on | PRD 0019 (RF2: CI) and PRD 0021 (RF1–RF4, which supersede PRD 0019 RF3: signed packages and artifact hosting) |
| Extended by | PRD 0025 (RF6.5: the first-run step hosts the update question of RF2.6), PRD 0028 (RF8.2 and RF8.3: the core install-source detector delivered in 0.77.0 serves a CLI update notice) |
| Date | 2026-10-19; amended 2026-10-23 (review item UPD-01, owner decision F: RF2.6 "ask before the first check" supersedes the on-by-default part of RF2.1) |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status

| Requirement | Version | State |
|---|---|---|
| RF1 (mechanism and keys) | 0.74.0, 0.75.0 and 0.75.1 | Delivered — in the app in 0.74.0 (`tauri-plugin-updater` 2.14.0 pinned and initialized, the public key committed in `tauri.conf.json`, no capability permission because the web UI only calls shell commands), in the pipeline in 0.75.0 (`xtask updater-manifest` renders `latest.json`), and hardened in 0.75.1 (one artifact carries its platform/checksum proof plus its detached updater proof without duplicate signing records). |
| RF2 (flow in the app) | 0.74.0, 0.75.1 and 0.77.0 | Delivered except the cross-OS acceptance smoke, which needs a signed release (owner). In 0.75.1 cancellation restores the retained update and verified bytes wait for the restart/login gate; in 0.77.0 the desktop consumes the core's typed install-source result without changing its installability, guidance or request policy. |
| RF3 (Settings and About) | 0.74.0, 0.75.1 and 0.77.0 | Delivered; the four-language UI smoke and zero-request test are automated. In 0.75.1 malformed persisted dates, preference re-enabling, Scoop boundaries and Linux guidance were corrected. In 0.77.0 package-manager ownership/reasons and upgrade commands come from the same core function exposed by the offline CLI, with Cargo and Homebrew Cellar included. |
| RF2.6 (ask before the first check) | 0.80.0 | Delivered — a persisted answer gates the scheduler, the manual command and the request boundary in Rust. The one-time dialog discloses the exact request fields and click-only download/install in all four UI languages; its Yes, No and close paths are exercised through the production UI modules. A request-counting backend test proves zero requests while unanswered, after No and under every existing off policy, and one scheduled check after Yes; legacy profiles with an explicit off preference migrate as answered and stay off. |
| RF4 (operational security) | 0.74.0–0.75.1 | Delivered — HTTPS endpoint and real untrusted-certificate coverage in 0.74.0/0.75.1, artifact URLs restricted to this repository's HTTPS release downloads in 0.75.1, and key custody, leak/loss procedures plus the pipeline review in 0.75.0 ([`docs/updater-security.md`](../updater-security.md)). |

Open or owner-blocked, and not claimed as delivered:

- the **production signed release** (the private key lives in the `release`
  GitHub environment and the owner's Keychain; until the owner uploads the
  secrets, no real `latest.json` exists and a live check honestly reports
  `unconfigured`);
- the **RF2 acceptance smoke** — an app installed on an older version updating
  to the new one on macOS, Windows and Linux, with a simulated network and
  signature failure — needs the owner's signing run and native runners;
- the **package-manager installs seen live**: Homebrew, Scoop and Linux
  detection is proven on path fixtures, not real installations; normal winget
  installs share the NSIS path with manual installs, and the repository has no
  durable installer/receipt marker to distinguish ownership safely;
- the **`release` environment protection** (required reviewer, tag-only
  deployment branches) is a repository setting the owner must turn on, as
  documented in [`docs/updater-security.md`](../updater-security.md).

## 1. Context

The desktop app is a Tauri 2 application (`cuotai-desktop` 0.17.2, identifier `dev.cuotai.desktop`) that depends only on `tauri-plugin-dialog`. There is no updater plugin, no `pubkey` in `tauri.conf.json` and no place to fetch new versions from. The app stays resident in the tray, so the user rarely restarts it on their own and can stay for months on an old version, which may depend on provider interfaces that have already changed.

In the 2026-10-19 market analysis: CodexBar uses Sparkle and Homebrew for updates; Quotio updates stable and beta versions through Sparkle, with separate apps for the beta channel; OpenUsage uses signed, notarized Sparkle with an optional beta channel; OpenQuota updates through packages signed with the project's key, independent of the system signature; WhatsMyUsage warns that it cannot update itself when it runs from the install disk image.

## 2. Problem

- Fixing an adapter broken by a provider change only helps those who can update the app.
- Without a trusted channel, the user installs manually and loses continuity.
- A badly done automatic update is a classic compromise vector: it needs a signature, verification and consent.

## 3. Goals

- Check for new versions and install them with the user's consent, through Tauri 2's official mechanism.
- Guarantee that only artifacts signed with the project's key are accepted.
- Keep privacy: the check sends no user data, and the feature can be turned off.

## 4. Non-goals

- Updating the TUI/CLI: it arrives through package managers or download (PRD 0019).
- Silent updating without a notice, usage telemetry or collecting identifiers.
- Automatically rolling back versions (the documentation tells the user to reinstall an earlier version).
- A beta channel in this version (may be a future PRD).

## 5. Functional requirements

### RF1 — Mechanism and keys (high priority; prerequisite)

- **RF1.1** Adopt `tauri-plugin-updater` 2.x, with the matching permission in `capabilities/default.json`, and `bundle.createUpdaterArtifacts` enabled.
- **RF1.2** Generate a Tauri update-signing key pair. The **public** key goes in `tauri.conf.json` (`plugins.updater.pubkey`); the **private** one never enters the repository and lives in a release-pipeline secret (PRD 0019), with a documented rotation and loss procedure.
- **RF1.3** A static update endpoint (one `latest.json` per platform and architecture, hosted alongside the release artifacts), reachable over HTTPS. The plugin only accepts artifacts whose signature matches the embedded public key.
- **RF1.4** Versioning: `latest.json` carries a semantic version, release notes in plain text, a date, a URL and a signature. It only updates to a **greater** version; the same or a lower version is ignored.
- **Acceptance criterion:** an integration test with a local server simulating `latest.json`: a greater version with a valid signature (accepted), an invalid signature (rejected), an equal or lower version (ignored), a malformed manifest (handled error).

### RF2 — Flow in the app (high priority)

- **RF2.1** *Superseded on 2026-10-23 by RF2.6 for the default ("default: on").* Automatic check on opening and then once every 24 hours, only if the user has not turned it off (default: on, with a clear explanation on first run of what is sent). A check fetches the same public release manifest for every user with no request body or query string; the current version, operating system and architecture are compared locally and are not sent, and no user or machine identifier is included.
- **RF2.2** On finding a new version, show a notice in the main window and in the tray menu ("Update 0.18.0 available"), with the release notes (the matching section of `CHANGELOG.md`, PRD 0019 RF7.5) and the **Update now** and **Remind me later** buttons. Nothing downloads or installs without a click.
- **RF2.3** Installation: download, verify the signature, apply and ask for a restart, showing progress and allowing the download to be cancelled. On failure (network, signature, space, permission), show a localized message, keep the current version intact and offer the manual download link.
- **RF2.4** Before restarting, save the necessary state (configuration, preferences). The refresh cycle and logins in progress are interrupted cleanly; a native login in progress blocks the update with a notice.
- **RF2.5** When the app runs from a location that does not allow self-update (a mounted disk image, a read-only folder, an installation managed by a package manager), explain and guide the user instead of failing.
- **RF2.6** **Ask before the first check (2026-10-23, review item UPD-01, decision F).** No update request is made before the user answers one question: "Check for updates automatically?", in a single dialog that states exactly what a check sends (a request for the public release manifest, while the current version, operating system and architecture are compared locally and are not sent) and that nothing downloads or installs without a click. The question lives in the first-run step of PRD 0025 RF6.5 (and, until that step exists, in a one-time dialog on the first launch of the delivering version). **Yes** turns on the RF2.1 schedule; **No** leaves it off (Settings → Updates can change it later); closing without an answer makes no request and asks again on the next launch. Existing profiles that already turned the check off keep it off; profiles that never answered are asked once. A package-managed install (PRD 0021 RF8.1, PRD 0028 RF8.1) is never asked and shows the manager's command. **Acceptance criterion:** a request-counting server sees zero updater requests before the answer, with **No**, after closing the dialog, and on a package-managed install; one check after **Yes**; the dialog in every UI language.
- **Acceptance criterion:** a smoke test on macOS, Windows and Linux (when there is a package) updating from an old version to the new one, with a simulated network and signature failure; the configuration and accounts persist.

### RF3 — Settings and About (medium priority)

- **RF3.1** **Settings → Updates**: a switch to check automatically, a **Check now** button, the date of the last check, the current version and the channel (stable). The About screen shows the version and the state.
- **RF3.2** When the feature is off, the app makes no update request. An installation managed by Homebrew, winget or AUR must be able to turn the internal updater off (an environment variable or a build key), so it does not compete with the manager.
- **RF3.3** Preferences in `desktop.json`, texts in the three languages.
- **Acceptance criterion:** a smoke test in three languages; with the feature off, a network capture shows zero updater requests.

### RF4 — Operational security (high priority)

- **RF4.1** The private signing key is used only in the release pipeline, in a protected environment, with restricted access and use logging; the repository and the CI logs never show it.
- **RF4.2** An incident-response document: what to do if the key leaks (a new key, a transition version signed with the old key that embeds the new one, communication to users).
- **RF4.3** The update endpoint is served only over HTTPS; a certificate failure is an error, never ignored.
- **Acceptance criterion:** a review of the pipeline (PRD 0019) confirming that the private key exists only as a secret; a test that refuses an HTTP endpoint and an invalid certificate.

## 6. Non-functional requirements

- **Privacy:** no persistent identifier, no account or usage data is sent when checking for updates. The documentation lists exactly what the request contains.
- **Trust:** a mandatory signature; update by consent.
- **Compatibility:** accounts, TOML, history and preferences survive the update; schema migrations go forward only.

## 7. Order of execution and verification

1. **RF1:** plugin, keys and a test with a simulated server.
2. **RF2 and RF3:** the flow in the app and settings.
3. **RF4:** procedures and verification of the pipeline, together with PRD 0019.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when an app installed on an earlier version updates to the next one with consent, rejects any package with an invalid signature, preserves all the user's data, and with the feature off makes no update request.

## 8. What 0.74.0–0.77.0 verified, and what it could not

Verified in the repository, on every push:

- the RF1.4 acceptance criterion as an integration test against a local server
  with an ephemeral key pair: a greater version with a valid signature is
  accepted and verifies the package bytes, a signature from another key and a
  tampered package are refused, an equal or lower version is ignored and a
  malformed manifest is a handled error;
- the RF3.2/RF3.3 acceptance criterion: with the feature off — by preference,
  by `CUOTAI_UPDATER=off`, by the `CUOTAI_UPDATER_DEFAULT` build key or by a
  package-manager install — a request-counting server sees **zero** requests,
  including from *Check now*;
- RF2.6: absent, legacy-on and malformed preferences are all unanswered and
  produce zero requests from the scheduler and *Check now*; **No** persists an
  answered off preference with zero requests, **Yes** starts the existing daily
  schedule and produces one request when due, and every environment, build and
  package-manager policy continues to produce zero requests. The production UI
  smoke covers Yes, No and close (which records no answer), renders the dialog
  in all four built-in languages and keeps the question hidden for managed
  installs.
- the 0.77.0 shared-source regression: core path/environment tests cover every
  source and its boundaries, while the desktop view receives the same disable
  reasons, upgrade commands and installability after deleting its private
  classifier; the CLI process smoke is offline and exits 0;
- RF4.3: a non-HTTPS endpoint is refused before any request, the selected
  artifact URL must be an HTTPS release download from this repository both
  when shown and immediately before download, and the TLS fixture presents a
  real self-signed certificate that fails as untrusted;
- `cargo deny` over both manifests, which is why the plugin's `zip` feature is
  off (see [`docs/updater-security.md`](../updater-security.md));
- the four-language smoke of Settings → Updates and the banner, including the
  distinction between a user-disabled (re-enableable) switch and an external
  policy lock;
- the updater's manifest renderer against real ephemeral-key signatures, and
  merge-level release-script fixtures proving Windows/Linux dual proofs keep
  one signing record per artifact and that unsigned rehearsals stay honestly
  unavailable.

Not verified here, and therefore not claimed:

- an installed app on an earlier version downloading and applying a real signed
  release on each of the three operating systems;
- live Homebrew, Scoop, winget or AUR installs; normal winget ownership is not
  implemented because its NSIS path is also used by manual installs and the
  repository has no durable ownership marker;
- the key rotation of the leak procedure, which needs a second key pair and a
  public release.
