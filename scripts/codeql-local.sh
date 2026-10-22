#!/bin/sh
# Runs CodeQL locally with the query suites of GitHub's code-scanning default
# setup, so findings show up before a push instead of as alerts on GitHub.
#
# Usage: sh scripts/codeql-local.sh [--swift] [--keep] [language ...]
#
#   language   rust, python, javascript-typescript, actions, swift.
#              Default: rust python javascript-typescript actions
#              (all build-mode none). Swift is opt-in and macOS-only: it
#              traces `swift build` of apps/desktop/widgets/macos.
#   --swift    add swift to the selected languages.
#   --keep     keep the source snapshot after the run.
#
# The analyzed tree is a snapshot of the tracked and untracked-but-not-ignored
# files (`git ls-files -co --exclude-standard`), so `target/`, `node_modules/`,
# local configs and nested agent worktrees are not scanned. Databases, SARIF
# and CSV go to ${CUOTAI_CODEQL_DIR:-${TMPDIR:-/tmp}/cuotai-codeql}, never
# inside the repository. Results can differ slightly from GitHub's default
# setup when the local CLI or query packs are a different version.
#
# Exit codes: 0 no findings, 3 findings reported, 1 tool or usage error.
set -eu

die() {
    echo "codeql-local: $*" >&2
    exit 1
}

if ! command -v codeql >/dev/null 2>&1; then
    die "codeql not found on PATH. Install it with 'brew install --cask codeql',
  from https://github.com/github/codeql-action/releases (codeql-bundle), or
  with 'gh extension install github/gh-codeql' (then use 'gh codeql')."
fi

repo=$(git rev-parse --show-toplevel 2>/dev/null) || die "run inside the Cuotai git repository"
tmp=${TMPDIR:-/tmp}
out=${CUOTAI_CODEQL_DIR:-${tmp%/}/cuotai-codeql}
out=${out%/}
case "$out/" in
    "$repo"/*) die "CUOTAI_CODEQL_DIR must be outside the repository ($out)" ;;
esac

langs=""
keep=0
for arg in "$@"; do
    case "$arg" in
        --swift) langs="$langs swift" ;;
        --keep) keep=1 ;;
        -h | --help)
            sed -n '2,21p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        rust | python | javascript-typescript | actions | swift) langs="$langs $arg" ;;
        javascript | typescript) langs="$langs javascript-typescript" ;;
        *) die "unknown argument '$arg' (see --help)" ;;
    esac
done
only_swift=1
for l in $langs; do [ "$l" = swift ] || only_swift=0; done
if [ -z "$langs" ]; then
    langs="rust python javascript-typescript actions"
elif [ "$only_swift" = 1 ] && [ "$#" -eq 1 ] && [ "$1" = --swift ]; then
    # `--swift` alone means "the defaults plus swift".
    langs="rust python javascript-typescript actions swift"
fi
case " $langs " in
    *" swift "*) [ "$(uname -s)" = Darwin ] || die "swift needs macOS (WidgetKit SDK)" ;;
esac

mkdir -p "$out" || die "cannot create $out"
src="$out/src"
rm -rf "$src"
mkdir -p "$src"
echo "codeql-local: $(codeql version --format=terse 2>/dev/null || echo '?') -> $out"
echo "codeql-local: snapshotting the working tree"
(cd "$repo" && git ls-files -co --exclude-standard | grep -v '^\.claude/worktrees/' |
    while IFS= read -r f; do [ -f "$f" ] && printf '%s\n' "$f"; done |
    tar -cf - -T -) | (cd "$src" && tar -xf -) || die "snapshot failed"

total=0
failed=0
summary=""
for lang in $langs; do
    pack=$lang
    [ "$lang" = javascript-typescript ] && pack=javascript
    db="$out/db-$lang"
    sarif="$out/$lang.sarif"
    csv="$out/$lang.csv"
    suite="codeql/$pack-queries:codeql-suites/$pack-code-scanning.qls"
    start=$(date +%s)
    echo "codeql-local: [$lang] creating database"
    if [ "$lang" = swift ]; then
        # SwiftPM's manifest sandbox cannot spawn the manifest binary under
        # the CodeQL tracer ("Bad CPU type"), so the traced build runs without it.
        set -- --source-root="$src/apps/desktop/widgets/macos" \
            --command="swift build --disable-sandbox --scratch-path $out/swift-build"
        rm -rf "$out/swift-build"
    else
        set -- --source-root="$src" --build-mode=none
    fi
    if ! codeql database create "$db" --language="$lang" --overwrite "$@" >"$out/$lang.log" 2>&1; then
        echo "codeql-local: [$lang] database creation failed; see $out/$lang.log" >&2
        [ "$lang" = swift ] && echo "codeql-local: [swift] needs full Xcode selected (sudo xcode-select -s /Applications/Xcode.app/Contents/Developer) with its license accepted; Command Line Tools alone fail with libxcrun or \"Bad CPU type\" errors" >&2
        failed=1
        summary="$summary
  $lang: ERROR (database create, $out/$lang.log)"
        continue
    fi
    echo "codeql-local: [$lang] analyzing ($suite)"
    if ! codeql database analyze "$db" "$suite" --download --format=sarif-latest \
        --output="$sarif" >>"$out/$lang.log" 2>&1 ||
        ! codeql database interpret-results "$db" "$suite" --format=csv \
            --output="$csv" >>"$out/$lang.log" 2>&1; then
        echo "codeql-local: [$lang] analysis failed; see $out/$lang.log" >&2
        failed=1
        summary="$summary
  $lang: ERROR (analyze, $out/$lang.log)"
        continue
    fi
    secs=$(($(date +%s) - start))
    count=$(grep -c . "$csv" || true)
    total=$((total + count))
    summary="$summary
  $lang: $count finding(s) in ${secs}s ($sarif)"
    if [ "$count" -gt 0 ]; then
        # CSV columns: name, description, severity, message, path, line, ...
        # A tiny quote-aware splitter keeps this free of jq/python.
        awk -v lang="$lang" '
            {
                n = 0; f = ""; q = 0
                for (i = 1; i <= length($0); i++) {
                    c = substr($0, i, 1)
                    if (q) { if (c == "\"") { if (substr($0, i + 1, 1) == "\"") { f = f c; i++ } else q = 0 } else f = f c }
                    else if (c == "\"") q = 1
                    else if (c == ",") { col[++n] = f; f = "" }
                    else f = f c
                }
                col[++n] = f
                printf "    [%s] %s (%s) %s:%s\n", lang, col[1], col[3], substr(col[5], 2), col[6]
            }' "$csv"
    fi
done

[ "$keep" = 1 ] || rm -rf "$src"
echo "codeql-local: summary$summary"
[ "$failed" = 0 ] || exit 1
[ "$total" = 0 ] || exit 3
exit 0
