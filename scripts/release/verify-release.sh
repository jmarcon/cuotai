#!/usr/bin/env bash
# Downloads every release asset, compares the complete set byte-for-byte with
# the local assembly, then verifies payload checksums, Sigstore identity and
# GitHub build provenance before (and again after) publication.
set -euo pipefail

shopt -s nullglob dotglob
: "${TAG:?}" "${REPO:?}" "${LOCAL_DIR:?}" "${XTASK:?}"
BODY_FILE="${BODY_FILE:-$LOCAL_DIR/release-body.md}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

gh release download "$TAG" --repo "$REPO" --dir "$work" --clobber

declare -A local_assets=()
body_base="$(basename "$BODY_FILE")"
for f in "$LOCAL_DIR"/*; do
  [ -e "$f" ] || continue
  [ -f "$f" ] && [ ! -L "$f" ] || {
    echo "local release input is not a regular file: $f" >&2
    exit 1
  }
  base="$(basename "$f")"
  [ "$base" = "$body_base" ] && continue
  local_assets["$base"]="$f"
done

remote_count=0
for f in "$work"/*; do
  [ -e "$f" ] || continue
  [ -f "$f" ] && [ ! -L "$f" ] || {
    echo "downloaded release asset is not a regular file: $f" >&2
    exit 1
  }
  base="$(basename "$f")"
  [ -n "${local_assets[$base]+present}" ] || {
    echo "unexpected remote release asset: $base" >&2
    exit 1
  }
  cmp -s "${local_assets[$base]}" "$f" || {
    echo "remote release asset differs from local assembly: $base" >&2
    exit 1
  }
  remote_count=$((remote_count + 1))
done

[ "$remote_count" -eq "${#local_assets[@]}" ] || {
  echo "release asset count differs: local ${#local_assets[@]}, remote $remote_count" >&2
  exit 1
}
for base in "${!local_assets[@]}"; do
  [ -f "$work/$base" ] || { echo "remote release asset missing: $base" >&2; exit 1; }
done

"$XTASK" verify-download --dir "$work" --manifest "$work/manifest.json"
(cd "$work" && shasum -a 256 -c SHA256SUMS)

for required in SHA256SUMS.sig SHA256SUMS.pem; do
  [ -s "$work/$required" ] || { echo "missing Sigstore metadata: $required" >&2; exit 1; }
done
cosign verify-blob \
  --signature "$work/SHA256SUMS.sig" \
  --certificate "$work/SHA256SUMS.pem" \
  --certificate-identity "https://github.com/${REPO}/.github/workflows/release.yml@refs/tags/${TAG}" \
  --certificate-oidc-issuer https://token.actions.githubusercontent.com \
  "$work/SHA256SUMS"

for base in "${!local_assets[@]}"; do
  gh attestation verify "$work/$base" --repo "$REPO"
done

echo "all assets of $TAG match the local assembly and have valid integrity and provenance"
