# tmux

A status segment from [`tmux.sh`](../../examples/integrations/tmux.sh), coloured with tmux style codes. See the [index](README.md) for requirements and the environment variables.

**Prefer `cuotai --status-line --from-server`** for a one-line bar: it reads the same running server, takes the token from its file, and never puts the token on a command line or sends it through a proxy. The script below keeps the token off command lines and proxies too.

```tmux
# ~/.tmux.conf
set -g status-interval 30
set -g status-right '#(CUOTAI_TOKEN_FILE=$HOME/.local/share/cuotai/serve/token /path/to/tmux.sh) %H:%M'
```

Output with a week at 78% (`limits-busy.json`):

```text
#[fg=yellow]78% Personal#[default]
```

Exhausted (`limits-blocked.json`):

```text
#[fg=red]100% Personal#[default]
```

With the server unreachable the segment reads `cuotai offline` in grey.

## Without a server: `--status-line`

`cuotai --status-line` runs one refresh and prints the same line the desktop menu bar shows, so no server and no `jq` are needed. It costs one query cycle per call, so keep `status-interval` generous (60 seconds or more), or use the server recipe above to avoid a second query cycle.

```tmux
set -g status-interval 120
set -g status-right '#(cuotai --status-line) %H:%M'
```

With a server running, `#(cuotai --status-line --from-server)` prints the same line from it instead, without a query cycle, so a short `status-interval` is fine.

`--style logos_percent` lists every account (`C 22% · O 80%`), `--account ID` (repeatable) picks accounts and `--color` adds ANSI colour only on a terminal with `NO_COLOR` unset. Exit codes: 0 all accounts ready, 2 an account failed, 1 a local error.
