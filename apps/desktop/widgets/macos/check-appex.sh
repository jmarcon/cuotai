#!/bin/sh
# PRD 0014 RF3: check a built `CuotaiWidgets.appex` against what macOS needs
# to load it and against `widget-contract.json`, the configuration macOS has
# stored for every widget already placed.
#
# build-appex.sh runs this on every build (CI's `widgets` job, the pre-push
# preflight and the release). It catches a bundle the system would not load
# (identifier, executable, extension point, minimum system), a version that
# differs from the app's, a missing entry point or architecture, missing App
# Intents metadata, and an intent, parameter, entity or enum case that was
# renamed or removed. It cannot see signing, notarization or the sandbox at
# run time: scripts/release/smoke-desktop-macos.sh checks the signed app.
#
# Usage: apps/desktop/widgets/macos/check-appex.sh <CuotaiWidgets.appex> <version>
set -eu

appex=${1:?path to CuotaiWidgets.appex}
version=${2:?bundle version}
here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
contract="$here/widget-contract.json"
plist="$appex/Contents/Info.plist"
metadata="$appex/Contents/Resources/Metadata.appintents/extract.actionsdata"

fail() {
    echo "check-appex: $*" >&2
    exit 1
}

[ -f "$plist" ] || fail "no Contents/Info.plist in $appex"
[ -f "$metadata" ] || fail "no App Intents metadata: the widgets cannot resolve their configuration"

value() {
    plutil -extract "$1" raw -o - "$plist" 2>/dev/null || echo ""
}
expected() {
    plutil -extract "$1" raw -o - "$contract"
}

[ "$(value CFBundleIdentifier)" = "$(expected bundle_identifier)" ] ||
    fail "CFBundleIdentifier is '$(value CFBundleIdentifier)', not $(expected bundle_identifier)"
[ "$(value CFBundleExecutable)" = "$(expected executable)" ] ||
    fail "CFBundleExecutable is '$(value CFBundleExecutable)'"
[ "$(value CFBundlePackageType)" = "XPC!" ] || fail "CFBundlePackageType is not XPC!"
[ "$(value NSExtension.NSExtensionPointIdentifier)" = "com.apple.widgetkit-extension" ] ||
    fail "NSExtensionPointIdentifier is not com.apple.widgetkit-extension"
[ "$(value LSMinimumSystemVersion)" = "$(expected minimum_system_version)" ] ||
    fail "LSMinimumSystemVersion is '$(value LSMinimumSystemVersion)'"
# chronod refuses what an extension draws when its version is not the one
# LaunchServices registered for the bundle.
[ "$(value CFBundleVersion)" = "$version" ] || fail "CFBundleVersion is '$(value CFBundleVersion)', not $version"
[ "$(value CFBundleShortVersionString)" = "$version" ] ||
    fail "CFBundleShortVersionString is '$(value CFBundleShortVersionString)', not $version"

binary="$appex/Contents/MacOS/$(expected executable)"
[ -x "$binary" ] || fail "no executable at $binary"
archs=$(lipo -archs "$binary")
for arch in arm64 x86_64; do
    case " $archs " in
    *" $arch "*) ;;
    *) fail "the executable has no $arch slice ($archs)" ;;
    esac
    nm -arch "$arch" -u "$binary" | grep -q '^_NSExtensionMain$' ||
        fail "the $arch slice is not linked with the _NSExtensionMain entry point"
done

python3 -I - "$contract" "$metadata" <<'PY'
import json
import sys

contract = json.load(open(sys.argv[1]))
data = json.load(open(sys.argv[2]))
errors = []

actions = data.get("actions", {})
for intent, params in contract["intents"].items():
    action = actions.get(intent)
    if action is None:
        errors.append(f"intent {intent} is missing from the metadata")
        continue
    if not action.get("mangledTypeName", "").startswith("13CuotaiWidgets"):
        errors.append(f"intent {intent} is not in the CuotaiWidgets module")
    built = [p.get("name") for p in action.get("parameters", [])]
    for name in params:
        if name not in built:
            errors.append(f"intent {intent} lost its parameter {name!r}")
    for name in built:
        if name not in params:
            errors.append(f"intent {intent} has a new parameter {name!r}: append it to widget-contract.json")
for intent in contract["kinds"].values():
    if intent not in contract["intents"]:
        errors.append(f"widget-contract.json names {intent} for a kind but lists no parameters for it")

entities = data.get("entities", {})
for entity in contract["entities"]:
    if entity not in entities:
        errors.append(f"entity {entity} is missing from the metadata")

enums = {e.get("identifier"): [c.get("identifier") for c in e.get("cases", [])] for e in data.get("enums", [])}
for name, cases in contract["enums"].items():
    built = enums.get(name)
    if built is None:
        errors.append(f"enum {name} is missing from the metadata")
        continue
    for case in cases:
        if case not in built:
            errors.append(f"enum {name} lost its case {case!r}")
    for case in built:
        if case not in cases:
            errors.append(f"enum {name} has a new case {case!r}: append it to widget-contract.json")

if errors:
    for error in errors:
        print(f"check-appex: {error}", file=sys.stderr)
    sys.exit(1)
PY

echo "check-appex: ok ($version, $archs)"
