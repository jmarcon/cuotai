# Advanced usage: configuration, command line and automation

This page collects the technical side of Cuotai that the README leaves out: the accounts file, the command-line modes for scripts and status bars, the JSON contracts, events and hooks, script extensions, and the local cost and history commands. The interactive terminal dashboard and its shortcuts are in [`tui.md`](tui.md); building from source is in [`building.md`](building.md).

## The accounts file

Accounts live in a TOML file; the desktop app and the terminal dashboard edit it for you, and both preserve unrelated content and comments and refuse to overwrite a file that changed on disk behind their back. A commented reference of every `[[accounts]]` entry and parameter is in [`../cuotai.example.toml`](../cuotai.example.toml).

- **Where it is.** The terminal and the command line look at `--config`, then `CUOTAI_CONFIG`, then an existing `./cuotai.toml`, then `<OS config dir>/cuotai/config.toml`. The desktop app uses `CUOTAI_CONFIG`, the file chosen in **Settings › Advanced › Files**, then the OS path, and never reads `./cuotai.toml`. Details and exit codes are in [`tui.md`](tui.md#configuration-precedence-and-exit-codes).
- **Credentials are never in the file.** An account stores the **name** of an environment variable (`credential_env = "OPENAI_ADMIN_KEY"`) or a reference to an entry in the system vault (OAuth sign-ins and pasted web sessions). See [`oauth.md`](oauth.md) for the native logins and [`providers.md`](providers.md) for each provider's credential, default variable, authentication caveats and per-query cost warnings. Cuotai does not load `.env` files.
- **Intervals.** `refresh_seconds` is the global default (300 s); an account can set its own (minimum 30 s; AWS Bedrock, which bills per query, never below 300 s).
- **Groups and the best account.** `group = "coding"` on an account (a slug of up to 32 lowercase letters, digits, `-` or `_`) says it can replace the other accounts with the same group; an optional `[[groups]]` table (`id`, `label`, `order`) names and orders them. The best account now of each group of two or more appears as `best: [{ group, account }]` in `--json-v1` and `/v1/limits`, and each account carries its `group` ([`export-v1.md`](export-v1.md)). Accounts without a group are compared only with the other ungrouped accounts of their provider (`provider:<slug>`). Advice only.

## Quick command-line queries

Run these without opening the interactive interface (`cuotai` is the installed binary; from a source checkout use `cargo run -p cuotai-tui --` instead):

```sh
# List the 36 available providers
cuotai --list-providers

# Human-readable summary in the terminal (single run)
cuotai --config cuotai.toml --once

# Query one account and print a problem report (English text and a prefilled
# GitHub issue link). It names the failing JSON field and its types, never an
# account name, e-mail, credential or response value; nothing is sent.
cuotai --config cuotai.toml --report-problem ACCOUNT

# Maintainers: the local contract canary compares each provider's answer shape
# with the previous run (see docs/canary.md). Exit 2 means drift.
cuotai --config cuotai.toml --canary

# Structured JSON snapshot for scripts and status bars
cuotai --config cuotai.toml --json

# The versioned export::v1 contract (class, age, binding metric, pace, access level)
cuotai --config cuotai.toml --json-v1

# Read-only local server on 127.0.0.1:6738, refreshed in the background (Ctrl+C stops it)
cuotai --config cuotai.toml --serve
curl -s http://127.0.0.1:6738/v1/limits

# One NDJSON event per real transition (threshold crossed, exhausted, reset, failure…)
cuotai --config cuotai.toml --watch --jsonl | jq -c 'select(.event == "quota_exhausted")'

# Recorded history of every account (or one), without querying providers
cuotai --config cuotai.toml --history "Personal Codex" --history-days 30
```

`--once`, `--json` and `--json-v1` exit `0` when every account is ready, `2` when an account failed or lacks configuration, and `1` for local errors.

## JSON output and the export contract

In the JSON output, each metric carries a `kind` (`quota`, `rate_limit`, `balance`, `credits`, `spend` or `usage`) and a `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other` or `null`), classified by the provider adapter. Consumers do not need to parse `label` or `period` to tell quotas, rate limits and windows apart.

`cuotai --json-v1` prints the versioned `export::v1` contract, which Rust integrations can also build from `cuotai-core`: the last known state of every account with its age, freshness, preserved stale reading, binding metric, pace and usage access level, with no credentials, credential variable names or e-mails, and `null` for anything unknown. See [`export-v1.md`](export-v1.md).

