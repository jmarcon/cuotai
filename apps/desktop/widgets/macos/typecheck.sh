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

# RF3.4: the string catalog must compile for the four languages.
cat_out=$(mktemp -d)
trap 'rm -rf "$cat_out"' EXIT
xcrun xcstringstool compile -o "$cat_out" "$here/Localizable.xcstrings"
for l in pt-BR es it; do
    [ -f "$cat_out/$l.lproj/Localizable.strings" ] || { echo "missing $l strings" >&2; exit 1; }
done

swiftc -typecheck -parse-as-library \
    -sdk "$sdk" \
    -target "$target" \
    "$here"/Sources/*.swift