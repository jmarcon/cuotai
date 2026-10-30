#!/bin/sh
# Builds the project site into _site/ and validates it. Run it from the
# repository root: `sh scripts/site-build.sh`. .github/workflows/pages.yml runs
# it before any deploy, so a failure here publishes nothing.
#
# _site/ is rebuilt from scratch: site/ first, then docs/assets/screenshots/
# into _site/assets/screenshots/ (one copy of the screenshots, shared with the
# README). The site lives under /cuotai/ and is published independently of the
# app releases, so the build fails, listing every problem, on:
#   - a symbolic link (the Pages artifact would dereference it);
#   - no index.html;
#   - a relative link, source or CSS url() that does not resolve inside _site/;
#   - a root-relative link (/x), which leaves /cuotai/;
#   - an external <script> tag;
#   - a hard-coded release version (vX.Y.Z or 0.NN.N) in HTML: link to the
#     latest release instead of naming one;
#   - a personal mailbox address, in any text file (same rule and domains as
#     xtask/tests/privacy.rs; keep the two lists equal).
#
# Exit codes: 0 valid site, 1 validation failure or usage error.
set -eu

die() {
    echo "site-build: $*" >&2
    exit 1
}

[ -d site ] || die "site/ not found; run this from the repository root"
[ -d docs/assets/screenshots ] || die "docs/assets/screenshots/ not found; run this from the repository root"
command -v python3 >/dev/null 2>&1 || die "python3 is required"

rm -rf _site
cp -R site _site
mkdir -p _site/assets/screenshots
cp -R docs/assets/screenshots/. _site/assets/screenshots/

python3 -I - _site <<'PY'
import html
import html.parser
import os
import re
import sys
import urllib.parse

ROOT = sys.argv[1]
PERSONAL_DOMAINS = {
    "gmail.com",
    "googlemail.com",
    "hotmail.com",
    "outlook.com",
    "live.com",
    "yahoo.com",
    "icloud.com",
    "me.com",
    "proton.me",
    "protonmail.com",
}
ADDRESS = re.compile(r"([a-z0-9._%+-]+)@([a-z0-9.-]+)")
VERSION = re.compile(
    r"(?<![0-9A-Za-z.])(?:v\d+\.\d+\.\d+|0\.\d{2,3}\.\d+)(?!\.?\d|[A-Za-z])"
)
SCHEME = re.compile(r"[A-Za-z][A-Za-z0-9+.-]*:")
CSS_COMMENT = re.compile(r"/\*.*?\*/", re.S)
CSS_REFERENCE = re.compile(
    r"""url\(\s*(?:"([^"]*)"|'([^']*)'|([^)\s]*))\s*\)"""
    r"""|@import\s+(?:"([^"]*)"|'([^']*)')"""
)
# Attributes that hold one URL, whatever the element.
URL_ATTRIBUTES = {"href", "xlink:href", "src", "poster", "data", "action"}
SRCSET_ATTRIBUTES = {"srcset", "imagesrcset"}

errors = []


def fail(path, line, message):
    errors.append(f"{os.path.relpath(path, ROOT)}:{line}: {message}")


def is_external(url):
    return url.startswith("//") or SCHEME.match(url) is not None


def check_url(path, line, url):
    """Reports a local reference that breaks once published under /cuotai/."""
    url = url.strip()
    if not url or url.startswith("#") or is_external(url):
        return
    if url.startswith("/"):
        fail(path, line, f"root-relative link {url} leaves /cuotai/; use a relative path")
        return
    target = urllib.parse.unquote(re.split(r"[?#]", url, maxsplit=1)[0])
    if not target:
        return
    resolved = os.path.normpath(os.path.join(os.path.dirname(path), target))
    root = os.path.normpath(ROOT)
    if resolved != root and not resolved.startswith(root + os.sep):
        fail(path, line, f"link {url} escapes the site root")
    elif os.path.isdir(resolved):
        if not os.path.isfile(os.path.join(resolved, "index.html")):
            fail(path, line, f"link {url} is a folder without index.html")
    elif not os.path.isfile(resolved):
        fail(path, line, f"link {url} does not resolve inside the site")


