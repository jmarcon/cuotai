# tmux

A status segment from [`tmux.sh`](../../examples/integrations/tmux.sh), coloured with tmux style codes. See the [index](README.md) for requirements and the environment variables.

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
