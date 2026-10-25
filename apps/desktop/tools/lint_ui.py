#!/usr/bin/env python3
"""Literal-color, duration and keyframe lint of the desktop UI (PRD 0022 RF1.9, DS-COL-1, DS-MOT-3, DS-GOV-4).

Usage: python3 apps/desktop/tools/lint_ui.py [UI_DIR]   (default: apps/desktop/ui next to this file)
       python3 apps/desktop/tools/lint_ui.py --files a.css b.js   (lint only these files, as one of the kinds below)

Rules:
  * `ui/css/*.css`: no hex, `rgb()`/`rgba()`/`hsl()` or named colors, except the token defaults of `themes.css`
    (its `:root` blocks) and the logo artwork (`.wm-ai`, DS-ID-2); themes (`css/themes/*.css`) are not linted here
    (`lint_theme.py` and `contrast.py` own them);
  * no literal durations in `transition`/`animation` (use `--motion-*` tokens);
  * no `@keyframes` other than `spin` (DS-MOT-5, DS-MOT-7);
  * `ui/js/**/*.js`: no literal colors in markup or style strings, except the provider brand tiles of `js/lib.js`
    (DS-ID-3, between the providers marker and the theme marker) and the flag artwork of `js/flags.js`.
Exit status: 0 clean, 1 with findings.
"""
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
HEX = re.compile(r"(?<![\w&])#(?:[0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})\b")
FUNC = re.compile(r"\b(?:rgba?|hsla?)\(")
NAMED = re.compile(
    r"(?:color|background|background-color|border(?:-[a-z]+)*|outline(?:-color)?|fill|stroke|box-shadow|text-shadow)\s*:[^;{}]*?"
    r"(?<![\w-])(white|black|red|green|blue|gray|grey|orange|yellow|purple|pink|silver|navy|teal|maroon|lime|aqua|fuchsia|olive)(?![\w-])",
    re.I,
)
DURATION = re.compile(r"\b(?:transition|animation)(?:-duration|-delay)?\s*:([^;{}]*)")
LITERAL_TIME = re.compile(r"(?<![\w-])\d*\.?\d+m?s\b")
KEYFRAMES = re.compile(r"@keyframes\s+([\w-]+)")
COMMENT = re.compile(r"/\*.*?\*/", re.S)


def line_of(text, pos):
    return text.count("\n", 0, pos) + 1


def strip_comments(text):
    # keep line numbers: replace comments with the same number of newlines
    return COMMENT.sub(lambda m: "\n" * m.group(0).count("\n"), text)


def allowed_css_spans(name, text):
    """Ranges where literal colors are allowed: `themes.css` `:root` blocks and the logo artwork."""
    spans = []
    if name != "themes.css":
        return spans
    for m in re.finditer(r"([^{}]+)\{([^{}]*)\}", text):
        sel = m.group(1).strip()
        if sel.startswith(":root") or ".wm-ai" in sel:
            spans.append((m.start(2), m.end(2)))
    return spans


def lint_css(path, name=None):
    name = name or os.path.basename(path)
    text = strip_comments(open(path, encoding="utf-8").read())
    out = []
    spans = allowed_css_spans(name, text)
    inside = lambda pos: any(a <= pos < b for a, b in spans)
    for rx, what in ((HEX, "literal hex color"), (FUNC, "literal color function")):
        for m in rx.finditer(text):
            if not inside(m.start()):
                out.append((line_of(text, m.start()), what, m.group(0)))
    for m in NAMED.finditer(text):
        if not inside(m.start(1)):
            out.append((line_of(text, m.start(1)), "named color", m.group(1)))
    for m in DURATION.finditer(text):
        value = re.sub(r"var\([^)]*\)", "", m.group(1))
        for t in LITERAL_TIME.finditer(value):
            if float(re.sub(r"m?s$", "", t.group(0)) or 0) != 0:
                out.append((line_of(text, m.start()), "literal duration", t.group(0)))
    for m in KEYFRAMES.finditer(text):
        if m.group(1) != "spin":
            out.append((line_of(text, m.start()), "@keyframes other than spin", m.group(1)))
    return out


def lint_js(path, rel):
    text = open(path, encoding="utf-8").read()
    if rel.endswith("js/flags.js"):
        return []
    lo, hi = -1, -1
    if rel.endswith("js/lib.js"):
        lo = text.find("// ---------- providers ----------")
        hi = text.find("// ---------- tema")
    out = []
    for rx, what in ((HEX, "literal hex color"), (FUNC, "literal color function")):
        for m in rx.finditer(text):
            if lo <= m.start() < hi:
                continue
            out.append((line_of(text, m.start()), what, m.group(0)))
    return out


def walk(ui):
    for name in sorted(os.listdir(os.path.join(ui, "css"))):
        if name.endswith(".css"):
            yield os.path.join(ui, "css", name), f"css/{name}", "css"
    for base, _, files in sorted(os.walk(os.path.join(ui, "js"))):
        for name in sorted(files):
            if name.endswith(".js"):
                p = os.path.join(base, name)
                yield p, os.path.relpath(p, ui).replace(os.sep, "/"), "js"


def main(argv):
    if argv[:1] == ["--files"]:
        targets = [(p, p.replace(os.sep, "/"), "css" if p.endswith(".css") else "js") for p in argv[1:]]
    else:
        ui = argv[0] if argv else os.path.join(HERE, "..", "ui")
        targets = list(walk(ui))
    bad = 0
    for path, rel, kind in targets:
        findings = lint_css(path) if kind == "css" else lint_js(path, rel)
        for line, what, sample in findings:
            print(f"{rel}:{line}: {what}: {sample}")
        bad += len(findings)
    print(f"lint_ui: {bad} finding(s) in {len(targets)} file(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
