# Waybar

A custom module that runs [`waybar.sh`](../../examples/integrations/waybar.sh) every 30 seconds. See the [index](README.md) for requirements and the environment variables.

**Versions.** The script needs Cuotai vNEXT or later on the server side (it reads `summary.tightest.level` and `label`). `cuotai --status-line` needs 0.81.0 or later, and `--from-server` needs vNEXT on both sides (the server proves itself before the token is sent). Copy [`common.sh`](../../examples/integrations/common.sh) next to the script: the script sources it from its own folder.

**Prefer `cuotai --status-line --from-server`** for a one-line bar: it reads the same running server, takes the token from its file, and never puts the token on a command line or sends it through a proxy, and sends it only after the server proved it holds it. The script below keeps the token off command lines and proxies too, but cannot check the server first: if another program holds the port while your server is down, it receives the token ([details](../local-server.md#who-receives-the-token)).

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

Output with 45% of a Claude week left (the fixture `limits-busy.json`):

```json
{"text":"Cuotai 45% left · Personal","class":"warning","tooltip":"Resets at 2026-10-24T09:00:00Z"}
```

Output when the account is exhausted (`limits-blocked.json`):

```json
{"text":"Cuotai 0% left · Personal","class":"critical","tooltip":"Resets at 2026-10-20T14:00:00Z"}
```

With the server unreachable the module shows `Cuotai offline` with the `offline` class. Waybar reads `text` as Pango markup, so the script escapes `&`, `<` and `>` in the name itself: leave `"escape"` off in the module, or the name shows `&amp;`.

## Without a server: `--status-line`

A `custom` module can run `cuotai --status-line` directly. It queries once per call, so use a long `interval`. Each call queries the selected accounts (all of them without `--account`), so pick only the ones the bar needs. An AWS Bedrock account is spared: Cost Explorer bills each request, so within its refresh interval (one hour by default, never under 15 minutes) a call reuses the reading an earlier call recorded, from any process. Other providers are queried on every call.

```json
"custom/cuotai": { "exec": "cuotai --status-line", "interval": 120 }
```

The line is plain text such as `22% · Claude` (the provider; with `--account` the account's name), and it prints `—` while no share is known. See the [tmux page](tmux.md#without-a-server---status-line) for the options.
