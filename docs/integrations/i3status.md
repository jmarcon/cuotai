# i3bar and sway

[`i3status.sh`](../../examples/integrations/i3status.sh) prints one block object of the [i3bar protocol](https://i3wm.org/docs/i3bar-protocol.html). Add it to the block array your `status_command` wrapper already emits (for example the wrapper around `i3status`, `i3blocks` or `bumblebee-status`), or run it from a small loop of your own. See the [index](README.md) for requirements and the environment variables.

**Prefer `cuotai --status-line --from-server`** for a one-line bar: it reads the same running server, takes the token from its file, and never puts the token on a command line or sends it through a proxy, and sends it only after the server proved it holds it. The script below keeps the token off command lines and proxies too, but cannot check the server first: if another program holds the port while your server is down, it receives the token ([details](../local-server.md#who-receives-the-token)).

```sh
# one block, refreshed by your wrapper every 30 seconds
CUOTAI_TOKEN_FILE=$HOME/.local/share/cuotai/serve/token /path/to/i3status.sh
```

Output with a week at 78% (`limits-busy.json`):

```json
{"name":"cuotai","full_text":"Cuotai 78% Personal","color":"#ffb86c"}
```

The colour is `#50fa7b` (ok), `#ffb86c` (warning), `#ff5555` (critical or outage) or `#888888` with `cuotai offline` when the server cannot be read.

## Without a server: `--status-line`

A block that runs `cuotai --status-line` and wraps the text in the i3bar JSON line needs no server, at the price of one query cycle per call (use 120 seconds or more). See the [tmux page](tmux.md#without-a-server---status-line) for the options and exit codes.