## Local server

`cuotai --serve [--port N] [--token-file FILE | --no-token]` (or, with the core's `serve` feature, `serve::LocalServer`) publishes the export read-only on `127.0.0.1:6738` (`/health`, `/v1/snapshot`, `/v1/accounts/{key}`, `/v1/limits`) from the state the refresh cycle already holds, never querying a provider per request, with DNS-rebinding and browser protection and a bearer token required by default (created on the first start in `<OS local data dir>/cuotai/serve/token`, readable only by you; `--no-token` opts out and lets any local user read); it is off until you start it, and stops cleanly on `SIGINT`/`SIGTERM`. The desktop app runs the same server from **Settings › Advanced › Integrations**. See [`local-server.md`](local-server.md).

## Events and hooks

`cuotai --watch [--interval S] [--events LIST] [--jsonl]` keeps the same refresh cycle in the foreground and emits one event per real transition between consecutive readings (a quota crossed 75% or 90%, ran out, recovered or reset, an account was blocked, a query failed or recovered, authentication is needed, usage changed), never repeating a condition that persists and treating the first reading of each account as its baseline; `--jsonl` writes NDJSON on stdout with every message on stderr, and `--serve` can share the same cycle. Payloads carry no credentials, URLs or raw error messages. See [`events.md`](events.md).

`cuotai --watch --hooks` also runs your own commands on those events, from a `hooks.toml` next to the accounts' TOML (off by default): each rule lists its events, optional provider/account/threshold filters and a command as an argument list, never a shell string. The exact commands run only after you confirm them in a terminal, again after any change; a hook gets the event as JSON on stdin and in `CUOTAI_*` variables, an environment reduced to `PATH`, `HOME` and `LANG` (no credentials), a time limit that kills it, bounded output and concurrency, and its failure never stops the watch. See [`events.md`](events.md#hooks-cuotai---watch---hooks). The desktop app edits the same rules in **Settings › Advanced › Integrations › Hooks**.

## Script extensions

Script extensions (PRD 0017) add your own usage sources without recompiling Cuotai: a folder under `<OS local data dir>/cuotai/extensions/<id>/` holds a `manifest.toml` (id, name, version, `schema = 1`, the command as an argument list whose program is a file inside the folder, the interval — 60 s at least — and time limit, and the **names** of the environment variables the extension asks for) and a local program that prints one JSON object (`{ "schema": 1, "status", "metrics", "notes" }`) to stdout. Every extension is born disabled: `cuotai --extension-check <id>` lists the exact command, the directory, the requested variables and the interval, (and, for a script, its `#!` interpreter) and runs only after you type the confirmation — and only while the whole extension folder keeps the SHA-256 recorded at the approval, so any change to any of its files disables it again until you approve it anew (on macOS and Linux a folder other users can write is refused). The run has no shell, a closed stdin, the extension's folder as working directory, an environment reduced to `PATH`, `HOME`, `LANG` and the approved variables (Cuotai never hands it credentials, and its stderr never reaches a report), a time limit that kills the whole process tree, bounded output and at most two extensions at once. **Cuotai does not restrict the extension's network or files — it runs with your user's powers, so review the code before approving it** — and Cuotai never installs, downloads or updates an extension. Exit codes: `0` when the reading is ready, `2` when the run failed or the output broke the contract, `1` for local errors; `--json` prints the same result as JSON. The full contract is in [`extensions.md`](extensions.md).

## Local cost from the command line

`cuotai --cost-enable claude_code|codex_cli [--cost-path DIR]` turns on reading that tool's local session logs (off by default) after printing exactly which directories are read (by default those of `CLAUDE_CONFIG_DIR`/`~/.config/claude` and `~/.claude`, or `CODEX_HOME`/`~/.codex`) and that only the time, the model, the token counts and an anonymous session digest are extracted, and asking you to confirm in a terminal; conversation content is never read into the index. `--cost-sources [--json]` lists the sources and what they read, `--cost-scan [--json]` reads what grew now (the TUI and the desktop app do it in the background), `--cost-period` and `--cost-retention-days` set the period and the retention, and `--cost-disable SOURCE` or `--cost-erase` remove everything derived. The private index lives in `<OS local data dir>/cuotai/cost/cost.sqlite3`. See [`tui.md`](tui.md#configuration-precedence-and-exit-codes).

`cuotai --cost [--days N] [--json]` reads what grew in the logs of the sources that are on, then prints the estimate at API prices in USD (not a charge): per source today, yesterday and the saved period (or the last `N` days, 1 to 730), by model and by day, with models without a known price shown as cost unknown, never zero; exit `0`, `2` when no source is on or one has no logs or no access, `1` for local errors. The TUI shows the same section in the details of Claude and ChatGPT / Codex accounts, the desktop shows a "Local usage" block on those cards with the breakdown in the details (Settings › Advanced › Local cost also chooses each source's directories and shows the prices in effect), and `--json-v1`/`--serve` publish it as `local_cost` in `export::v1`.

`cuotai --cost-plan ACCOUNT PRICE CURRENCY [DAY]` stores what you pay for a plan (`ACCOUNT` is `provider:name`, for example `claude:personal`; `DAY` 1 to 28 is the cycle start, default 1) in the machine-local cost settings, never in the TOML, and `--cost` then prints, per source, "estimate at API prices of the current cycle next to the plan price". A ratio is shown only when both are in one currency (or the plan is in your display currency, "at your rate"); two currencies are never summed and several accounts of one source give one combined line. `--cost-plan-clear ACCOUNT` removes it. `cuotai --cost-currency BRL 5.42` (1 USD = 5.42 BRL) sets a display-only second currency at a rate you typed (never fetched; `--cost-currency off` removes it): `--cost` adds "≈ BRL … at your rate (set N days ago)" under the total, asking you to update the rate after 30 days. Totals, `--json` and `export::v1` stay in USD. `--cost` also says "Prices from N days ago" from 60 days after the price table's date and lists the models without a price; `cuotai --cost-prices --template MODEL [MODEL…]` prints a `pricing.json` fragment with those models and `null` prices to fill in (it never guesses a price).

`cuotai --cost-prices [--json]` lists the API prices the local cost estimate uses, in USD per million tokens (input, output, cache read, cache write), with the date and source of the table embedded in this build; nothing is ever downloaded. To review or change a price, write your own `pricing.json` beside the local cost preferences (`<OS local data dir>/cuotai/cost/pricing.json`, e.g. `~/Library/Application Support/cuotai/cost/pricing.json` on macOS) in the same format; each model you list wins over the embedded price, and an invalid file is ignored as a whole (`--cost-prices` says why and exits `1`):

```json
{
  "currency": "USD",
  "effective": "2026-10-21",
  "source": "my contract",
  "models": {
    "claude-haiku-5-5": { "input": 0.5, "output": 2.5, "cache_read": 0.05, "cache_write": 0.625 }
  }
}
```

A model name with a date suffix (`claude-sonnet-4-5-20250929`) uses the price of the undated name; a model in neither table is shown as "cost unknown", never as zero, and every figure is an estimate at API prices, not a charge.

## Recorded history

Every successful reading (TUI, desktop app, `--once`, `--json`, `--json-v1`, `--serve` and `--watch`) also enters a local measurement history on this computer only: `<OS local data dir>/cuotai/history/history.sqlite3`, private to your user, holding numbers and window metadata, never credentials, account names or provider text. The history keeps 90 days by default (`--history-days 7…365` on the command line, **Settings › Data & privacy** on the desktop), keeps one sample per hour after 7 days (each window's peak stays) and stays under 64 MiB, warning when it had to delete old samples. `--history [ACCOUNT]` prints the recorded series (`--json` for scripts; `--history-days` picks the days shown, 7 by default). How the desktop app draws it is in [`gui-features.md`](gui-features.md#account-details-and-history).

## Environment variables

| Variable | Effect |
|---|---|
| `CUOTAI_CONFIG` | Accounts file to use (terminal and desktop). |
| `CUOTAI_LANG` | Locale for the texts delivered by the core in the terminal (`en-US`, `es`, `it`, `pt-BR`...). |
| `CUOTAI_PACE_UNIT` | `points` (default) or `percent`: how the terminal shows the pace deviation (`--pace-unit` wins). |
| `CUOTAI_STATUS=1` | Show providers' public status in the terminal and `--once` / `--json` (`--no-status` always wins). |
| `CUOTAI_NO_ANIMATIONS=1`, `CUOTAI_ANIMATIONS=1` | Turn terminal animations off or back on (see [`tui.md`](tui.md#animations)). |
| `CUOTAI_USER_DIR` | Folder for user themes and languages (desktop). |
| `CUOTAI_UPDATER=off` | Turn the desktop in-app updater off (for example for a package-manager install). |
