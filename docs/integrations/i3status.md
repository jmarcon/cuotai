# i3bar and sway

[`i3status.sh`](../../examples/integrations/i3status.sh) prints one block object of the [i3bar protocol](https://i3wm.org/docs/i3bar-protocol.html). Add it to the block array your `status_command` wrapper already emits (for example the wrapper around `i3status`, `i3blocks` or `bumblebee-status`), or run it from a small loop of your own. See the [index](README.md) for requirements and the environment variables.

**Versions.** The script needs Cuotai 0.86.5 or later on the server side (it reads `summary.tightest.level` and `label`). `cuotai --status-line` needs 0.81.0 or later, and `--from-server` needs 0.86.5 on both sides (the server proves itself before the token is sent). Copy [`common.sh`](../../examples/integrations/common.sh) next to the script: the script sources it from its own folder.

**Prefer `cuotai --status-line --from-server`** for a one-line bar: it reads the same running server, takes the token from its file, and never puts the token on a command line or sends it through a proxy, and sends it only after the server proved it holds it. The script below keeps the token off command lines and proxies too, but cannot check the server first: if another program holds the port while your server is down, it receives the token ([details](../local-server.md#who-receives-the-token)).

```sh
# one block, refreshed by your wrapper every 30 seconds
CUOTAI_TOKEN_FILE=$HOME/.local/share/cuotai/serve/token /path/to/i3status.sh
```

Output with 45% of a week left (`limits-busy.json`):

```json
{"name":"cuotai","full_text":"Cuotai 45% left · Personal","color":"#ffb86c"}
```

The colour is `#50fa7b` (ok), `#ffb86c` (warning), `#ff5555` (critical or outage) or `#888888` with `Cuotai offline` when the server cannot be read.

## Without a server: `--status-line`

A block that runs `cuotai --status-line` and wraps the text in the i3bar JSON line needs no server, at the price of one query cycle per call (use 120 seconds or more). Each call queries the selected accounts (all of them without `--account`), so pick only the ones the bar needs. An AWS Bedrock account is spared: Cost Explorer bills each request, so within its refresh interval (one hour by default, never under 15 minutes) a call reuses the reading an earlier call recorded, from any process. Other providers are queried on every call. For example, as the whole `status_command` of i3bar or swaybar:

```sh
#!/bin/sh
# ~/.config/i3/cuotai-bar.sh; in the i3 config: bar { status_command ~/.config/i3/cuotai-bar.sh }
echo '{"version":1}'
echo '['
while :; do
  printf '[{"name":"cuotai","full_text":%s}],\n' "$(cuotai --status-line | jq -R .)"
  sleep 120
done
```

See the [tmux page](tmux.md#without-a-server---status-line) for the options and exit codes.
