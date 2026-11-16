# SketchyBar

An item whose plugin is [`sketchybar.sh`](../../examples/integrations/sketchybar.sh). SketchyBar passes the item name in `$NAME` and the script sets its label and colour. See the [index](README.md) for requirements and the environment variables.

**Prefer `cuotai --status-line --from-server`** for a one-line bar: it reads the same running server, takes the token from its file, and never puts the token on a command line or sends it through a proxy, and sends it only after the server proved it holds it. The script below keeps the token off command lines and proxies too, but cannot check the server first: if another program holds the port while your server is down, it receives the token ([details](../local-server.md#who-receives-the-token)).

```sh
# ~/.config/sketchybar/sketchybarrc
sketchybar --add item cuotai right \
           --set cuotai update_freq=30 \
                        script="CUOTAI_TOKEN_FILE=$HOME/Library/Application\ Support/cuotai/serve/token /path/to/sketchybar.sh"
```

What the script runs with the fixture `limits-busy.json` (a week at 78%):

```text
sketchybar --set cuotai label=78% Personal label.color=0xffffb86c
```

The colour is `0xff50fa7b` (ok), `0xffffb86c` (warning), `0xffff5555` (critical or outage) or `0xff888888` with the label `offline` when the server cannot be read.

## Without a server: `--status-line`

```sh
sketchybar --add item cuotai right \
  --set cuotai update_freq=120 script='sketchybar --set cuotai label="$(cuotai --status-line)"'
```

It queries once per call; see the [tmux page](tmux.md#without-a-server---status-line) for the options and exit codes.
