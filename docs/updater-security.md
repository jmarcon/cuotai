# Updater security (PRD 0018 RF4)

How the desktop app's in-app updates are signed, who holds the key, what
happens when it leaks, and what happens when it is lost. Read this before
your first signed release; [`docs/releasing.md`](releasing.md) describes the
pipeline that consumes it.

## What the app trusts (RF1.2, RF1.3, RF4.3)

| Piece | Value |
|---|---|
| Public key | committed in `apps/desktop/src-tauri/tauri.conf.json` (`plugins.updater.pubkey`), key id `3C624E8A5AFC4791` |
| Endpoint | `https://github.com/jmarcon/cuotai/releases/latest/download/latest.json` |
| Manifest | one `latest.json` with a `platforms` map covering every updatable target the release produces |

`tauri-plugin-updater` refuses anything it cannot verify:

- the signature in `latest.json` must verify against the embedded public key
  over the downloaded bytes — a modified artifact is rejected before install;
- the announced version must be **strictly greater** than the running one
  (the plugin's default comparator), so a replayed or older manifest is
  ignored;
- the endpoint must be `https`. The plugin validates its configured endpoints
  at startup and refuses a plain-HTTP endpoint in a release build
  (`dangerousInsecureTransportProtocol` is left `false`); a certificate
  failure is an error, never a warning to click past.

Every signature the pipeline writes is bound to the release version in the
minisign trusted comment (`cargo tauri signer sign --app-version`), so a
signature cannot be replayed against a different `version` field in the
manifest.

### One manifest, many platforms

`latest.json` carries a `platforms` map instead of one file per platform and
architecture. `tauri-plugin-updater` looks up `{os}-{arch}-{installer}` first
(`darwin-aarch64-app`, `windows-x86_64-nsis`, `windows-x86_64-msi`,
`linux-x86_64-appimage`, `linux-x86_64-deb`, `linux-x86_64-rpm`, …) and falls
back to the generic `{os}-{arch}` key, so one manifest covers every target and
an architecture whose installer type cannot be identified still gets an
artifact. `xtask updater-manifest` decides which generic key belongs to which
artifact (NSIS before MSI, AppImage before deb/rpm) so the result does not
depend on the order the pipeline passed its arguments.

This satisfies RF1.3's per-platform/architecture requirement with a single
document: a missing platform is **absent from the map**, never fabricated, and
`--require` makes a partial release fail rather than strand users on an
architecture the pipeline stopped serving.

## Who holds the key (RF4.1)

| | Where | Who can read it |
|---|---|---|
| Private key | `~/.tauri/cuotai-updater.key` on the owner's machine; GitHub **environment secret** `TAURI_SIGNING_PRIVATE_KEY` in the `release` environment | the owner, and a workflow job that references the `release` environment |
| Password | macOS Keychain (`cuotai-tauri-updater-key-password`, account `cuotai-release`); environment secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | same |
| Public key | committed in `tauri.conf.json` | everyone |

Pipeline review result (RF4.1 acceptance):

- the private key exists **only** as an environment secret; it is never
  committed, never written to a build log and never echoed. Every script
  checks presence and reports honestly instead of printing the value;
- the secret is scoped to `github.event_name == 'push'`, so a
  `workflow_dispatch` rehearsal and every pull-request run see an empty value;
- it is passed to the four desktop build jobs only, all of which declare
  `environment: release`; no other job (CLI, SBOM, assembly, publish, channel
  jobs) receives it;
- it is used in exactly one place per platform — the updater signature. It is
  never used for code signing, notarization, Authenticode, checksums or the
  GitHub token.

### Environment protection the owner must configure

1. **Settings → Environments → `release`**, with:
   - **Required reviewers** (the owner, or two owners);
   - *Prevent self-review* enabled if more than one person can review;
   - **Deployment branches and tags → only tag references** (`v*`), so a
     branch push can never reach a job holding the key;
   - *Allow administrators to bypass configured protection* **off**.
2. Environment secrets (not repository secrets) for
   `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
3. Environment logs: GitHub records every job that references the environment
   and every reviewer's approval, which is the access log for RF4.1. Review
   it after the first releases.

Commands (documented, **not** run by the pipeline — the owner runs them once,
interactively):

```sh
gh secret set TAURI_SIGNING_PRIVATE_KEY --env release < ~/.tauri/cuotai-updater.key
security find-generic-password -w -s cuotai-tauri-updater-key-password -a cuotai-release \
  | gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --env release
```

Verify afterwards with `gh secret list --env release` (names only; GitHub
never returns a secret value) and confirm the environment shows the required
reviewer.

## If the private key leaks (RF4.2)

Assume any leak is total: a leaked key can sign a convincing update for a
version that does not exist, and users' apps will install it.

1. **Freeze first.** Mark the affected release as a pre-release or delete its
   assets so `/releases/latest/download/latest.json` stops serving the
   compromised artifacts. Never reuse the tag.
2. **Generate the new pair** on the owner's machine:
   `cargo tauri signer generate -w ~/.tauri/cuotai-updater-new.key`. Keep the
   new key id (`RURS7/AmeKSEb0I3y0AS7prBB7lVGkgfUp1rHSTFdVuaer0oz`) in the
   incident notes.
3. **Ship a transition release signed with the OLD key that embeds the NEW
   public key.** Update `plugins.updater.pubkey` in `tauri.conf.json` to the
   new public key and cut a release normally, signing with the *old* private
   key. Installed clients still hold the old public key, so they accept the
   transition release, and that release contains the new public key — after it
   installs, clients trust only the new pair. Do **not** delete the old key
   from the environment before this release is public: the plugin of every
   client still verifying with the old key would reject everything.
4. **Revoke the old secret** after the transition release is published and
   has rolled out: replace `TAURI_SIGNING_PRIVATE_KEY` /
   `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` in the `release` environment with the
   new pair, delete `~/.tauri/cuotai-updater.key` from the owner's machine
   and from every backup, and remove the old key from the incident timeline.
5. **Tell users.** Publish in the release notes and the repository's
   discussions: what happened, the window of exposure, which versions were
   affected, and that they should update now. Users who never received the
   transition release cannot be reached by the app itself — they must
   download the new version manually.
6. **Review what was signed.** Every updater signature made with the leaked
   key is valid forever; the SHA256SUMS and provenance attestations in the
   affected releases remain meaningful for the *other* artifacts, and the
   release history shows what shipped.

## If the private key is lost (RF4.2, no rotation path)

A lost key cannot be recovered: only the holder can sign, and no copy exists
in the repository by design. The consequence is precise:

- an installed app can only verify artifacts whose signature matches the
  public key it has compiled in;
- without the private key, no new artifact can carry that signature;
- therefore **no version after the key's last use can be delivered in place**.
  Users must reinstall manually, and the in-app updater must not pretend
  otherwise: shipping a new public key in a release that cannot itself be
  signed with the old key leaves every existing client unable to install it
  (the plugin rejects the unsigned artifact).

Procedure:

1. `cargo tauri signer generate -w ~/.tauri/cuotai-updater.key` for a new pair.
2. **Do not** cut a signed transition release (there is no old key to sign
   it with). Publish the new public key in `tauri.conf.json` together with a
   release note stating plainly that existing installations cannot update
   themselves and must download the new version (Homebrew, winget, the AUR,
   or the GitHub release page).
3. Replace both environment secrets as in the leak procedure, step 4.
4. Optional follow-up: ship a second release that shows a persistent notice
   with the manual download link, so the affected users are told in the app
   rather than only in the release notes.

## Exactly what an update check sends (privacy)

Checking for updates is a plain `GET` of the endpoint:

```text
GET /jmarcon/cuotai/releases/latest/download/latest.json HTTP/2
Host: github.com
```

There is **no request body, no query string and no header carrying user data**
— no identifier, no account, no machine ID, no usage data, no IP-derived
tracking. The manifest is a static file identical for every user; the app
decides what to download by looking up its own platform key locally, so the
`{os}-{arch}` of the requester is never transmitted. Only when the user
consents to an update is the platform artifact downloaded from
`github.com/jmarcon/cuotai/releases/download/<tag>/<asset>` — the same file a
browser would fetch.

The user can turn the feature off entirely (**Settings → Updates**), after
which the app makes no updater request at all. Installs managed by Homebrew,
winget, Scoop or a distribution package are detected and the in-app updater is
turned off for them, so the package manager and the app never compete
(PRD 0018 RF3.2, PRD 0021 RF8.1).

## What the pipeline does per platform

| Platform | Updater artifact | Signed by |
|---|---|---|
| macOS | `Cuotai.app.tar.gz` of the **notarized** app | `collect-macos.sh`, after notarization, with `cargo tauri signer sign` |
| Windows | `Cuotai_<v>_x64-setup.exe` (NSIS), `Cuotai_<v>_x64_en-US.msi`, `Cuotai_<v>_arm64-setup.exe` | `scripts/release/updater-sign.sh`, after Authenticode |
| Linux | `Cuotai_<v>_amd64.AppImage`, `.deb`, `.rpm` (and the `aarch64` set) | the Tauri bundler, through `tauri.updater.conf.json` |

The signing point differs on purpose: each signature must cover the **final**
bytes of the file a user downloads. macOS notarization and Windows Authenticode
both rewrite the artifact after bundling, so an updater signature computed at
bundle time would not verify — while the Linux packages are not touched after
bundling, so the bundler's signature is already final.

Every artifact above is shipped **uncompressed**: a `.tar.gz` on macOS (which
the plugin extracts with `tar`, not `zip`), and the raw `.exe`, `.msi`,
`.AppImage`, `.deb` and `.rpm` elsewhere. That is what lets the desktop disable
the plugin's `zip` feature, which exists only for the legacy v1 updater's zipped
archives and for compressed AppImages. Leaving it on would pull `zip 4.x` into
the desktop dependency graph, where it carries three extraction advisories
(RUSTSEC-2026-0367, RUSTSEC-2026-0368 and RUSTSEC-2026-0369: SUID/SGID bits
preserved on extraction, and symlinks whose targets escape the destination
directory), and `cargo deny` would fail the CI gate for both manifests. If a
future release ever needs a zipped or compressed AppImage artifact, the
dependency has to be updated to a fixed `zip` first — not re-enabled as is.