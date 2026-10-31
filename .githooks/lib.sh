# Sourced by the hooks in this directory: the one place that finds cargo and
# calls `cargo xtask preflight`, which holds every check. Hooks are opt-in:
#
#   git config core.hooksPath .githooks
#
# Git resolves that relative path inside each worktree, so linked worktrees
# run their own copy of these files.

# Graphical Git clients often start without the shell profile's PATH.
if ! command -v cargo >/dev/null 2>&1 && [ -x "$HOME/.cargo/bin/cargo" ]; then
    PATH="$HOME/.cargo/bin:$PATH"
    export PATH
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo "preflight: cargo not found; install Rust, or bypass with --no-verify (CI still applies)." >&2
    exit 1
fi

preflight() {
    exec cargo run -q --locked -p xtask -- preflight "$@"
}
