# Waybar

A custom module that runs [`waybar.sh`](../../examples/integrations/waybar.sh) every 30 seconds. See the [index](README.md) for requirements and the environment variables.

```jsonc
// ~/.config/waybar/config
"custom/cuotai": {
  "exec": "CUOTAI_TOKEN_FILE=$HOME/.local/share/cuotai/serve/token /path/to/waybar.sh",
  "return-type": "json",
  "interval": 30
}
```

```css
/* ~/.config/waybar/style.css */
#custom-cuotai.warning  { color: #ffb86c; }
#custom-cuotai.critical { color: #ff5555; }
#custom-cuotai.outage   { color: #ff5555; }
#custom-cuotai.offline  { color: #888888; }
```

Output with a Claude week at 78% (the fixture `limits-busy.json`):

```json
{"text":"Cuotai 78% Personal","class":"warning","tooltip":"Resets at 2026-10-24T09:00:00Z"}
```

Output when the account is exhausted (`limits-blocked.json`):

```json
{"text":"Cuotai 100% Personal","class":"critical","tooltip":"Resets at 2026-10-20T14:00:00Z"}
```

With the server unreachable the module shows `Cuotai offline` with the `offline` class.
