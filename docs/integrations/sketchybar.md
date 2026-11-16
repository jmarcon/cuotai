# SketchyBar

An item whose plugin is [`sketchybar.sh`](../../examples/integrations/sketchybar.sh). SketchyBar passes the item name in `$NAME` and the script sets its label and colour. See the [index](README.md) for requirements and the environment variables.

**Versions.** The script needs Cuotai vNEXT or later on the server side (it reads `summary.tightest.level` and `label`). `cuotai --status-line` needs 0.81.0 or later, and `--from-server` needs vNEXT on both sides (the server proves itself before the token is sent). Copy [`common.sh`](../../examples/integrations/common.sh) next to the script: the script sources it from its own folder.

**Prefer `cuotai --status-line --from-server`** for a one-line bar: it reads the same running server, takes the token from its file, and never puts the token on a command line or sends it through a proxy, and sends it only after the server proved it holds it. The script below keeps the token off command lines and proxies too, but cannot check the server first: if another program holds the port while your server is down, it receives the token ([details](../local-server.md#who-receives-the-token)).

```sh
# ~/.config/sketchybar/sketchybarrc
sketchybar --add item cuotai right \
           --set cuotai update_freq=30 \
                        script="CUOTAI_TOKEN_FILE=$HOME/Library/Application\ Support/cuotai/serve/token /path/to/sketchybar.sh"
```

What the script runs with the fixture `limits-busy.json` (45% of a week left):

```text
sketchybar --set cuotai label=45% left · Personal label.color=0xffffb86c
```

The colour is `0xff50fa7b` (ok), `0xffffb86c` (warning), `0xffff5555` (critical or outage) or `0xff888888` with the label `Cuotai offline` when the server cannot be read.

## Without a server: `--status-line`

```sh
sketchybar --add item cuotai right \
  --set cuotai update_freq=120 script='sketchybar --set cuotai label="$(cuotai --status-line)"'
```

It queries once per call. Each call queries the selected accounts (all of them without `--account`), so pick only the ones the bar needs. An AWS Bedrock account is spared: Cost Explorer bills each request, so within its refresh interval (one hour by default, never under 15 minutes) a call reuses the reading an earlier call recorded, from any process. Other providers are queried on every call. See the [tmux page](tmux.md#without-a-server---status-line) for the options and exit codes.
