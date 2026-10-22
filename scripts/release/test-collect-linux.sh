#!/usr/bin/env bash
# Behavior test for scripts/release/collect-linux.sh (PRD 0021 RF2.5,
# PRD 0018 RF1.1). Build a synthetic `bundle/` directory under a temporary
# root, exports a trivial `VERSION`, runs the collector, and asserts on:
#   * the green path: one final package per extension plus Tauri's
#     linuxdeploy `Cuotai.AppDir` staging directory and an unrelated
#     extension sibling collect the right files into `dist/` with the
#     exact stable names;
#   * zero matching files fail closed;
#   * two final same-extension matches fail closed and list the offenders
#     (a separate case from any staging directory);
#   * a symlink whose own name matches the expected extension is rejected
#     instead of being copied;
#   * the updater paths, as ASSEMBLY contracts and not as fragment shapes: the
#     collected fragment plus a manifest of the same run must merge into a
#     usable updater state through `xtask signing-state`, with one record per
#     artifact (a signed package is never listed twice).
# Dependencies: POSIX `bash`, the GNU coreutils `readlink` the collector uses
# itself, `jq`, and the workspace `xtask` binary for the assembly half.
# Invoked directly as:
#   bash scripts/release/test-collect-linux.sh
# The script exits 0 on success, non-zero on any failed assertion.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COLLECT="${SCRIPT_DIR}/collect-linux.sh"
UPDATER_SIGN="${SCRIPT_DIR}/updater-sign.sh"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
XTASK="${XTASK:-${REPO_ROOT}/target/debug/xtask}"

fail() { echo "FAIL: $*" >&2; exit 1; }
note() { echo "==> $*"; }

setup_workdir() {
  local dest="$1"
  mkdir -p "$dest/dist"
  mkdir -p "$dest/apps/desktop/src-tauri/target/release/bundle"
}

write_pkg() {
  local path="$1" name="$2"
  mkdir -p "$(dirname "$path")"
  printf 'fixture payload for %s\n' "$name" > "$path"
}

