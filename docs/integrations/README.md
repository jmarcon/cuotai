# Status-bar integrations

Recipes that show Cuotai's tightest quota in a status bar. Each reads `GET /v1/limits` from the local server (`cuotai --serve`, or the desktop app with the local server on; see [`local-server.md`](../local-server.md)) with `curl` and `jq`, and prints what the bar expects. The scripts live in [`examples/integrations/`](../../examples/integrations) and run in CI against fixture bodies, so a contract change that breaks a recipe fails the build.

| Tool | Page | Script |
|---|---|---|
| Waybar | [waybar.md](waybar.md) | `waybar.sh` |
| SketchyBar | [sketchybar.md](sketchybar.md) | `sketchybar.sh` |
| tmux | [tmux.md](tmux.md) | `tmux.sh` |
| i3bar / sway | [i3status.md](i3status.md) | `i3status.sh` |

**Prefer `cuotai --status-line --from-server`** for a one-line bar: it reads the same running server, takes the token from its file, never puts the token on a command line or sends it through a proxy, and sends it only after the server proved it holds it. The scripts' `curl` helper cannot make that check: if another program holds the port while your server is down, it receives the token ([details](../local-server.md#who-receives-the-token)). Its options and exit codes are on the [tmux page](tmux.md#without-a-server---status-line). The scripts below are for bars that want the colour classes or the reset time.

Claude Code's own status line is a different integration: Claude Code feeds Cuotai, and `cuotai --statusline-claude` prints Claude's windows plus the tightest other account in its footer. See [claude-code.md](claude-code.md).

## What they read

All of them need Cuotai 0.86.5 or later on the server side, whose `/v1/limits` carries `summary.tightest.remaining_ratio`, `level` and `label` (see [`CHANGELOG.md`](../../CHANGELOG.md)); an older server reads as "no data". `cuotai --status-line` needs 0.81.0 or later, and `--from-server` needs 0.86.5 on both sides. The scripts use only `summary.tightest` (`label`, `remaining_ratio`, `level`, `resets_at`), `summary.worst_class` and `summary.worst_incident`, all documented in [`export-v1.md`](../export-v1.md#limits-summary-limits). Treat an unknown `worst_class` or incident state as "other", as the contract policy says.

## Configuration

`common.sh` is shared by the scripts: copy it next to them, since each script sources it from its own folder. The four scripts are executable; run them directly. `common.sh` reads three environment variables:

- `CUOTAI_URL`: the server address, `http://127.0.0.1:6738` by default.
- `CUOTAI_URL`: only a loopback `http://127.0.0.1:PORT`, `http://[::1]:PORT` or `http://localhost:PORT` (read as `127.0.0.1`) is accepted; anything else reads as offline.
- `CUOTAI_TOKEN_FILE` or `CUOTAI_TOKEN`: `cuotai --serve` requires a bearer token by default and keeps it in a `0600` file at `<OS local data dir>/cuotai/serve/token` (see [`local-server.md`](../local-server.md)); point `CUOTAI_TOKEN_FILE` at it. Never put the token itself in a bar configuration or on a command line.
- The scripts hand the token to `curl` on standard input (`-K -`), so it never shows in `ps`, and run `curl` with `--noproxy '*'`, `--proto '=http'` and no redirects, so a proxy in `http_proxy` or `ALL_PROXY` never receives it. If you write your own script, copy that form from [`local-server.md`](../local-server.md#from-the-command-line-cuotai---serve), never `curl -H "Authorization: Bearer …"`.
- The scripts never start a query: the server only publishes what its refresh cycle already read, and a bar polling every 30 seconds adds no provider traffic.

## What they print

The share **left** of the tightest account's binding limit and its name, `45% left · Personal`, as the menu bar, the dashboard and `cuotai --status-line` show it. The name is the account's own, or its provider's when it has none or its name looks like an e-mail address. The colours follow the menu bar's levels (`summary.tightest.level`).

| Severity | When |
|---|---|
| `ok` | 70% left or more, and no incident |
| `warning` | under 70% left, or a degraded provider |
| `critical` | under 30% left, or some account is exhausted |
| `outage` | a partial or major outage at a provider |
| `offline` | the server could not be read (the script still exits 0, so the bar keeps running) |