def check_css(path, line, text):
    # Comments are blanked, keeping their newlines, so line numbers stay true.
    text = CSS_COMMENT.sub(lambda m: re.sub(r"[^\n]", " ", m.group()), text)
    for match in CSS_REFERENCE.finditer(text):
        url = next((group for group in match.groups() if group is not None), "")
        check_url(path, line + text.count("\n", 0, match.start()), url)


def check_version(path, line, text):
    for match in VERSION.finditer(text):
        where = line + text.count("\n", 0, match.start())
        fail(path, where, f"hard-coded release version {match.group()}; the site must not name one")


class Page(html.parser.HTMLParser):
    def __init__(self, path):
        super().__init__(convert_charrefs=True)
        self.path = path
        # Numbers inside inline SVG are geometry ("0.25.5" is two valid path
        # coordinates) and those inside <style> are CSS, not versions.
        self.not_prose = 0
        self.in_style = False

    def handle_starttag(self, tag, attrs):
        line = self.getpos()[0]
        if tag in ("svg", "style"):
            self.not_prose += 1
        self.in_style = self.in_style or tag == "style"
        for name, value in attrs:
            if value is None:
                continue
            if name in URL_ATTRIBUTES:
                check_url(self.path, line, value)
                if tag == "script" and name == "src" and is_external(value.strip()):
                    fail(self.path, line, f"external script {value.strip()}; keep scripts in the site")
            elif name in SRCSET_ATTRIBUTES:
                for candidate in value.split(","):
                    if candidate.split():
                        check_url(self.path, line, candidate.split()[0])
            elif name == "style":
                check_css(self.path, line, value)
                continue
            if not self.not_prose:
                check_version(self.path, line, value)

    def handle_endtag(self, tag):
        if tag in ("svg", "style") and self.not_prose:
            self.not_prose -= 1
        self.in_style = self.in_style and tag != "style"

    def handle_data(self, data):
        line = self.getpos()[0]
        if self.in_style:
            check_css(self.path, line, data)
        if not self.not_prose:
            check_version(self.path, line, data)

    def handle_comment(self, data):
        check_version(self.path, self.getpos()[0], data)


def check_addresses(path, text):
    for number, line in enumerate(text.lower().splitlines(), 1):
        for local, domain in ADDRESS.findall(line):
            domain = domain.rstrip(".")
            if domain in PERSONAL_DOMAINS:
                # Masked, so a failure never reprints the address.
                fail(path, number, f"personal mailbox {local[0]}***@{domain}; use a placeholder")


files = []
for folder, subfolders, names in os.walk(ROOT):
    for name in subfolders + names:
        path = os.path.join(folder, name)
        if os.path.islink(path):
            fail(path, 1, "symbolic link; the Pages artifact would dereference it")
        elif name in names:
            files.append(path)

if not os.path.isfile(os.path.join(ROOT, "index.html")):
    errors.append("index.html: missing; the site would answer 404")

for path in sorted(files):
    extension = os.path.splitext(path)[1].lower()
    is_html = extension in (".html", ".htm")
    try:
        with open(path, encoding="utf-8") as handle:
            text = handle.read()
    except UnicodeDecodeError:
        # Binary (images, fonts): nothing to scan.
        continue
    check_addresses(path, html.unescape(text) if is_html else text)
    if is_html:
        Page(path).feed(text)
    elif extension == ".css":
        check_css(path, 1, text)

if errors:
    print(f"site-build: {len(errors)} problem(s) in {ROOT}/:", file=sys.stderr)
    for error in errors:
        print(f"  {error}", file=sys.stderr)
    sys.exit(1)
print(f"site-build: {len(files)} file(s) valid in {ROOT}/")
PY
