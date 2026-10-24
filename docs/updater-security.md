# Updater security (PRD 0018 RF4)

How the desktop app's in-app updates are signed, who holds the key, what
happens when it leaks, and what happens when it is lost. Read this before
your first signed release. [`docs/releasing.md`](releasing.md) describes the
pipeline and the complete
[`RF9.4 rollback procedure`](releasing.md#pull-back-a-bad-release-rf94).

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
- the manifest endpoint and every selected package URL must use `https`; the
  package URL must match
  `github.com/jmarcon/cuotai/releases/download/<tag>/<asset>` and is checked
  once before the update is shown and again immediately before download;
- certificate validation is never bypassed. The integration fixture drives a
  real TLS handshake with a self-signed certificate and requires an
  untrusted-certificate error;

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
- tag-push desktop build jobs route to the protected `release` environment;
  dispatch builds route to the secretless, unrestricted `rehearsal`
  environment, so the tag-only deployment rule does not block a branch
  rehearsal. No other job (CLI, SBOM, assembly, publish, channel jobs)
  receives the updater key;
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

Create a separate **`rehearsal`** environment with no secrets, reviewers or
deployment-branch rule. The workflow routes `workflow_dispatch` desktop
builds and opt-in channel validation there, while only tag pushes can route
to `release`. GitHub creates `rehearsal` on first use when absent; creating it
explicitly makes its secretless, unprotected contract visible in repository
settings.

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
version that does not exist. Start the
[`RF9.4 release rollback`](releasing.md#pull-back-a-bad-release-rf94)
immediately, but remember that removing a release cannot revoke copies already
downloaded or signatures an attacker can make with the key.

1. **Freeze distribution.** Mark the affected GitHub Release as a pre-release
   or delete its assets, then roll every package channel back by following the
   linked procedure. Never reuse or move the tag.
2. **Generate the replacement pair** on the owner's machine. The command is
   interactive so the password is not placed in shell history; it writes the
   encoded public key beside the private key:

   ```sh
   cd apps/desktop
   cargo tauri signer generate \
     --write-keys ~/.tauri/cuotai-updater-next.key
   chmod 600 ~/.tauri/cuotai-updater-next.key
   cat ~/.tauri/cuotai-updater-next.key.pub
   ```

   Record only the new public key id in the incident log. Keep the new private
   key and its password out of the repository and logs.
3. **Prepare a transition release while the `release` environment still holds
   the OLD private key.** Replace `plugins.updater.pubkey` in
   `apps/desktop/src-tauri/tauri.conf.json` with the one-line contents of
   `cuotai-updater-next.key.pub`, commit that change with the normal version
   bump, and release normally. The pipeline signs the transition artifacts
   with the **old** environment secret. Existing clients embed the old public
   key, so they can verify and install this one release; the newly installed
   binary embeds only the new public key.
4. **Understand the one-key limit.** The pinned
   `tauri-plugin-updater` 2.14.0 cannot embed or try two public keys. This was
   verified against the exact crate source at revision
   `44293a5722efcaf27060504e63b17aeba721b40f`:
   [`Config.pubkey`](https://github.com/tauri-apps/plugins-workspace/blob/44293a5722efcaf27060504e63b17aeba721b40f/plugins/updater/src/config.rs#L115-L116)
   is one `String`, the
   [builder replaces it](https://github.com/tauri-apps/plugins-workspace/blob/44293a5722efcaf27060504e63b17aeba721b40f/plugins/updater/src/lib.rs#L183-L186),
   and
   [`verify_signature`](https://github.com/tauri-apps/plugins-workspace/blob/44293a5722efcaf27060504e63b17aeba721b40f/plugins/updater/src/updater.rs#L1893-L1907)
   decodes one `PublicKey` and invokes it once. Therefore the transition
   binary contains **only the new key**; dual-key overlap is not available or
   needed. Test an installation built with the old public key updating to the
   transition release before revoking the old secret.
5. **Replace the environment secrets only after the transition release is
   public and its update smoke passed.** Enter the same password chosen in
   step 2 when prompted below:

   ```sh
   gh secret set TAURI_SIGNING_PRIVATE_KEY --env release \
     < ~/.tauri/cuotai-updater-next.key
   read -r -s NEW_TAURI_KEY_PASSWORD
   printf '\n'
   printf '%s' "$NEW_TAURI_KEY_PASSWORD" |
     gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --env release
   unset NEW_TAURI_KEY_PASSWORD
   security add-generic-password -U \
     -s cuotai-tauri-updater-key-password -a cuotai-release -w
   gh secret list --env release
   ```

   Replacing the two same-named GitHub environment secrets immediately revokes
   the workflow's access to their old values. GitHub cannot return either
   value. Keep the environment's required reviewer and tag-only rule intact.
6. **Retire the old local secret.** Delete it from the owner's machine and
   every secret backup, then move the replacement to the canonical path:

   ```sh
   rm ~/.tauri/cuotai-updater.key ~/.tauri/cuotai-updater.key.pub
   mv ~/.tauri/cuotai-updater-next.key ~/.tauri/cuotai-updater.key
   mv ~/.tauri/cuotai-updater-next.key.pub ~/.tauri/cuotai-updater.key.pub
   chmod 600 ~/.tauri/cuotai-updater.key
   ```

   Preserve the old **public** key id, incident times and affected versions,
   never the private key itself.
7. **Tell users.** Publish the exposure window and affected versions in the
   release notes and repository discussions. Users who did not install the
   transition release are still pinned to the compromised public key and
   cannot be reached safely by the app; they must install the new release
   manually.
8. **Review what was signed.** Old signatures remain cryptographically valid.
   Replacing the secret prevents official future use but cannot invalidate the
   compromised private key inside old clients. Preserve `SHA256SUMS`,
   provenance attestations and release history as evidence of what the project
   actually shipped.

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
3. Replace both environment secrets as in the leak procedure, step 5.
4. Optional follow-up: ship a second release that shows a persistent notice
   with the manual download link, so the affected users are told in the app
   rather than only in the release notes.

## Rotating the other release credentials

These credentials do **not** use the updater transition mechanism. Freeze tag
releases and complete the
[`RF9.4 rollback`](releasing.md#pull-back-a-bad-release-rf94) first whenever a
compromise may have affected published bytes.

### Checksum signing: Sigstore keyless

There is no long-lived checksum private key or `CHECKSUM_KEY` secret to rotate.
Each tag run obtains a short-lived certificate from GitHub Actions OIDC, and
the verifier binds it to this repository's workflow identity. If that identity
or a GitHub credential is compromised, cancel affected runs, disable the
release workflow while repository access, branch/tag protection and
environment approvals are repaired, and revoke the compromised GitHub
credential. Re-enable releases only after that review:

```sh
gh run cancel RUN_ID --repo jmarcon/cuotai
gh workflow disable release.yml --repo jmarcon/cuotai
# Owner repairs/revokes GitHub access and reviews protection/audit logs here.
gh workflow enable release.yml --repo jmarcon/cuotai
```

A corrected release receives a new ephemeral OIDC certificate; there is no
project key pair to upload. Verify its identity and issuer exactly as usual:

```sh
cosign verify-blob \
  --bundle SHA256SUMS.sigstore.json \
  --certificate-identity-regexp '^https://github\.com/jmarcon/cuotai/\.github/workflows/release\.yml@refs/tags/v' \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  SHA256SUMS
```

### Apple signing and notarization

Apple is the revocation authority. Revoke a leaked Developer ID certificate in
the Apple Developer account, ask Apple/your certificate authority to revoke it
when the portal cannot, generate a new private key and Developer ID
certificate, and export a new password-protected `.p12`. Replace all three
environment secrets together:

```sh
openssl base64 -A -in Cuotai-Developer-ID-new.p12 |
  gh secret set APPLE_CERTIFICATE --env release
gh secret set APPLE_CERTIFICATE_PASSWORD --env release
gh secret set APPLE_SIGNING_IDENTITY --env release
gh secret list --env release
```

If an App Store Connect API key leaked, revoke that key under **Users and
Access → Integrations**, create a new key, and replace `APPLE_API_KEY` (the
base64-encoded `.p8`), `APPLE_API_KEY_ID` and `APPLE_API_ISSUER`. If the Apple
ID notarization path leaked instead, revoke its app-specific password and
replace `APPLE_PASSWORD`; review `APPLE_ID` and `APPLE_TEAM_ID` rather than
rotating identifiers that did not leak. Never retain a revoked private key in
CI or a backup. The first replacement release must pass:

```sh
codesign --verify --deep --strict --verbose=2 Cuotai.app
spctl --assess --type execute --verbose=2 Cuotai.app
xcrun stapler validate Cuotai.app
```

### Windows Authenticode

The certificate issuer is the revocation authority. Report the leaked
certificate and private key to the CA, request revocation for the old
certificate serial, obtain a new code-signing certificate/private key, export
it as a password-protected `.pfx`, and replace both environment secrets:

```sh
openssl base64 -A -in Cuotai-Authenticode-new.pfx |
  gh secret set WINDOWS_CERTIFICATE --env release
gh secret set WINDOWS_CERTIFICATE_PASSWORD --env release
gh secret list --env release
```

There is no old-to-new client transition: Windows validates the certificate
chain and revocation state, not `tauri.conf.json`. Keep the old serial number,
revocation ticket and affected artifact hashes in the incident record, not the
private key. On the first replacement release, verify every executable and
installer in PowerShell:

```powershell
Get-AuthenticodeSignature .\Cuotai.exe
Get-AuthenticodeSignature .\Cuotai_X.Y.Z_x64-setup.exe
Get-AuthenticodeSignature .\Cuotai_X.Y.Z_x64_en-US.msi
```

Each result must name the new certificate and have `Status` equal to `Valid`.

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

The user can turn the feature off entirely (**Settings › About › Updates**), after
which the app makes no updater request at all. The desktop detects Homebrew
Caskroom, the exact Scoop `apps\cuotai` component, winget's package-cache path
and Linux system package roots, and turns the in-app updater off for those
installs. With Tauri's default current-user NSIS mode, a normal winget install
shares `%LocalAppData%\Cuotai` with a manual NSIS install, so it cannot be
classified safely until the packaging contract defines a durable ownership
marker; `CUOTAI_UPDATER=off` is the honest override in the meantime (PRD 0018
RF3.2, PRD 0021 RF8.1).

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