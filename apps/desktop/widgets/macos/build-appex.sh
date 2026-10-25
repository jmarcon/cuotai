#!/bin/sh
# PRD 0014 RF3.1: build the WidgetKit extension as an unsigned, universal
# `CuotaiWidgets.appex` without an Xcode project.
#
# Tauri cannot build a Swift app extension, so the release pipeline builds it
# here and embeds it in `Cuotai.app/Contents/PlugIns/` after `cargo tauri
# build` (scripts/release/macos-embed-widgets.sh), before anything is signed.
# Signing is not done here: the caller signs the extension inside-out with
# `CuotaiWidgets.entitlements` before it signs the app.
#
# The App Intents metadata (`Metadata.appintents`) the Account widget's
# picker needs is produced with Xcode's `appintentsmetadataprocessor`, the
# same tool Xcode runs for an extension target. That needs full Xcode, not
# only the Command Line Tools.
#
# Usage: apps/desktop/widgets/macos/build-appex.sh <out-dir> <version>
set -eu

out=${1:?output directory}
version=${2:?bundle version}
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
deployment=14.0
module=CuotaiWidgets
name=CuotaiWidgets

sdk=$(xcrun --sdk macosx --show-sdk-path)
toolchain=$(dirname "$(dirname "$(dirname "$(xcrun --find swiftc)")")")
xcode_build=$(xcodebuild -version | awk '/Build version/ { print $3 }')
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

ls "$here"/Sources/*.swift > "$work/sources.txt"
# The protocols whose conformances App Intents extracts as constant values.
cat > "$work/protocols.json" <<'EOF'
["AppIntent", "EntityQuery", "AppEntity", "TransientEntity", "AppEnum",
 "AppShortcutProviding", "AppShortcutsProvider", "AnyResolverProviding",
 "AppIntentsPackage", "DynamicOptionsProvider", "WidgetConfigurationIntent"]
EOF

slices=""
: > "$work/constvals.txt"
for arch in arm64 x86_64; do
    triple="$arch-apple-macosx$deployment"
    # `-application-extension` restricts the build to extension-safe API;
    # `-parse-as-library` lets the `@main` WidgetBundle be the entry point.
    xcrun swiftc -parse-as-library -application-extension \
        -O -whole-module-optimization \
        -sdk "$sdk" -target "$triple" \
        -module-name "$module" \
        -emit-const-values-path "$work/$arch.swiftconstvalues" \
        -Xfrontend -const-gather-protocols-file \
        -Xfrontend "$work/protocols.json" \
        -o "$work/$name-$arch" \
        "$here"/Sources/*.swift
    slices="$slices $work/$name-$arch"
done
echo "$work/arm64.swiftconstvalues" > "$work/constvals.txt"

appex="$out/$name.appex"
rm -rf "$appex"
mkdir -p "$appex/Contents/MacOS" "$appex/Contents/Resources"
# shellcheck disable=SC2086 # the slice list is word-split on purpose
lipo -create $slices -output "$appex/Contents/MacOS/$name"

# PRD 0014 RF3.4: the string catalog (en source, pt-BR, es, it) compiles to
# one <lang>.lproj/Localizable.strings per language in the extension.
xcrun xcstringstool compile -o "$appex/Contents/Resources" "$here/Localizable.xcstrings"

sed -e "s/@VERSION@/$version/g" "$here/Info.plist" > "$appex/Contents/Info.plist"
plutil -lint "$appex/Contents/Info.plist" >/dev/null

xcrun appintentsmetadataprocessor \
    --output "$appex/Contents/Resources" \
    --toolchain-dir "$toolchain" \
    --module-name "$module" \
    --sdk-root "$sdk" \
    --xcode-version "$xcode_build" \
    --platform-family macOS \
    --deployment-target "$deployment" \
    --target-triple "arm64-apple-macosx$deployment" \
    --binary-file "$appex/Contents/MacOS/$name" \
    --dependency-file "$work/appintents.d" \
    --stringsdata-file "$work/appintents.stringsdata" \
    --source-file-list "$work/sources.txt" \
    --swift-const-vals-list "$work/constvals.txt" \
    --force
[ -d "$appex/Contents/Resources/Metadata.appintents" ] || {
    echo "appintentsmetadataprocessor wrote no Metadata.appintents" >&2
    exit 1
}

lipo -info "$appex/Contents/MacOS/$name"
echo "built $appex ($version)"
