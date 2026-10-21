#!/usr/bin/env bash
# Draft/publish of the GitHub Release (PRD 0021 RF4.1/RF4.3).
# Draft reruns reconcile the complete asset set. Published releases are never
# mutated or deleted.
set -euo pipefail
shopt -s nullglob dotglob

: "${TAG:?}" "${REPO:?}" "${DIST_DIR:?}" "${BODY_FILE:?}" "${PRERELEASE:?}"
command="${1:?draft or publish}"
state="$(gh release view "$TAG" --repo "$REPO" --json isDraft --jq .isDraft 2>/dev/null || echo missing)"

case "$command" in
  draft)
    case "$state" in
      false)
        echo "release $TAG is already published; refusing to overwrite its assets" >&2
        exit 1
        ;;
      true)
        echo "updating existing draft $TAG"
        gh release edit "$TAG" --repo "$REPO" --title "Cuotai $TAG" \
          --notes-file "$BODY_FILE" --prerelease="$PRERELEASE"
        ;;
      missing)
        echo "creating draft $TAG"
        args=(release create "$TAG" --repo "$REPO" --draft --title "Cuotai $TAG" --notes-file "$BODY_FILE")
        [ "$PRERELEASE" = "true" ] && args+=(--prerelease)
        gh "${args[@]}"
        ;;
      *)
        echo "cannot determine release state for $TAG: $state" >&2
        exit 1
        ;;
    esac

    declare -A desired=()
    body_base="$(basename "$BODY_FILE")"
    for f in "$DIST_DIR"/*; do
      [ -e "$f" ] || continue
      [ -f "$f" ] && [ ! -L "$f" ] || {
        echo "release input is not a regular file: $f" >&2
        exit 1
      }
      base="$(basename "$f")"
      [ "$base" = "$body_base" ] && continue
      desired["$base"]="$f"
    done
    [ "${#desired[@]}" -gt 0 ] || { echo "no release assets in $DIST_DIR" >&2; exit 1; }

    # Extras can only be deleted after the state check above proved this is a
    # draft (or after this script created it as a draft).
    while IFS= read -r asset; do
      [ -n "$asset" ] || continue
      if [ -z "${desired[$asset]+present}" ]; then
        echo "deleting stale draft asset $asset"
        gh release delete-asset "$TAG" "$asset" --repo "$REPO" --yes
      fi
    done < <(gh release view "$TAG" --repo "$REPO" --json assets --jq '.assets[].name')

    for base in "${!desired[@]}"; do
      gh release upload "$TAG" "${desired[$base]}" --repo "$REPO" --clobber
    done
    ;;
  publish)
    [ "$state" = "true" ] || {
      echo "no draft release $TAG to publish (state: $state)" >&2
      exit 1
    }
    gh release edit "$TAG" --repo "$REPO" --draft=false
    echo "published $TAG"
    ;;
  *)
    echo "unknown command '$command'" >&2
    exit 1
    ;;
esac
