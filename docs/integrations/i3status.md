# i3bar and sway

[`i3status.sh`](../../examples/integrations/i3status.sh) prints one block object of the [i3bar protocol](https://i3wm.org/docs/i3bar-protocol.html). Add it to the block array your `status_command` wrapper already emits (for example the wrapper around `i3status`, `i3blocks` or `bumblebee-status`), or run it from a small loop of your own. See the [index](README.md) for requirements and the environment variables.

```sh
# one block, refreshed by your wrapper every 30 seconds
CUOTAI_TOKEN_FILE=$HOME/.local/share/cuotai/serve/token /path/to/i3status.sh
```

Output with a week at 78% (`limits-busy.json`):

```json
{"name":"cuotai","full_text":"Cuotai 78% Personal","color":"#ffb86c"}
```

The colour is `#50fa7b` (ok), `#ffb86c` (warning), `#ff5555` (critical or outage) or `#888888` with `cuotai offline` when the server cannot be read.
