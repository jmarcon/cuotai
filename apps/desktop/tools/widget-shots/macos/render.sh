#!/bin/sh
# Renders the macOS WidgetKit views to PNG for the docs (PRD 0014).
#
# Compiles `apps/desktop/widgets/macos/Sources` (without the `@main`
# `WidgetBundle`) together with `Render.swift` into a throwaway executable
# under `target/widget-shots/`, then runs it on the example.com fixture.
# Needs macOS 14+ and the Swift toolchain (Command Line Tools or Xcode); no
# signing, no app group, no `.appex`.
#
# The only change made to the shipped sources is in the copy: the
# `@Environment(\.widgetFamily)` read becomes `@Environment(\.shotFamily)`,
# because `widgetFamily` cannot be set outside a widget extension.
#
# Usage: apps/desktop/tools/widget-shots/macos/render.sh [widget.json] [out-dir]
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../../../../.." && pwd)
fixture=${1:-"$here/../fixtures/widget.json"}
out=${2:-"$root/docs/assets/screenshots/widgets"}
build="$root/target/widget-shots"
sdk=$(xcrun --sdk macosx --show-sdk-path)

rm -rf "$build/src"
mkdir -p "$build/src"
for file in "$root"/apps/desktop/widgets/macos/Sources/*.swift; do
    name=$(basename "$file")
    [ "$name" = CuotaiWidgets.swift ] && continue
    sed -e 's/@Environment(\\\.widgetFamily)/@Environment(\\.shotFamily)/' \
        -e 's/\.containerRelativeFrame(\.horizontal) { length, _ in length \* clamped }/.shotTrackFraction(clamped)/' \
        "$file" > "$build/src/$name"
done

# Fail loudly if a rewrite stopped matching (a renamed property would
# otherwise render every widget as `.systemSmall`, or bars without a fill).
if grep -q 'widgetFamily)\|containerRelativeFrame' "$build"/src/*.swift; then
    echo "render.sh: a widgetFamily or containerRelativeFrame use was not rewritten" >&2
    exit 1
fi
cp "$here/Render.swift" "$build/src/Render.swift"

swiftc -O -parse-as-library -sdk "$sdk" \
    -target "$(uname -m)-apple-macosx14.0" \
    "$build"/src/*.swift -o "$build/render"

mkdir -p "$out"
"$build/render" "$fixture" "$out"