green_scenario() {
  local root="$1"
  setup_workdir "$root"
  # Final artefacts (one per extension) plus Tauri's linuxdeploy AppDir
  # staging directory (`Cuotai.AppDir` is the canonical AppImage staging
  # shape) and an unrelated sibling extension to make sure the collector
  # does not blindly take "the first entry".
  local v=0.67.0
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai_${v}_amd64.AppImage" "amd64 AppImage"
  mkdir -p "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai.AppDir/usr/bin"
  echo "linuxdeploy staging payload" > "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai.AppDir/usr/bin/cuotai"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai_${v}_amd64.ignore" "unrelated"

  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/deb/Cuotai_${v}_amd64.deb" "amd64 deb"
  mkdir -p "$root/apps/desktop/src-tauri/target/release/bundle/deb/Cuotai.AppDir/DEBIAN"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/deb/Cuotai_${v}_amd64.deb.swp" "vim swap"

  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/rpm/Cuotai_${v}_amd64.rpm" "amd64 rpm"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/rpm/Cuotai_${v}_amd64.txt" "notes"

  (
    cd "$root"
    export VERSION="$v"
    "$COLLECT" x64
  )

  local expected_pkg base
  for base in "Cuotai_${v}_amd64.AppImage" "Cuotai_${v}_amd64.deb" "Cuotai_${v}_amd64.rpm"; do
    expected_pkg="$root/dist/$base"
    [ -s "$expected_pkg" ] || fail "expected $expected_pkg to be collected"
    cmp -s "$expected_pkg" "$(find "$root" -path "*/bundle/*/$base" -not -type l)" \
      || fail "$expected_pkg contents differ from the bundle source"
  done

  local frag="$root/fragments/linux-x64.json"
  [ -s "$frag" ] || fail "missing fragments/linux-x64.json"
  grep -q "\"name\": *\"Cuotai_${v}_amd64.AppImage\"" "$frag" || fail "AppImage not in fragment"
  grep -q "\"name\": *\"Cuotai_${v}_amd64.deb\"" "$frag" || fail "deb not in fragment"
  grep -q "\"name\": *\"Cuotai_${v}_amd64.rpm\"" "$frag" || fail "rpm not in fragment"
}

zero_scenario() {
  local root="$1"
  setup_workdir "$root"
  # Bundle directory exists but only contains a linuxdeploy AppDir staging
  # tree (which the extension glob correctly ignores because the name has
  # no `.AppImage` suffix) plus non-matching siblings.
  mkdir -p "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai.AppDir/usr/bin"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai.AppDir/usr/bin/cuotai" "linuxdeploy staging payload"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/deb/note.txt" "unrelated"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/rpm/note.txt" "unrelated"

  if (
      cd "$root"
      export VERSION=0.67.0
      "$COLLECT" x64
    ) >/dev/null 2>"$root/.err"; then
    fail "collector accepted zero matching AppImage files"
  fi
  grep -q "expected one AppImage package" "$root/.err" || fail "zero-match error did not name the bundle dir"
}

duplicate_scenario() {
  local root="$1"
  setup_workdir "$root"
  # Two AppImage final artefacts. This is a separate fail-closed case from a
  # staging directory: every entry matches the expected extension and the
  # collector cannot choose one.
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai_0.67.0_amd64.AppImage" "first"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai_0.67.0_amd64-amd64.AppImage" "second"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/deb/Cuotai_0.67.0_amd64.deb" "deb"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/rpm/Cuotai_0.67.0_amd64.rpm" "rpm"

  if (
      cd "$root"
      export VERSION=0.67.0
      "$COLLECT" x64
    ) >/dev/null 2>"$root/.err"; then
    fail "collector accepted multiple AppImage files"
  fi
  grep -q "found 2" "$root/.err" || fail "duplicate-match error did not report the count"
}

symlink_scenario() {
  local root="$1"
  setup_workdir "$root"
  # Another `.AppImage` whose own name ends in the same extension, but it
  # is a symlink. The resolver inside the collector must reject the symlink
  # explicitly so it cannot be a vector for copying arbitrary files. The
  # other bundles produce their real packages so the failure is scoped to
  # the AppImage step.
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/appimage/Cuotai_0.67.0_amd64.AppImage" "real"
  printf 'tainted payload' > "$root/apps/desktop/src-tauri/target/release/bundle/appimage/aliased.bin"
  # Name has the literal `.AppImage` suffix so the `*.AppImage` glob sees
  # it; without the symlink guard the collector would treat it as a
  # duplicate and reject with `found 2`.
  ln -s aliased.bin "$root/apps/desktop/src-tauri/target/release/bundle/appimage/aliased.AppImage"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/deb/Cuotai_0.67.0_amd64.deb" "deb"
  write_pkg "$root/apps/desktop/src-tauri/target/release/bundle/rpm/Cuotai_0.67.0_amd64.rpm" "rpm"

  if (
      cd "$root"
      export VERSION=0.67.0
      "$COLLECT" x64
    ) >/dev/null 2>"$root/.err"; then
    fail "collector accepted a symlinked AppImage"
  fi
  grep -q "not a regular, non-symlink file" "$root/.err" \
    || fail "symlink error did not name the kind of failure"
}

# Every file `xtask release-manifest` needs for the fixture version, so the
# assembly half of the updater scenarios has a complete `manifest.json`.
# `latest.json` is the assembly job's own updater manifest: it is published and
# authenticated through the signed `SHA256SUMS`, never by the updater key.
write_required_dist() { # <root> <version>
  local root="$1" v="$2" dist="$1/dist"
  mkdir -p "$dist"
  local name
  for name in \
    "Cuotai_${v}_universal.dmg" \
    "Cuotai_${v}_x64-setup.exe" \
    "Cuotai_${v}_x64_en-US.msi" \
    "cuotai-${v}-aarch64-apple-darwin.tar.gz" \
    "cuotai-${v}-x86_64-apple-darwin.tar.gz" \
    "cuotai-${v}-x86_64-unknown-linux-gnu.tar.gz" \
    "cuotai-${v}-x86_64-unknown-linux-musl.tar.gz" \
    "cuotai-${v}-x86_64-pc-windows-msvc.zip" \
    sbom.cdx.json
  do
    printf 'payload of %s\n' "$name" > "$dist/$name"
  done
  if [ -n "${WITH_UPDATER_MANIFEST:-}" ]; then
    printf '{"version":"%s","platforms":{}}\n' "$v" > "$dist/latest.json"
  fi
}

# The fragment the assembly job writes when it generated `latest.json`.
assemble_fragment() { # <root> <with manifest: yes|no>
  local root="$1"
  if [ "$2" = yes ]; then
    jq -n '{artifacts: [{name: "latest.json", signed: false,
      method: "checksum-signature", status: "unsigned"}]}' \
      > "$root/fragments/assemble.json"
  else
    printf '{"artifacts": []}\n' > "$root/fragments/assemble.json"
  fi
}

# A fragment for every artifact this scenario does not own, so the merged
# state covers the whole manifest exactly as the other platform jobs do.
# `latest.json` is reported by the assembly job's own fragment, not here.
other_jobs_fragments() { # <root> <files this job owns...>
  local root="$1"
  shift
  local owned=("$@")
  local others=()
  local name owned_name is_owned
  for name in "$root/dist"/*; do
    name="$(basename "$name")"
    if [ "$name" = "latest.json" ]; then
      continue
    fi
    is_owned=false
    for owned_name in "${owned[@]}"; do
      if [ "$name" = "$owned_name" ] || [ "$name" = "${owned_name}.sig" ]; then
        is_owned=true
        break
      fi
    done
    if [ "$is_owned" = false ]; then
      others+=("$name")
    fi
  done
  jq -n --args '{artifacts: ($ARGS.positional | map(
    {name: ., signed: false, method: "none", status: "unsigned"}
  ))}' "${others[@]}" > "$root/fragments/other-jobs.json"
}

# The consumer-visible invariant (PRD 0021 RF3.4, PRD 0018 RF1.1): the
# collected fragment and a manifest of the same run merge into one signing
# state that `xtask` accepts, with one record per artifact.
assembly_contract() { # <root> <version> <checksums signed|unavailable>
  local root="$1" v="$2" checksums="$3"
  local status="dry-run: checksum signature not produced"
  if [ "$checksums" = "signed" ]; then
    status="signed by the GitHub Actions OIDC identity (Sigstore keyless)"
  fi
  ( cd "$root" && "$XTASK" release-manifest \
      --artifacts-dir dist --version "$v" --channel stable --out-dir dist ) >/dev/null
  if ! ( cd "$root" && "$XTASK" signing-state \
      --version "$v" --channel stable \
      --manifest dist/manifest.json --fragments-dir fragments \
      --checksums "$checksums" --checksums-status "$status" \
      --out dist/signing-state.json ) >"$root/.state" 2>&1; then
    cat "$root/.state" >&2
    fail "the collected fragment does not assemble into a signing state"
  fi
  # One record per artifact: a package with two proofs is listed once, and the
  # detached `.sig` is never an artifact of its own.
  jq -e '[.artifacts[].name] as $n | ($n | unique | length) == ($n | length)' \
    "$root/dist/signing-state.json" >/dev/null \
    || fail "the assembled state lists an artifact twice"
  jq -e '[.artifacts[] | select(.name | endswith(".sig"))] | length == 0' \
    "$root/dist/signing-state.json" >/dev/null \
    || fail "a detached signature must be the updater proof of its payload, not its own artifact"
}

updater_signed_scenario() {
  local root="$1"
  setup_workdir "$root"
  local v=0.67.0
  local bundle="$root/apps/desktop/src-tauri/target/release/bundle"
  write_pkg "$bundle/appimage/Cuotai_${v}_amd64.AppImage" "amd64 AppImage"
  write_pkg "$bundle/appimage/Cuotai_${v}_amd64.AppImage.sig" "base64 updater signature"
  write_pkg "$bundle/deb/Cuotai_${v}_amd64.deb" "amd64 deb"
  write_pkg "$bundle/deb/Cuotai_${v}_amd64.deb.sig" "base64 updater signature"
  write_pkg "$bundle/rpm/Cuotai_${v}_amd64.rpm" "amd64 rpm"

  (
    cd "$root"
    export VERSION="$v"
    "$COLLECT" x64
  ) >/dev/null

  # Every signature the bundler produced is collected next to its payload
  # (PRD 0018 RF1.1); an unsigned package brings none.
  cmp -s "$root/dist/Cuotai_${v}_amd64.AppImage.sig" \
    "$bundle/appimage/Cuotai_${v}_amd64.AppImage.sig" \
    || fail "collected AppImage signature differs from the bundle source"
  cmp -s "$root/dist/Cuotai_${v}_amd64.deb.sig" \
    "$bundle/deb/Cuotai_${v}_amd64.deb.sig" \
    || fail "collected deb signature differs from the bundle source"
  if [ -e "$root/dist/Cuotai_${v}_amd64.rpm.sig" ]; then
    fail "an unsigned rpm must not produce a signature in dist/"
  fi

  # The signed run assembles: every artifact has exactly one record and the
  # updater lists the two signed payloads plus their detached signatures.
  WITH_UPDATER_MANIFEST=1 write_required_dist "$root" "$v"
  other_jobs_fragments "$root" \
    "Cuotai_${v}_amd64.AppImage" "Cuotai_${v}_amd64.deb" "Cuotai_${v}_amd64.rpm"
  assemble_fragment "$root" yes
  assembly_contract "$root" "$v" signed
  local state="$root/dist/signing-state.json"
  local want
  want="$(printf '%s\n' \
      "Cuotai_${v}_amd64.AppImage" "Cuotai_${v}_amd64.AppImage.sig" \
      "Cuotai_${v}_amd64.deb" "Cuotai_${v}_amd64.deb.sig" \
    | jq -R -s -c 'split("\n")[:-1] | sort')"
  jq -e '.updater.available' "$state" >/dev/null \
    || fail "the assembled state does not report the updater as available"
  jq -e --argjson want "$want" '.updater.artifacts == $want' "$state" >/dev/null \
    || fail "assembled updater artifacts differ from the signed payloads and their signatures"
  # With the release key the checksum signature resolves too, and the two
  # proofs of each signed package coexist in one record.
  jq -e --arg a "Cuotai_${v}_amd64.AppImage" \
    '.artifacts[] | select(.name == $a) | .signed == true and .method == "checksum-signature" and .updater_signature == "\($a).sig"' \
    "$state" >/dev/null \
    || fail "the signed AppImage does not carry both proofs in one record"
}

updater_unsigned_scenario() {
  local root="$1"
  setup_workdir "$root"
  local v=0.67.0
  local bundle="$root/apps/desktop/src-tauri/target/release/bundle"
  write_pkg "$bundle/appimage/Cuotai_${v}_amd64.AppImage" "amd64 AppImage"
  write_pkg "$bundle/deb/Cuotai_${v}_amd64.deb" "amd64 deb"
  write_pkg "$bundle/rpm/Cuotai_${v}_amd64.rpm" "amd64 rpm"

  (
    cd "$root"
    export VERSION="$v"
    "$COLLECT" x64
  ) >/dev/null

  # No key, no signature: the packages keep their checksum-signature trust
  # and the fragment states why the updater produced nothing.
  local frag="$root/fragments/linux-x64.json"
  jq -e '.updater.available == false' "$frag" >/dev/null \
    || fail "fragment does not report the updater as unavailable"
  jq -e '.updater.reason | length > 0' "$frag" >/dev/null \
    || fail "an unavailable updater must give a reason"
  jq -e '[.artifacts[] | select(.updater_signature != null)] | length == 0' "$frag" >/dev/null \
    || fail "an unsigned run must not claim an updater proof"

  # The dry run assembles too: the updater is honestly unavailable, no
  # `latest.json` was generated, and every payload keeps exactly one
  # checksum-signature record.
  write_required_dist "$root" "$v"
  other_jobs_fragments "$root" \
    "Cuotai_${v}_amd64.AppImage" "Cuotai_${v}_amd64.deb" "Cuotai_${v}_amd64.rpm"
  assemble_fragment "$root" no
  assembly_contract "$root" "$v" unavailable
  local state="$root/dist/signing-state.json"
  jq -e '.updater.available == false and (.updater.artifacts | length == 0)' "$state" >/dev/null \
    || fail "an unsigned run must not report an available updater"
  jq -e --arg a "Cuotai_${v}_amd64.AppImage" \
    '.artifacts[] | select(.name == $a) | .signed == false and .method == "checksum-signature" and .updater_signature == null' \
    "$state" >/dev/null \
    || fail "an unsigned package must keep only its checksum-signature record"
}

# The Windows shape (PRD 0018 RF1.1, PRD 0021 RF3.4): `windows-sign.ps1`
# already wrote one Authenticode record for each installer, and
# `updater-sign.sh` adds the detached updater proof. One artifact, two proofs:
# the same consumer-visible merge must succeed here too.
updater_windows_scenario() {
  local root="$1"
  local v=0.67.0
  mkdir -p "$root/dist" "$root/fragments" "$root/apps/desktop"
  local installer="Cuotai_${v}_x64-setup.exe"
  printf 'installer payload\n' > "$root/dist/$installer"
  printf 'base64 updater signature\n' > "$root/dist/$installer.sig"
  # The Linux packages are required by the manifest; the Windows job does not
  # own them, so they only need to exist for this scenario.
  local pkg
  for pkg in "Cuotai_${v}_amd64.AppImage" "Cuotai_${v}_amd64.deb" "Cuotai_${v}_amd64.rpm"; do
    printf 'payload of %s\n' "$pkg" > "$root/dist/$pkg"
  done

  # What `windows-sign.ps1 -Mode Installers` writes for a credentialed run.
  jq -n --arg a "$installer" '{
    artifacts: [{name: $a, signed: true, method: "authenticode", status: "signed"}]
  }' > "$root/fragments/windows.json"

  # The real `updater-sign.sh` runs against a `cargo` stub that behaves like
  # `cargo tauri signer sign`: it writes the sibling `.sig` the script then
  # validates. No production code is bypassed and no key exists anywhere.
  mkdir -p "$root/fake-bin"
  cat > "$root/fake-bin/cargo" <<'STUB'
#!/usr/bin/env bash
set -euo pipefail
for arg in "$@"; do last="$arg"; done
printf 'dGVzdC1vbmx5LWJhc2U2NC1zaWduYXR1cmU=\n' > "${last}.sig"
STUB
  chmod +x "$root/fake-bin/cargo"

  (
    cd "$root"
    mkdir -p apps/desktop
    export VERSION="$v" DRY_RUN=false
    export PATH="$root/fake-bin:$PATH"
    export TAURI_SIGNING_PRIVATE_KEY="test-only-key" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="test-only"
    "$UPDATER_SIGN" "dist/$installer" "fragments/windows.json"
  ) >/dev/null

  jq -e --arg a "$installer" \
    '[.artifacts[] | select(.name == $a)] | length == 1' "$root/fragments/windows.json" >/dev/null \
    || fail "the Authenticode record was duplicated instead of annotated"
  jq -e --arg a "$installer" --arg s "${installer}.sig" \
    '[.artifacts[] | select(.name == $a) | .updater_signature == $s] | all' \
    "$root/fragments/windows.json" >/dev/null \
    || fail "the installer does not carry its detached updater signature"

  WITH_UPDATER_MANIFEST=1 write_required_dist "$root" "$v"
  other_jobs_fragments "$root" "$installer"
  assemble_fragment "$root" yes
  assembly_contract "$root" "$v" signed
  local state="$root/dist/signing-state.json"
  jq -e --arg a "$installer" \
    '.artifacts[] | select(.name == $a)
      | .signed == true and .method == "authenticode" and .updater_signature == "\($a).sig"' \
    "$state" >/dev/null \
    || fail "the assembled installer does not carry both proofs in one record"
  jq -e --arg s "${installer}.sig" '.updater.artifacts | index($s) != null' "$state" >/dev/null \
    || fail "the assembled updater state does not list the detached signature"
}

main() {
  bash -n "$COLLECT"

  # The assembly half needs the workspace `xtask` binary.
  if [ ! -x "$XTASK" ]; then
    ( cd "$REPO_ROOT" && cargo build -p xtask --locked >/dev/null )
  fi
  [ -x "$XTASK" ] || fail "xtask binary not found at $XTASK (run: cargo build -p xtask --locked)"

  local green_dir zero_dir dup_dir sym_dir signed_dir unsigned_dir windows_dir
  green_dir="$(mktemp -d)"
  zero_dir="$(mktemp -d)"
  dup_dir="$(mktemp -d)"
  sym_dir="$(mktemp -d)"
  signed_dir="$(mktemp -d)"
  unsigned_dir="$(mktemp -d)"
  windows_dir="$(mktemp -d)"
  trap 'rm -rf "${green_dir:-}" "${zero_dir:-}" "${dup_dir:-}" "${sym_dir:-}" "${signed_dir:-}" "${unsigned_dir:-}" "${windows_dir:-}"' EXIT

  note "green: linuxdeploy AppDir staging tree + unrelated sibling collect the right packages"
  green_scenario "$green_dir"
  note "red: only a linuxdeploy AppDir staging tree (no final .AppImage) must fail closed"
  zero_scenario "$zero_dir"
  note "red: two final same-extension matches must fail closed (separate case from any staging tree)"
  duplicate_scenario "$dup_dir"
  note "red: a symlink whose own name matches the expected extension must be rejected"
  symlink_scenario "$sym_dir"
  note "green: bundler-produced updater signatures are collected and assembled"
  updater_signed_scenario "$signed_dir"
  note "green: a run without the updater key stays honestly unsigned"
  updater_unsigned_scenario "$unsigned_dir"
  note "green: an Authenticode installer plus its detached signature assembles as one artifact"
  updater_windows_scenario "$windows_dir"

  echo "PASS: collect-linux.sh green/red fixtures and their assembly contracts behave correctly"
}

main "$@"
