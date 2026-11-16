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
# Usage: apps/desktop/tools/widget-shots/macos/render.sh [widget.json] [out-dir] [--states]
# (`--states` adds review shots of the empty, removed and per-account states;
# pair it with `fixtures/widget-edge.json`.)
#
#        apps/desktop/tools/widget-shots/macos/render.sh --check [<ready|unreadable>:<widget.json>...]
# The CI guard: the shipped decoder must read what the core writes
# (`crates/cuotai-core/tests/fixtures/widget/writer.json`, kept equal to the
# writer's output by a core test) and the compatibility fixtures, and every
# widget must lay out under finite, unspecified and unbounded proposals
# without SwiftUI stopping the process. Writes no PNG.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../../../../.." && pwd)
build="$root/target/widget-shots"
mode=render
if [ "${1:-}" = --check ]; then
    mode=check
    shift
    if [ "$#" -eq 0 ]; then
        compat="$root/crates/cuotai-core/tests/fixtures/compat/widget"
        set -- \
            "ready:$root/crates/cuotai-core/tests/fixtures/widget/writer.json" \
            "ready:$here/../fixtures/widget.json" \
            "ready:$here/../fixtures/widget-edge.json" \
            "ready:$compat/major-1-revision-0/widget.json" \
            "ready:$compat/major-1-revision-1/widget.json" \
            "unreadable:$compat/major-2-revision-0/widget.json"
    fi
else
    fixture=${1:-"$here/../fixtures/widget.json"}
    out=${2:-"$root/docs/assets/screenshots/widgets"}
fi
sdk=$(xcrun --sdk macosx --show-sdk-path)

rm -rf "$build/src"
mkdir -p "$build/src"
for file in "$root"/apps/desktop/widgets/macos/Sources/*.swift; do
    name=$(basename "$file")
    [ "$name" = CuotaiWidgets.swift ] && continue
    sed -e 's/@Environment(\\\.widgetFamily)/@Environment(\\.shotFamily)/' \
        "$file" > "$build/src/$name"
done

# Fail loudly if the rewrite stopped matching (a renamed property would
# otherwise render every widget as `.systemSmall`).
if grep -q 'widgetFamily)' "$build"/src/*.swift; then
    echo "render.sh: a widgetFamily use was not rewritten" >&2
    exit 1
fi
cp "$here/Render.swift" "$build/src/Render.swift"

swiftc -O -parse-as-library -sdk "$sdk" \
    -target "$(uname -m)-apple-macosx14.0" \
    "$build"/src/*.swift -o "$build/render"

if [ "$mode" = check ]; then
    "$build/render" --check "$@"
    exit
fi

mkdir -p "$out"
"$build/render" "$fixture" "$out" ${3:+"$3"}
