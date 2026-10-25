# Status-bar integrations

Recipes that show Cuotai's tightest quota in a status bar. Each reads `GET /v1/limits` from the local server (`cuotai --serve`, or the desktop app with the local server on; see [`local-server.md`](../local-server.md)) with `curl` and `jq`, and prints what the bar expects. The scripts live in [`examples/integrations/`](../../examples/integrations) and run in CI against fixture bodies, so a contract change that breaks a recipe fails the build.

| Tool | Page | Script |
|---|---|---|
| Waybar | [waybar.md](waybar.md) | `waybar.sh` |
| SketchyBar | [sketchybar.md](sketchybar.md) | `sketchybar.sh` |
| tmux | [tmux.md](tmux.md) | `tmux.sh` |
| i3bar / sway | [i3status.md](i3status.md) | `i3status.sh` |

## What they read

All of them need a Cuotai whose `/v1/limits` carries the `summary` object and the per-account `incident` (PRD 0031 RF4; the first release after 0.80.0, see [`CHANGELOG.md`](../../CHANGELOG.md)). They use only `summary.tightest` (`name`, `used_ratio`, `resets_at`), `summary.worst_class` and `summary.worst_incident`, all documented in [`export-v1.md`](../export-v1.md#limits-summary-limits). Treat an unknown `worst_class` or incident state as "other", as the contract policy says.

## Configuration

`common.sh` is shared by the scripts and reads three environment variables:

- `CUOTAI_URL`: the server address, `http://127.0.0.1:6738` by default.
- `CUOTAI_TOKEN_FILE` or `CUOTAI_TOKEN`: `cuotai --serve` requires a bearer token by default and keeps it in a `0600` file at `<OS local data dir>/cuotai/serve/token` (see [`local-server.md`](../local-server.md)); point `CUOTAI_TOKEN_FILE` at it. Never put the token in a world-readable bar configuration.
- The scripts never start a query: the server only publishes what its refresh cycle already read, and a bar polling every 30 seconds adds no provider traffic.

## Severity

| Severity | When |
|---|---|
| `ok` | below 75% and no incident |
| `warning` | 75% or more, or a degraded provider |
| `critical` | 90% or more, or some account is exhausted |
| `outage` | a partial or major outage at a provider |
| `offline` | the server could not be read (the script still exits 0, so the bar keeps running) |
