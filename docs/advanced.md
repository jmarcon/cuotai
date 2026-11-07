# Advanced usage: configuration, command line and automation

This page collects the technical side of Cuotai that the README leaves out: the accounts file, the command-line modes for scripts and status bars, the JSON contracts, events and hooks, script extensions, and the local cost and history commands. The interactive terminal dashboard and its shortcuts are in [`tui.md`](tui.md); building from source is in [`building.md`](building.md).

## The accounts file

Accounts live in a TOML file; the desktop app and the terminal dashboard edit it for you, and both preserve unrelated content and comments and refuse to overwrite a file that changed on disk behind their back. A commented reference of every `[[accounts]]` entry and parameter is in [`../cuotai.example.toml`](../cuotai.example.toml).

- **Where it is.** The terminal and the command line look at `--config`, then `CUOTAI_CONFIG`, then an existing `./cuotai.toml`, then `<OS config dir>/cuotai/config.toml`. The desktop app uses `CUOTAI_CONFIG`, the file chosen in **Settings › Advanced › Files**, then the OS path, and never reads `./cuotai.toml`. Details and exit codes are in [`tui.md`](tui.md#configuration-precedence-and-exit-codes).
- **Credentials are never in the file.** An account stores the **name** of an environment variable (`credential_env = "OPENAI_ADMIN_KEY"`) or a reference to an entry in the system vault (OAuth sign-ins and pasted web sessions). See [`oauth.md`](oauth.md) for the native logins and [`providers.md`](providers.md) for each provider's credential, default variable, authentication caveats and per-query cost warnings. Cuotai does not load `.env` files. Several accounts of one provider each need their own variable: the first uses the provider default and the others the wizard's `<DEFAULT>_<ACCOUNT NAME>` suggestion, for example `GROQ_API_KEY` and `GROQ_API_KEY_WORK`, exported separately (`export GROQ_API_KEY_WORK=...` in bash/zsh, `set -x GROQ_API_KEY_WORK ...` in fish, `$env:GROQ_API_KEY_WORK = '...'` in PowerShell) and named in `credential_env` of each account.
- **Across versions.** [`configuration.md`](configuration.md) explains how a version reads a file a newer one wrote (an unknown account is set aside, the others keep working), `cuotai --check-config` and what the editors keep.
- **Local files and downgrades.** [`local-state.md`](local-state.md) lists every file Cuotai keeps (where, its version, what a damaged or newer file does) and how to go back to an older version.
- **Intervals.** `refresh_seconds` is the global default (300 s); an account can set its own (minimum 30 s; AWS Bedrock, which bills per query, never below 300 s).
- **Groups and the best account.** `group = "coding"` on an account (a slug of up to 32 lowercase letters, digits, `-` or `_`) says it can replace the other accounts with the same group; an optional `[[groups]]` table (`id`, `label`, `order`) names and orders them. The best account now of each group of two or more appears as `best: [{ group, account }]` in `--json-v1` and `/v1/limits`, and each account carries its `group` ([`export-v1.md`](export-v1.md)). Accounts without a group are compared only with the other ungrouped accounts of their provider (`provider:<slug>`). Advice only.

## Quick command-line queries

Run these without opening the interactive interface (`cuotai` is the installed binary; from a source checkout use `cargo run -p cuotai-tui --` instead):

```sh
# List the 40 available providers
cuotai --list-providers

# Check the accounts file strictly and offline: line, column and key of the
# first problem, never its value (exit 1); --json for scripts
cuotai --config cuotai.toml --check-config

# The same catalog as JSON (schema 1, offline): slug, label key, connection
# modes, access level, metered, model-list and incident-status support
cuotai --list-providers --json

# Export the local history (CSV or JSON; one account with --history, the last
# N days with --history-days), or erase it after a typed confirmation
cuotai --history-export history.csv --history-format csv [--history ACCOUNT] [--history-days 30]
cuotai --history-erase ACCOUNT    # or: --history-erase all; --yes skips the question

# Erase every file, directory and vault entry Cuotai keeps (docs/privacy.md).
# It lists them first and asks for the confirmation word; without a terminal
# it needs --yes. The accounts TOML and hooks.toml stay unless --include-config.
cuotai --erase-all-data [--yes] [--include-config]

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

# How use spreads over the day (24h, 7d or a typical day over 30 days), from the history
cuotai --config cuotai.toml --stats --rhythm --range typical

# The weekly digest of last week (or --week current), as text, --markdown or --json
cuotai --config cuotai.toml --digest --markdown
```

`--once`, `--json` and `--json-v1` exit `0` when every account is ready, `2` when an account failed or lacks configuration, and `1` for local errors. `--serve` and `--watch` exit `0` when a signal stops them (`SIGINT`/`SIGTERM`, Ctrl+C or Ctrl+Break), whatever the accounts' state; only with `--strict-exit` do they exit `2`, when no account had a ready reading until shutdown (`--serve`) or an account's latest reading had failed (`--watch`); `1` is a local error.

## JSON output and the export contract

In the JSON output, each metric carries a `kind` (`quota`, `rate_limit`, `balance`, `credits`, `spend` or `usage`) and a `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other` or `null`), classified by the provider adapter. Consumers do not need to parse `label` or `period` to tell quotas, rate limits and windows apart.

