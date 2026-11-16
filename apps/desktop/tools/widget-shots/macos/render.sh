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
# Usage: apps/desktop/tools/widget-shots/macos/render.sh [widget.json] [out-dir] [--states] [--locale=<code>]
# (`--states` adds review shots of the empty, removed and per-account states;
# pair it with `fixtures/widget-edge.json`. `--locale=de` draws the file as
# the app writes it in that language, PRD 0034.)
#
#        apps/desktop/tools/widget-shots/macos/render.sh --check [<ready|unreadable>:<widget.json>...]
# The CI guard: the shipped decoder must read what the core writes
# (`crates/cuotai-core/tests/fixtures/widget/writer.json`, kept equal to the
# writer's output by a core test) and the compatibility fixtures; the widget
# sources must not use a view that grows to whatever it is offered (see
# below); and every widget, drawn under its own size and under unspecified
# and unbounded proposals, must be the same picture each time, inside its
# margins and, with data, spread over the widget, in the file's language and
# in the longest translations (de, ru, hi-IN, vi). A file in a language must
# draw that language, and a file whose metrics carry `label_text` must never
# draw their raw labels (PRD 0034). Writes no PNG.
#
#        apps/desktop/tools/widget-shots/macos/render.sh --proposals [widget.json] <out-dir>
# For review: every widget drawn under each of those proposals, on a card.
set -eu

here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
root=$(CDPATH= cd -- "$here/../../../../.." && pwd)
build="$root/target/widget-shots"
mode=render
if [ "${1:-}" = --proposals ]; then
    mode=proposals
    shift
    if [ "$#" -ge 2 ]; then
        fixture=$1
        out=$2
    else
        fixture="$here/../fixtures/widget.json"
        out=${1:?usage: render.sh --proposals [widget.json] <out-dir>}
    fi
elif [ "${1:-}" = --check ]; then
    mode=check
    shift
    if [ "$#" -eq 0 ]; then
        compat="$root/crates/cuotai-core/tests/fixtures/compat/widget"
        set -- \
            "ready:$root/crates/cuotai-core/tests/fixtures/widget/writer.json" \
            "ready:$here/../fixtures/widget.json" \
            "ready:$here/../fixtures/widget-edge.json" \
            "ready:$here/../fixtures/widget-localized.json" \
            "ready:$compat/major-1-revision-0/widget.json" \
            "ready:$compat/major-1-revision-1/widget.json" \
            "unreadable:$compat/major-2-revision-0/widget.json"
    fi
else
    fixture=${1:-"$here/../fixtures/widget.json"}
    out=${2:-"$root/docs/assets/screenshots/widgets"}
fi
sdk=$(xcrun --sdk macosx --show-sdk-path)

# PRD 0014 RF3.2: macOS 27.2 lays widget content out under proposals that are
# not the widget's size (an unbounded width among them), and 0.86.3 and
# 0.86.4 shipped widgets that crashed or collapsed there. A widget's size
# comes from WidgetKit (`WidgetCanvas`), and nothing inside may ask for "as
# much as offered": no GeometryReader, no Spacer, no Grid, no infinite frame.
# Comment lines are not code.
if [ "$mode" = check ]; then
    flexible=$(grep -nE 'GeometryReader|Spacer\(|Grid\(|\.infinity' \
        "$root"/apps/desktop/widgets/macos/Sources/*.swift | grep -vE '^[^:]+:[0-9]+:[[:space:]]*//' || true)
    if [ -n "$flexible" ]; then
        echo "render.sh: the widgets use views that grow to the proposal (macOS 27.2 offers them an unbounded width):" >&2
        echo "$flexible" | sed "s|$root/||" >&2
        exit 1
    fi
fi

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

# PRD 0034: the widget's strings, compiled next to the executable, whose
# bundle they then are: a file in a language draws that language's texts
# (and `--check` fails when one does not).
rm -rf "$build"/*.lproj
xcrun xcstringstool compile -o "$build" "$root/apps/desktop/widgets/macos/Localizable.xcstrings"

swiftc -O -parse-as-library -sdk "$sdk" \
    -target "$(uname -m)-apple-macosx14.0" \
    "$build"/src/*.swift -o "$build/render"

if [ "$mode" = check ]; then
    "$build/render" --check "$@"
    exit
fi
if [ "$mode" = proposals ]; then
    "$build/render" --proposals "$fixture" "$out"
    exit
fi

mkdir -p "$out"
shift $(( $# < 2 ? $# : 2 ))
"$build/render" "$fixture" "$out" "$@"
