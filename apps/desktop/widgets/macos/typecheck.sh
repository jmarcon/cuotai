#!/bin/sh
# PRD 0014 RF3.1: type-check the WidgetKit sources without Xcode.
#
# The Command Line Tools carry the macOS SDK with WidgetKit, SwiftUI, Charts
# and AppIntents, so `swiftc -typecheck` is enough to prove the extension
# compiles before any signing work exists. This checks the code, not the
# packaging: building and signing the `.appex` needs Xcode and an Apple
# developer account (PRD 0019), which is an owner prerequisite.
#
# Usage: apps/desktop/widgets/macos/typecheck.sh [deployment-target]
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
target=${1:-arm64-apple-macosx14.0}
sdk=$(xcrun --sdk macosx --show-sdk-path)

# RF3.4 / PRD 0034: the string catalog must compile for every language the
# extension declares in `CFBundleLocalizations` (the 14 desktop languages,
# under their Apple codes); English is the source and has no table of its
# own. `crates/cuotai-core/tests/widget_strings.rs` checks that this list
# matches the desktop languages and that every string has every language.
cat_out=$(mktemp -d)
trap 'rm -rf "$cat_out"' EXIT
xcrun xcstringstool compile -o "$cat_out" "$here/Localizable.xcstrings"
languages=$(plutil -extract CFBundleLocalizations json -o - "$here/Info.plist" | tr -d '[]"' | tr ',' ' ')
[ -n "$languages" ] || { echo "Info.plist lists no CFBundleLocalizations" >&2; exit 1; }
for l in $languages; do
    [ "$l" = en ] && continue
    [ -f "$cat_out/$l.lproj/Localizable.strings" ] || { echo "missing $l strings" >&2; exit 1; }
done

swiftc -typecheck -parse-as-library \
    -sdk "$sdk" \
    -target "$target" \
    "$here"/Sources/*.swift