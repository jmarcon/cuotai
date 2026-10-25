# SketchyBar

An item whose plugin is [`sketchybar.sh`](../../examples/integrations/sketchybar.sh). SketchyBar passes the item name in `$NAME` and the script sets its label and colour. See the [index](README.md) for requirements and the environment variables.

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
