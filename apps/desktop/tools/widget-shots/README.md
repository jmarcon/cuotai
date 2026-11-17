# Widget screenshots (PRD 0014)

Dev-only tooling that produces the widget images under
[`docs/assets/screenshots/widgets/`](../../../../docs/assets/screenshots/widgets/)
for [`docs/widgets.md`](../../../../docs/widgets.md) and the per-OS guides.
Nothing here is part of the app, the widget package or a release, and nothing
talks to a provider or reads a credential: every image is drawn from
[`fixtures/widget.json`](fixtures/widget.json), a hand-written `widget.json`
(schema 1) whose only identity is `alex@example.com`.

Run both from the repository root; each takes an optional fixture path and
output directory.

`render.sh --proposals [<fixture>] <out-dir>` draws every widget under its own
size and under the unspecified and unbounded proposals macOS 27.2 lays widgets
out with, each on a card of the widget's size, to review what a proposal does
to a widget (`--check` asks for the same picture under all of them).

For review rather than the docs, `render.sh <fixture> <out-dir> --states`
adds `macos-state-*` shots: the Account and History widgets with no file and
with a pinned account that went away, and every account of the fixture
pinned in turn. Pair it with
[`fixtures/widget-edge.json`](fixtures/widget-edge.json) (an account without
a plan limit, and readings far over the limit that a foreign or hand-edited
file could carry) and write it outside `docs/`.

Languages (PRD 0034): the widget draws in the app's language, which the app
writes into the file (`locale`), and draws each metric's `label_text` (its
name in that language) instead of the raw label. `render.sh` compiles the
widget's `Localizable.xcstrings` next to its executable, so a fixture with a
`locale` renders in that language; `--locale=<code>` overrides the fixture's
(`render.sh fixtures/widget-edge.json out --states --locale=vi`).
[`fixtures/widget-many.json`](fixtures/widget-many.json) has eight accounts
(one exhausted, one stale, one with nothing used, one with a long name, one
with a long window label, one without a plan limit, one unavailable), for the
list layouts. `--states` also draws each of the widgets' choices once away
from its default (`option-*` shots: what was used, compact, the
configuration order with empty accounts hidden, a chosen quota, a shorter
History period), and `--check` lays those out too.
[`fixtures/widget-localized.json`](fixtures/widget-localized.json) is a German
file with Portuguese raw labels. `--check` also lays every widget out in de,
ru, hi-IN and vi (the longest translations), fails when a file in a language
draws English, and, for a file whose metrics all carry `label_text`, fails
when a raw label is drawn.

| Platform | Command | What it produces | Real or rendered |
|---|---|---|---|
| macOS | `sh apps/desktop/tools/widget-shots/macos/render.sh` | `macos-<widget>-<family>-<light\|dark>.png` and `macos-gallery-<light\|dark>.png` | The shipped SwiftUI views, drawn off-screen (see below) |
| Linux | `sh apps/desktop/tools/widget-shots/linux/capture.sh` | `linux-waybar.png` | A real screenshot of Waybar in a headless sway session |
| Windows | — | — | Windows has no widget surface (see below) |

## macOS: `macos/render.sh`

Needs macOS 14+ and the Swift toolchain (Command Line Tools or Xcode). No
signing, no *app group*, no `.appex`.

The script copies `apps/desktop/widgets/macos/Sources` (without the `@main`
`WidgetBundle` in `CuotaiWidgets.swift`) to `target/widget-shots/src`, adds
[`macos/Render.swift`](macos/Render.swift) and compiles a throwaway
executable that feeds the fixture through the shipped
`WidgetSnapshot.load(from:)` into the shipped entry views
(`SummaryWidgetEntryView`, `AccountWidgetEntryView`,
`HistoryWidgetEntryView`) and writes PNGs with SwiftUI's `ImageRenderer`, at
2x, in light and dark.

What differs from a live widget, and why:

- **Family.** Outside a widget extension `\.widgetFamily` cannot be set, so
  the copy reads `\.shotFamily` instead (a `sed` rewrite; the script fails if
  the rewrite stops matching).
- **Background and size.** `.containerBackground(.fill.tertiary, for: .widget)`
  is not drawn outside an extension; the harness draws a solid rounded card
  behind the entry view. The view brings its own size and margins, as in the
  extension: without a timeline context it takes the family's size (small
  164x164, medium 344x164, large 344x344, the sizes macOS 26 and 27 give a
  live widget) and, outside an extension, the 16 pt default margin.
- **Ages.** Every timestamp of the fixture is shifted so `generated_at` is
  "now", so the ages read like the fixture ("2 min") on any day. The system's
  relative text is frozen at render time ("2 min, 0 secs"); a live widget
  advances it by itself.

The families are the ones the widgets declare: Summary (small, medium,
large), Account (small, medium), History (medium). No accessory family is
declared, so none is rendered. A visual check on a clean Mac with the signed
`.appex` is still the PRD's acceptance criterion; these images are not that.

## Linux: `linux/capture.sh`

Needs Docker (or a compatible engine) on a Linux or macOS host, amd64 or
arm64. The [`Dockerfile`](linux/Dockerfile) (Alpine 3.20) runs sway on
wlroots' headless backend with the software renderer
(`WLR_BACKENDS=headless`, `WLR_RENDERER=pixman`), starts Waybar with
[`linux/waybar/config.jsonc`](linux/waybar/config.jsonc) and
[`style.css`](linux/waybar/style.css), and captures the bar with `grim`.
The fixture is copied in and the PNG copied out (no bind mounts, so it also
works with Docker Desktop). `SHOT_DEBUG=1` prints the sway and Waybar logs.

The module is [`linux/waybar/cuotai-waybar.sh`](linux/waybar/cuotai-waybar.sh),
the example Waybar module documented in `docs/widgets.md`: it reads
`widget.json` with `jq` and prints Waybar's JSON protocol. The bar's clock
shows the time of the capture.

GNOME and KDE have no headless path light enough for this (a full GNOME
Shell or Plasma session), and the repository ships no extension or plasmoid
for them, so no image is produced for either.

## Windows

Cuotai has no Windows widget surface: PRD 0014 delivers no native widget
there (Windows 11 Widgets Board providers need a packaged MSIX app). The app
only writes `%LOCALAPPDATA%\cuotai\widget.json` for third-party tools;
`docs/gui-windows.md` points to the notification-area popover screenshots.

## Checks before committing new images

- Keep each PNG at or below roughly 300 KB (`ls -l docs/assets/screenshots/widgets`).
- The fixture uses `example.com` only; `cargo test -p xtask --test privacy`
  fails on personal-mailbox addresses in tracked files.
- `swift build` in `apps/desktop/widgets/macos` must still pass: this tooling
  never edits the widget sources, only a copy under `target/`.