`cuotai --json-v1` prints the versioned `export::v1` contract, which Rust integrations can also build from `cuotai-core`: the last known state of every account with its age, freshness, preserved stale reading, binding metric, pace and usage access level, with no credentials, credential variable names or e-mails, and `null` for anything unknown. See [`export-v1.md`](export-v1.md).

## Claude Code and Codex CLI feeds

A Claude account can be fed by Claude Code's own status line (`params = { query_mode = "claude_code" }`, or `claude_code_or_api` to fall back to the query when the feed is older than the refresh interval), and a ChatGPT / Codex account by Codex CLI's local session logs (`codex_cli` or `codex_cli_or_api`; it needs the Codex CLI local cost source on). Neither makes a network request or reuses a token, the reading ages from the agent's own time and a window the agent dropped stays unknown. A query-mode account can bind a feed for its live session with `params = { feed = "claude_code" }`; one account at most reads each feed. Set-up, the status-line commands and the privacy statement are in [`integrations/claude-code.md`](integrations/claude-code.md).

## Local server

`cuotai --serve [--port N] [--token-file FILE | --no-token]` (or, with the core's `serve` feature, `serve::LocalServer`) publishes the export read-only on `127.0.0.1:6738` (`/health`, `/v1/snapshot`, `/v1/accounts/{key}`, `/v1/limits`) from the state the refresh cycle already holds, never querying a provider per request, with DNS-rebinding and browser protection and a bearer token required by default (created on the first start in `<OS local data dir>/cuotai/serve/token`, readable only by you; `--no-token` opts out and lets any local user read); it is off until you start it, and stops cleanly on `SIGINT`/`SIGTERM`. The desktop app runs the same server from **Settings › Advanced › Integrations**. See [`local-server.md`](local-server.md).

## Events and hooks

`cuotai --watch [--interval S] [--events LIST] [--jsonl]` keeps the same refresh cycle in the foreground and emits one event per real transition between consecutive readings (a quota crossed 75% or 90%, ran out, recovered or reset, an account was blocked, a query failed or recovered, authentication is needed, usage changed), never repeating a condition that persists and treating the first reading of each account as its baseline; `--jsonl` writes NDJSON on stdout with every message on stderr, and `--serve` can share the same cycle. Payloads carry no credentials, URLs or raw error messages. See [`events.md`](events.md).

`cuotai --watch --hooks` also runs your own commands on those events, from a `hooks.toml` next to the accounts' TOML (off by default): each rule lists its events, optional provider/account/threshold filters and a command as an argument list, never a shell string. The exact commands run only after you confirm them in a terminal, again after any change; a hook gets the event as JSON on stdin and in `CUOTAI_*` variables, an environment reduced to `PATH`, `HOME` and `LANG` (no credentials), a time limit that kills it, bounded output and concurrency, and its failure never stops the watch. See [`events.md`](events.md#hooks-cuotai---watch---hooks). The desktop app edits the same rules in **Settings › Advanced › Integrations › Hooks**.

## Notification channels

PRD 0044 sends the notifications Cuotai shows to destinations you own: a generic webhook, an ntfy topic, a Slack incoming webhook or a Discord webhook. Up to 5 channels can exist, and each one is off until you add it. The full guide, with the payloads, is [`notifications.md`](notifications.md).

```sh
# Add a channel. The address comes from standard input or a prompt, never an argument.
cuotai --channel-add ntfy --label Phone --filter kinds=threshold,exhausted,floor
printf '%s\n' "$SLACK_WEBHOOK" | cuotai --channel-add slack --label Team --filter names=on --yes
cuotai --channel-add webhook --label Relay --sign      # prints the signing secret once
cuotai --channel-list [--json]                         # host, filters, last delivery; never the address
cuotai --channel-test <id|label>                       # exit 0 accepted, 2 refused or unreachable
cuotai --channel-log [--json]                          # the delivery log of the last 7 days
cuotai --channel-remove <id|label>                     # also deletes the address from the vault
```

`--filter` repeats: `kinds=` (comma-separated, or `all`), `accounts=` (account ids, or `all`), `min_severity=info|warning|critical` and `names=on|off|default`. The address goes to the system vault (`channel_<id>`). `desktop.json` keeps only the kind, the name, the host, the filters and the switch. `cuotai --notify` delivers to the channels what it announces, and it shares `notified.json` with the desktop, so one crossing is sent once.

## Script extensions

Script extensions (PRD 0017) add your own usage sources without recompiling Cuotai: a folder under `<OS local data dir>/cuotai/extensions/<id>/` holds a `manifest.toml` (id, name, version, `schema = 1`, the command as an argument list whose program is a file inside the folder, the interval — 60 s at least — and time limit, and the **names** of the environment variables the extension asks for) and a local program that prints one JSON object (`{ "schema": 1, "status", "metrics", "notes" }`) to stdout. Every extension is born disabled: `cuotai --extension-check <id>` lists the exact command, the directory, the requested variables and the interval, (and, for a script, its `#!` interpreter) and runs only after you type the confirmation — and only while the whole extension folder keeps the SHA-256 recorded at the approval, so any change to any of its files disables it again until you approve it anew (on macOS and Linux a folder other users can write is refused). The run has no shell, a closed stdin, the extension's folder as working directory, an environment reduced to `PATH`, `HOME`, `LANG` and the approved variables (Cuotai never hands it credentials, and its stderr never reaches a report), a time limit that kills the whole process tree, bounded output and at most two extensions at once. **Cuotai does not restrict the extension's network or files — it runs with your user's powers, so review the code before approving it** — and Cuotai never installs, downloads or updates an extension. Exit codes: `0` when the reading is ready, `2` when the run failed or the output broke the contract, `1` for local errors; `--json` prints the same result as JSON. The full contract is in [`extensions.md`](extensions.md).

## Local cost from the command line

`cuotai --cost-enable claude_code|codex_cli [--cost-path DIR]` turns on reading that tool's local session logs (off by default) after printing exactly which directories are read (by default those of `CLAUDE_CONFIG_DIR`/`~/.config/claude` and `~/.claude`, or `CODEX_HOME`/`~/.codex`) and that only the time, the model, the token counts and an anonymous session digest are extracted, and asking you to confirm in a terminal; conversation content is never read into the index. `--cost-sources [--json]` lists the sources and what they read, `--cost-scan [--json]` reads what grew now (the TUI and the desktop app do it in the background), `--cost-period` and `--cost-retention-days` set the period and the retention, and `--cost-disable SOURCE` or `--cost-erase` remove everything derived. The private index lives in `<OS local data dir>/cuotai/cost/cost.sqlite3`. See [`tui.md`](tui.md#configuration-precedence-and-exit-codes).

`cuotai --cost [--days N] [--json]` reads what grew in the logs of the sources that are on, then prints the estimate at API prices in USD (not a charge): per source today, yesterday and the saved period (or the last `N` days, 1 to 730), by model and by day, with models without a known price shown as cost unknown, never zero; exit `0`, `2` when no source is on or one has no logs or no access, `1` for local errors. The TUI shows the same section in the details of Claude and ChatGPT / Codex accounts, the desktop shows a "Local usage" block on those cards with the breakdown in the details (Settings › Advanced › Local cost also chooses each source's directories and shows the prices in effect), and `--json-v1`/`--serve` publish it as `local_cost` in `export::v1`.

### Sessions, blocks and projects

`cuotai --cost --by day|model|session|block|project [--days N] [--json]` (PRD 0042) picks the breakdown: `day` is the default above, `model` lists the span's models per source, and the other three read the index's per-request rows:

- **Sessions** (`--by session [--top N|--all]`, 20 by default): one agent conversation each, shown as "Session · 2026-10-25 14:02–16:40 · 2h 38m" with its source, its main project, tokens, requests, models and the estimate. A session has no name (its title lives in the conversation, which is never read); `cuotai --cost-label session ab12cd34 "Refactor"` labels it by the first characters of its digest (at least 4, as the list shows them), `--clear` removes the label.
- **Blocks** (`--by block`): 5-hour blocks computed from your logs, the way `ccusage` counts Claude Code's windows: a block starts at the hour of the first request after the previous one ended and lasts the source's block length (`cuotai --cost-block-hours SOURCE HOURS`, 1 to 24; 5 for Claude Code and Codex CLI, 24 for Gemini CLI by default). They are a **local estimate**: the provider's own window is shown beside the active block where Cuotai holds a reading of it (the desktop and the TUI details), never merged with it.
- **Projects** (`--by project [--top N|--all]`): the folder an agent ran in, stored only as a one-way fingerprint (the first 16 hex characters of the SHA-256 of the path; the path is never stored, logged or shown). A project shows as "Project ab12cd34" until you label it: `cuotai --cost-label project ab12cd34 "Client work"` (up to 48 characters, kept in the index, never in the TOML). `cuotai --cost-hide PREFIX` leaves a project out of the totals while it stays listed; `--cost-unhide` counts it again. Lines without a folder count under "No project". Gemini CLI records no folder, so its projects are fingerprints of Gemini's own project hash and never match the same folder seen by another tool. `cuotai --cost-folder-hint SOURCE on` keeps the last folder name of each project of that source (64 characters at most) to prefill its label; it is off by default, never exported, and `off` deletes the names kept.

`cuotai --cost-live [--source SOURCE] [--json] [--watch-interval S]` shows the active block of each source: its window, the time elapsed and left, tokens, the estimated cost, the burn rate (tokens per minute over the last 10 minutes, shown only after 3 requests and 3 minutes) and, from 20% of the block on, "At this rate ≈ $6.40 by 18:53" (never for usage without a price). With `--watch-interval S` (2 to 3600) it reads what grew and redraws in place every `S` seconds on a terminal (`q` quits), or prints one JSON object per line with `--json`. Exit codes are those of `--cost`.

Upgrading reads each followed log once more from its start to fill the per-request rows; the day totals do not change, and days whose logs are already gone keep their totals without sessions or blocks. The disclosure now names the project fingerprint, so a source turned on before asks for the confirmation once more (`cuotai --cost-enable SOURCE`) and reads nothing until then; the numbers already indexed stay. The disclosure text, verbatim:

> Only numbers are extracted: the time, the model, the token counts (input, output, cache read and cache write), an anonymous digest of the session and a one-way fingerprint of the project folder, so you can label it.
>
> Conversation content, prompts, answers, file names and project paths are never read into the index, stored or shown; the last folder name of a project is kept only if you turn that on. Nothing leaves this computer.

`export::v1` `local_cost` adds `blocks` (the last 7 days), `live` and `projects` (labels or fingerprint prefixes, never a hidden project or a folder name); see [`export-v1.md`](export-v1.md).

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

### Moving the accounts file

The history is filed under the accounts file's location, so moving or renaming the file starts a new, empty history. When the file the terminal or the desktop app opened last no longer exists, and its history holds **exactly** the accounts of the file you open now, by their stable `id` (the first key of each account once Cuotai saved the file; an account without an `id` line never matches), the TUI asks in a dialog and the desktop app in a banner: "Use that history here?". Accepting moves the readings and the journal of the old location to the new one in one step, after replacing `history.sqlite3.bak` with a copy of the database; a reading the new file already has for the same time wins. Declining, or ignoring the question, changes nothing, and it is not asked again for that move. Names and providers are never compared, and `--once`, `--json`, `--serve` and the other command-line modes never adopt a history on their own. The model cache is fetched again, and notice marks stay with the accounts' ids.

## Budgets, floors and allowance

A **budget** (PRD 0041) is your own limit on a metric the provider reports without one: a monthly API spend, a daily usage count, prepaid credits. Add `[[accounts.budgets]]` under an account (see [`cuotai.example.toml`](../cuotai.example.toml)) or use **Budgets and floors** in the desktop account dialog or the **Budgets** step of the terminal wizard. Each budget becomes one extra quota labelled **Budget · <metric>** (or your `label`) with `used`, `limit`, `remaining` and a reset at the next period boundary, so the pace, usage alerts, the projection, the binding limit, the reset notices, the history, the statistics and the events work on it unchanged. It is marked `derived: true` everywhere (the export, `--json`, the cards show "Your budget"): it is your number, never a provider limit, and Cuotai never enforces it or changes anything at the provider.

- **Choosing the metric:** `kind = "spend" | "usage" | "credits"`, optionally narrowed by `unit` and `label_contains` (matched against the metric's label, case-insensitively), or `scope = "account"` for the account's total spend.
- **Periods:** `daily`, `weekly` (`starts = "monday".."sunday"`, Monday by default), `monthly` (`starts = 1..28`, the 1st by default) or `window` (the provider's own cycle, for a metric with a reset). Boundaries are local midnight, DST included.
- **How `used` is counted:** when the provider's metric already covers the same period (a monthly spend for a monthly budget from the 1st), its own value; a provider month in another time zone is accepted with a note. Otherwise the current value minus the first reading recorded in the period, so the history must be on: until the period's first reading the budget shows "counting starts with the first reading of this period" instead of a number. A prepaid balance counts its drops; a top-up is detected and is not use.
- **Rules:** at most 8 budgets and 4 floors per account, an amount above 0 and at most 1,000,000,000, a unit that is a currency code or the metric's unit, no two budgets on the same metric and period. `cuotai --check-config` names the offending field.
- **On a breakdown row (PRD 0045 RF5.2):** add `row = { scope = "key", id = "<row id>" }` (scope `key`, `virtual_key`, `project`, `workspace` or `team`; the id is the row's `id` in `--json-v1`, never its label) to follow one key, project, workspace or team of an account with `breakdown = [...]`; the budget then becomes a quota of that row, counted over the row's own history, and alerts only when **Alert on breakdown rows** is on for the account. The desktop's **Set a budget…** on a row of the Breakdown section prefills it. The key is new in this release: an older Cuotai rejects a budget that has it.

A **floor** (`[[accounts.floors]]`) warns when a balance or credits metric drops below its `amount`: a "Balance low · $3.20 left" chip on the card (its role turns "bad" at a quarter of the floor), a line in the details and `--once`, a notification under the usual alert policy (held by quiet hours like a threshold, private texts by default) and the `balance_below_floor` / `balance_recovered` events. It fires once per crossing and re-arms above 110% of the floor or after a top-up.

The **allowance** (PRD 0041 RF4) is how much of a quota you can use per day (per hour when less than 2 days remain) and still reach the reset: "≈ 12%/day to the reset", "≈ $3.40/day left in the budget", with the recent pace beside it when known ("· recently 18%/day"). It shows in the desktop details, the TUI details, an `Allowance:` line of `--once` and `allowance` in `export::v1`; never for a 5-hour window, a rate limit or an unknown value, and only for a fresh reading.

## Usage insights: rhythm, limit changes and the weekly digest

Everything here comes from the readings already recorded on this computer (PRD 0043); nothing queries a provider.

- **Rhythm:** `cuotai --stats --rhythm [ACCOUNT] [--range 24h|7d|typical] [--json]` prints how use spread over the last 24 hours (hourly bars), the last 7 days (days off marked) or a typical day over 30 days (an hour needs 3 days of data), in percentage points of each account's tightest limit. A gap (no reading) shows `·`, never 0; `↺` marks a reset. The TUI statistics screen shows the same with `r`, and the desktop Stats view has a **Rhythm** section. Days off are `notify.days_off` in `desktop.json` (Saturday and Sunday by default; **Settings › Alerts** on the desktop).
- **Limit changes:** when a provider metric's limit changes between two readings (a promotion, a plan change, a cap someone edited), the journal records `limit_changed`, the desktop card shows a note for 48 hours ("Weekly limit changed 100 → 150 points on Oct 25") and hooks receive `limit_changed`. A notification is opt-in (**Notify when a limit changes**, `notify.limit_changes`).
- **Weekly digest:** `cuotai --digest [--week last|current] [--json|--markdown]` prints the week: limit hits, average peak, extra spent, limit changes, the heaviest and lightest day, the busiest hours, your budgets and the allowance for the coming week. Exit `0`; `2` when nothing was recorded. The desktop can also send it as a notification once a week (**Weekly digest**, off by default, Monday 09:00); one process delivers it, and a slot missed while the app was closed is delivered within 48 hours, then dropped. `--stats --json` and `/v1/stats` carry `rhythm` and `limit_changes` (additive).

## Update notice

The terminal never looks for a new version unless you ask it to. Set `CUOTAI_UPDATE_CHECK=1`, or add this to the accounts file:

```toml
[updates]
check = true
```

Then the interactive TUI and `--once`, when run on a terminal, read the same signed release manifest the desktop updater reads (`latest.json` on GitHub Releases) at most once every 24 hours and print one line to standard error when a newer stable version exists, for example `Cuotai 0.81.0 is available: brew upgrade cuotai`. The command is the one of the channel you installed from (`cuotai --install-source`); a copy without a package manager gets the releases page instead. The request carries only the `User-Agent` `cuotai/<version>`; the answer is cached in `<OS local data dir>/cuotai/update-check.json`. Nothing is printed for `--json`, `--json-v1`, `--watch`, `--serve` or when standard output is not a terminal, and a failed check is silent. Cuotai never updates itself from the terminal.

## Backup, restore and self-check

```sh
# Check this installation, read-only and offline (exit 2 when something needs attention)
cuotai --doctor            # or --doctor --json
# The same, plus one credential-free HEAD request to each provider host the accounts use
cuotai --doctor --probe
# Back up the settings, never a secret (--with-history adds the last 90 days of history)
cuotai --backup cuotai-backup-2026-11-04.cuotai-backup.zip [--with-history]
# See what a backup would change here, then restore it
cuotai --restore cuotai-backup-2026-11-04.cuotai-backup.zip --check
cuotai --restore cuotai-backup-2026-11-04.cuotai-backup.zip [--only config|prefs|user|history] [--yes]
```

- **`--doctor [--json] [--probe]`** (PRD 0046 RF4) checks the configuration file (with `--check-config`'s strict reader), every account's credential variable (set or not, never its value; local tools are left to the next refresh), each extension account's approval (still valid, or changed since you approved it), whether the system vault answers when an account signs in through it (no entry is read, so a sign-in's expiry is left to the next refresh), the data folder (`CUOTAI_DATA_DIR` included), the history, model and cost databases (opened read-only: size, version, `quick_check`, a newer format), `hooks.toml`, the desktop settings, the local server token's permissions, whether the default port 6738 is free, the local cost sources that are on (can their folders be listed), each notification channel's last delivery, the clock (a server-clock difference this run saw, or a stored reading dated more than five minutes ahead) and the language and time zone; since PRD 0052 RF3 also the app log's health (written, off, or failing; never its lines), the network proxy (the variable that sets it and `scheme://host:port` only, never a user name, password or the `NO_PROXY` entries), how many system root certificates load (none fails: every HTTPS request would), and a summary of Cuotai's own files from the [privacy inventory](privacy.md) (how many exist, their size, and any whose permissions are wider than expected). `--json` adds an `artifacts` list with each file's id, home-abbreviated path, existence, size, Unix mode (and the expected one when it differs), Windows access summary and format version; no content is read. Each problem has a fix line. Accounts are named by position and provider, and paths under your home folder start with `~`, so the report can go into an issue. Exit `0` without a failure (warnings included), `2` with one, `1` when the doctor itself fails. It never writes and never reads a vault entry. Offline by default; **`--probe`** adds one `HEAD` request to each provider host the accounts use (fixed per provider, never an account's own base URL; and to their public status pages when `--status` or `CUOTAI_STATUS=1` is on), without any credential, cookie or body: any HTTP answer means reachable, and the answers' `Date` headers feed the clock check. `cuotai --report-problem` and the desktop's **Report a problem** append the doctor's failures and warnings to the issue text by their ids only (`- fail `account.2`: `doctor.account.env_missing``), never a path, variable or value.
- **`--backup FILE [--json]`** (PRD 0046 RF1/RF2) writes a `.cuotai-backup.zip` with `manifest.json` (format 1, date, version, OS, each file's SHA-256 and kind) and an allow-list of files: the accounts TOML (verbatim: it names variables, not values) and `hooks.toml`; the desktop settings without window geometry, one-time records or machine paths; the local cost settings without the per-machine sources and folders; your `pricing.json`; your user themes (`*.css`) and languages (`*.json`). **Never included:** vault entries, tokens, the local server token, the cost index, extensions and logs; the command lists them with the reason. If any file holds something shaped like a key or token, nothing is written (exit `1`), and the development SQLite store (`--dev-credentials-sqlite`) is refused. The same files give the same archive. The labels and hidden flags of the local cost index's projects and sessions travel as `prefs/cost-labels.json` (never a folder name). **`--with-history`** adds, per account, the last 90 days of its history as `history/<account id>.csv` (the sample table of the history CSV export) and warns on standard error above 32 MiB.
- **`--restore FILE`** (PRD 0046 RF3) checks the archive (only those folders, every digest, a known format; exit `2` for a damaged, edited or newer backup), then prints the plan: per file `add`, `replace`, `keep (identical)` or `skip (changed here after the backup)`, with the TOML's accounts added, changed and removed by id, and **what you still need to do**: each variable to set and each sign-in to redo, because no secret travels. It then writes the additions and asks `y/N` for each replacement in a terminal (`--yes` replaces all of them; without a terminal and without `--yes` it stops when a file would be replaced). Desktop settings are merged over this computer's (its window, paths and other members stay). The history and the cost labels are **merged** (`merge`, with the rows in the backup and how many are new here): a sample this computer already has for the same metric and time wins, and a label is taken only where this computer has none. A history file goes to the account with the same id, or else the same provider and name. Notification channels (PRD 0044) travel inside the desktop settings without their address: a restore keeps this computer's channels, adds the backup's others and lists each one whose address you type again. Every write is atomic and refused when the file changed since the plan. Nothing is written to the vault. `--check` prints the plan and writes nothing; `--only config|prefs|user|history` restores one part. The desktop does the same from **Settings › Advanced › Files › Backup and restore** (see [`gui-features.md`](gui-features.md)).
- **Move to a new computer:** run `cuotai --backup --with-history` on the old one (or **Back up…** in the desktop), install Cuotai on the new one, run `cuotai --restore FILE --check` there to read what is missing, then `cuotai --restore FILE`, set the listed variables in the environment that starts Cuotai and sign in again to the accounts it names. In the desktop, **Restore from a backup** on the first screen (or **Restore…** in Settings › Advanced › Files) shows the same plan and then a **Next steps** list with a button per key to type or sign-in to redo. `cuotai --doctor` (**Check setup** in the desktop) then names anything still missing.
- **Keep two computers aligned:** back up on the one you changed and run `cuotai --restore FILE --check` on the other: the plan is the difference. Restore with `--only config` to take the accounts and leave that computer's preferences.

## App log

The terminal, the CLI modes and the desktop app write one redacted log, `<OS local data dir>/cuotai/logs/cuotai.log` (`~/Library/Application Support/cuotai/logs/` on macOS, `%LOCALAPPDATA%\cuotai\logs\` on Windows, `~/.local/share/cuotai/logs/` on Linux; `logs/` under `CUOTAI_DATA_DIR` when that is set), private to your user (`0600` on Unix). At 256 KiB it moves to `cuotai.log.1`, replacing the older copy, so the log never holds more than 512 KiB. The default level is `info`; `CUOTAI_LOG` (or `RUST_LOG`) changes it and `off` turns it off.

Each line carries a time, a level, the app (`cli`, `tui` or `desktop`), a stable event id such as `serve.started` or `history.degraded`, and a few typed fields (counts, durations, classes). It never holds a token, cookie, authorization header, request or response body, the path or query of a URL, an e-mail address, or an account name below the `debug` level: every value passes a fail-closed redactor before it reaches the file. A crash adds one `panic` record (app, version, thread, source file name and line, a redacted message of at most 2 KiB) and never a backtrace. When the desktop app cannot start, it shows a native message naming this file and exits with a non-zero status. Nothing in the log leaves your computer unless you copy it.

## Environment variables

| Variable | Effect |
|---|---|
| `CUOTAI_CONFIG` | Accounts file to use (terminal and desktop). |
| `CUOTAI_LANG` | Language of the terminal interface and of the core's texts (any of the 14 interface languages: `en-US`, `pt-BR`, `es`, `it`, `de`, `fr`, `ja`, `zh-CN`...; system forms such as `pt_BR.UTF-8` are normalized). Without it the system language is used, and English when that is not one of them. |
| `CUOTAI_FALLBACK_LOCALE` | Language of the localized `fallback` texts and plain `notes` in `--json`, `--json-v1`, `--serve` and `--watch --jsonl` (the code of any core catalog, such as `pt-BR`, `es`, `it` or `de`; English without it). Same as `--fallback-locale`. |
| `CUOTAI_PACE_UNIT` | `points` (default) or `percent`: how the terminal shows the pace deviation (`--pace-unit` wins). |
| `CUOTAI_STATUS=1` | Show providers' public status in the terminal and `--once` / `--json` (`--no-status` always wins). |
| `CUOTAI_NO_ANIMATIONS=1`, `CUOTAI_ANIMATIONS=1` | Turn terminal animations off or back on (see [`tui.md`](tui.md#animations)). |
| `CUOTAI_USER_DIR` | Folder for user themes and languages (desktop). |
| `CUOTAI_DATA_DIR` | Absolute folder for every local store instead of `<OS local data dir>/cuotai`: the history, model cache, reset marks, local cost index and settings, prices, hook consent and logs, extensions, server token, last-config records and the desktop widget fallback (terminal and desktop; set it in both to share one profile). The desktop app can also choose its own folder in **Settings › Advanced › Files › Data folder** (applied at the next start); the variable wins over that choice and the terminal never reads it. It is created private (`0700` on Unix); a relative path, a link, a file or another user's folder is a local error (exit `1`) and Cuotai never falls back to the OS folder. Moving it does not move existing data: copy the old folder first. |
| `CUOTAI_UPDATE_CHECK=1` | Turn on the terminal update notice (see [Update notice](#update-notice)); `0` turns it off even when the accounts file turns it on. |
| `CUOTAI_UPDATER=off` | Turn the desktop in-app updater off (for example for a package-manager install). |
| `CUOTAI_LOG`, `RUST_LOG` | Level of the app log (see [App log](#app-log)): `EnvFilter` syntax such as `debug` or `cuotai=debug,warn`, `off` to write nothing. `CUOTAI_LOG` wins; an invalid value falls back to `info`. |
