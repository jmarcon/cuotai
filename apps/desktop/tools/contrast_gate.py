#!/usr/bin/env python3
"""Strict contrast gate of the built-in themes (PRD 0022 RF15.5, DS-COL-5..7, DS-GOV-4).

Runs `contrast.py --strict` on every built-in theme (ui/css/themes/*.css except index.css) and compares the failures
with the checked-in baseline `contrast-baseline.txt` (one line per known failure: theme, mode, pair, measured ratio).
It fails on any failure not in the baseline and on any baseline ratio that got worse; it reports baseline lines
that now pass so they are removed in the same commit (the baseline is empty when PRD 0022 is delivered).

Usage: python3 apps/desktop/tools/contrast_gate.py [--write-baseline] [THEME.css ...]
Exit status: 0 clean, 1 with new or worse failures (or stale baseline lines).
"""
import os
import re
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
THEMES = os.path.join(HERE, "..", "ui", "css", "themes")
BASELINE = os.path.join(HERE, "contrast-baseline.txt")
LINE = re.compile(r"^\s+FALHA (\S+) × (\S+): ([\d.]+) \(mín ([\d.]+)\)")
HEAD = re.compile(r"^== (\S+) / (dark|light)")


def failures(path):
    out = subprocess.run([sys.executable, os.path.join(HERE, "contrast.py"), "--strict", path], capture_output=True, text=True).stdout
    found, where = {}, None
    for line in out.splitlines():
        h = HEAD.match(line)
        if h:
            where = f"{h.group(1)} {h.group(2)}"
            continue
        m = LINE.match(line)
        if m and where:
            found[f"{where} {m.group(1)}×{m.group(2)}"] = float(m.group(3))
        elif "FALHA" in line or "FALTA" in line:
            found[f"{where} {line.strip()}"] = 0.0
    return found


def main(argv):
    write = "--write-baseline" in argv
    paths = [a for a in argv if not a.startswith("--")] or sorted(
        os.path.join(THEMES, f) for f in os.listdir(THEMES) if f.endswith(".css") and f != "index.css")
    current = {}
    for p in paths:
        current.update(failures(p))
    if write:
        with open(BASELINE, "w", encoding="utf-8") as f:
            f.write("# PRD 0022 RF15.5: known strict-contrast failures of built-in themes (theme mode pair ratio).\n")
            f.write("# Fix a theme, delete its lines in the same commit; the gate fails on anything new or worse.\n")
            for key, ratio in sorted(current.items()):
                f.write(f"{key} {ratio:.2f}\n")
        print(f"baseline written: {len(current)} line(s)")
        return 0
    base = {}
    if os.path.exists(BASELINE):
        for line in open(BASELINE, encoding="utf-8"):
            line = line.strip()
            if line and not line.startswith("#"):
                key, ratio = line.rsplit(" ", 1)
                base[key] = float(ratio)
    bad = 0
    for key, ratio in sorted(current.items()):
        if key not in base:
            print(f"NEW   {key} {ratio:.2f}")
            bad += 1
        elif ratio + 0.005 < base[key]:
            print(f"WORSE {key} {ratio:.2f} (baseline {base[key]:.2f})")
            bad += 1
    checked = {f"{os.path.basename(p)[:-4]}" for p in paths}
    for key in sorted(base):
        if key.split(" ", 1)[0] in checked and key not in current:
            print(f"FIXED {key}: remove it from contrast-baseline.txt")
            bad += 1
    print(f"contrast gate: {len(current)} known failure(s), {bad} problem(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
