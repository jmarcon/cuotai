<div align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="apps/desktop/ui/img/logo-on-dark.svg" />
    <source media="(prefers-color-scheme: light)" srcset="apps/desktop/ui/img/logo-on-light.svg" />
    <img src="apps/desktop/ui/img/logo.svg" alt="Cuotai logo" width="84" height="84" />
  </picture>
  <h1>Cuotai</h1>
  <p><strong>All your AI quotas, usage limits and costs in one place.</strong></p>
  <p>
    A read-only, multi-account monitor written in Rust, with a <strong>desktop app (Tauri 2)</strong>, an <strong>interactive TUI (Ratatui)</strong> and a <strong>JSON CLI</strong>.
  </p>
  <p>
    <a href="#desktop-app">Desktop</a> •
    <a href="#terminal-tui--cli">TUI &amp; CLI</a> •
    <a href="#supported-providers">36 providers</a> •
    <a href="#getting-started">Getting started</a> •
    <a href="#documentation">Documentation</a> •
    <a href="#license">License</a>
  </p>
</div>

---

## Why Cuotai?

Anyone working with several AI assistants and APIs ends up switching between dozens of web dashboards to find out how much is left of the 5-hour window, the weekly quota or the monthly budget. **Cuotai** brings it all together:

- **100% read-only, zero tokens spent:** it only queries quota, billing and header endpoints. It **never** sends inference requests that would burn your credits to measure a limit.
- **Secure by default:** `cuotai.toml` holds references only. OAuth tokens live in the operating system's native vault (**macOS Keychain**, **Windows Credential Manager**, **Linux Secret Service**) with automatic renewal, or in environment variables.
- **Built-in native login:** sign in through the browser, Device Code or AWS SSO without copying tokens by hand (Claude, ChatGPT/Codex, GitHub Copilot, Gemini CLI, Kiro, Antigravity, Cursor, Kimi, Zed, CodeBuddy and more).
- **Three interfaces, one core (`cuotai-core`):**
  - **Desktop and system tray (Tauri 2):** a detailed main window, a quick menu-bar popover (donut or list view) and a native right-click menu.
  - **Terminal TUI (Ratatui):** a fast dashboard with keyboard and mouse support, instant filtering and a full account wizard.
  - **CLI (`--once` / `--json` / `--json-v1` / `--serve` / `--watch`):** built for scripts, status bars (Waybar, SketchyBar, tmux) and automation.
- **Themes and internationalization:** 15 built-in themes (Cuotai, its own pastel theme; Dracula, Catppuccin, Tokyo Night, One Dark, GitHub, macOS, Windows 11, Ubuntu, Synthwave, CRT, Win98/XP, Tron), custom themes and languages, and a UI in **Portuguese**, **English**, **Spanish** and **Italian**.

---

## Desktop app

The desktop app watches your accounts in the background with incremental per-account scheduling, drag-and-drop reordering (synced with the TUI) and immediate visual alerts when a quota needs attention or runs out. Each account highlights its most restrictive limit: the tray ring and a `Week · 100%` chip on the card follow the window that blocks you (a weekly limit at 100% wins over a free 5-hour window), the list says "Blocked until 14:30", and Settings offers an optional "Most restrictive first" order that leaves your manual order untouched. Every card, tray entry and menu line also says how old its reading is ("Fresh · 3 min ago", "Aging", "Stale"), and the tray icon tooltip names the oldest one. When a query fails after a success, the card keeps the last valid numbers, dimmed, with a "Stale · 42 min ago" badge and the cause of the failure, until the next successful query; an authentication failure drops them.

**Settings → Appearance** can also group exhausted or blocked accounts in a collapsible "Recharging" section that shows each return time ("Returns Today 14:30", or "Return unknown"). It is off by default and only changes the display: the manual order holds inside each group and the order saved in `cuotai.toml` never changes.

Cards, details and the tray also show the **consumption pace until the reset**: whether what you used so far lasts at a steady rate ("Week: 8 points in reserve · lasts until the reset") or runs out early ("runs out at 16:40, 2h 10m before the reset"), with an "Expected 47% used" mark drawn over each bar in the details. It is a linear estimate, not a promise; windows that just opened, rolling rate limits and metrics without a reset time show nothing. **Settings → Appearance → Show consumption pace** hides it (on by default).

The TUI shows it too: a stage icon (`↓` in reserve, `→` on pace, `↑` above the pace, `✕` limit reached) before each eligible metric on the cards, a pace line per metric in the details, and a `Ritmo:` line under each eligible metric in `--once` (see [the TUI guide](docs/tui.md)). `--json` is unchanged.

**Settings → Notifications** can send a **system notification when a quota resets** (off by default; turning it on asks the system for permission, and a refusal keeps it off with guidance on allowing Cuotai in the system settings). Choose the moment (at the time, or 5, 15 or 60 minutes before), the filter (only if used, only if exhausted, always) and whether "other" windows count; **Send test notification** checks that notices arrive. The app schedules the notices itself, so they come with the window hidden in the tray, but not with the app closed. Each reset is notified once, even across restarts (`<OS local data dir>/cuotai/notified.json` keeps only digests and dates, for 8 days); a reset missed by more than 10 minutes while the app was closed or asleep is not notified late, and several resets of an account at once become one notice. A notice never queries the provider: it follows the time the provider reported and the new balance shows on the next refresh. Its text names only the provider, the account and the metrics, in the language chosen in the app. Each card and its details get a bell to silence one account; the choice is kept per provider and account name in `desktop.json`, so renaming an account turns its bell back on. The tray menu, its tooltip and the tray popover show the next notice ("Next notice: Claude 5h · today, 14:30").

The same section turns on **usage alerts** (off by default, with the same permission request and test button): a system notification when a quota with a limit reaches a threshold of use ("Claude: 90% of the 5-hour window used", with the account, the quota and its reset time), runs out (**Notify exhaustion**) or has slack again in the next window after running out (**Notify recovery**). The thresholds are chips from 10 to 99% (75 and 90 by default); each one notifies once per window, oscillating around it stays silent, and it re-arms only after use falls 5 points below it or a new window starts. Balances, spend, rate limits and missing values never alert, and no alert queries a provider: the app judges each reading its refresh already took, also with the window hidden in the tray. An account's alerts of one refresh become one notification, at most one notification goes out per minute (the rest are grouped into the next), and nothing is resent after a restart (the alert state joins `notified.json`, digests and dates only). The account's bell silences its alerts too, and each quota can be muted from the account details; the card, the tray and the details highlight an account with an active alert. Preferences live in `desktop.json`, never in `cuotai.toml`.

**Settings → Privacy** can show the **providers' public status** (off by default). Turned on, the app reads the public status pages of the providers you configured (the section lists their domains; each page at most once every 5 minutes, without credentials, cookies or account data, though those sites see your IP address and that you use the provider). A card then shows a colored badge with an icon and text when its provider reports degraded performance, a partial or major outage or maintenance, or when its status page could not be read, with **Open status page**; the details show the full status and when it was read, the tray popover lists the affected providers, and the tray icon gets a red dot while any of them reports a partial outage or worse. Providers without a verified public source never show as operational. A failed query during an incident says the failure may not be from your account.

The TUI and the CLI read the same status only when you ask (`--status`, or `CUOTAI_STATUS=1` in the environment); `--no-status` always turns it off, also in scripts that inherit the variable. A card then shows the badge on its top border (`▲ Interrupção parcial`, `? Página de status indisponível`), the details list the provider's full status with the incident, its start, when it was read and the page, and `--once` prints a `Status do provedor:` line under the affected account. `--json` keeps its shape.

**Settings → Integration** runs the same read-only **local server** as `cuotai --serve` from inside the app, off by default. Turned on, it listens on `127.0.0.1` only, on exactly the port you choose (6738 by default), and serves this app's own readings (`/health`, `/v1/snapshot`, `/v1/accounts/{key}`, `/v1/limits`), so reading it never queries a provider. The section shows the effective address (with **Copy address**) or why it is not serving; a taken port, for example by a `cuotai --serve` already running, is named as such there and in a banner on the main window. **Generate token** keeps a bearer token in the system vault (never in the TOML) and requires it; **Copy token** copies it without showing it, **Generate new token** stops the old one at once and **Revoke token** removes it. **Allow web browsers** asks for confirmation first, because any page open in your browser could then read your usage. See [the local server guide](docs/local-server.md).

**Settings → Integration → Hooks** runs your own commands on account events from inside the app, off by default: an editor for the rules of the `hooks.toml` next to the accounts' file (name, events and the command as one field per argument, never a shell string), shared with `cuotai --watch --hooks`. Turning hooks on, or saving changed rules while they are on, shows a dialog with the exact commands, and nothing runs until you confirm it. The confirmed rules run on the events of the app's own readings (no extra query); **Test** sends a rule a sample event marked `"sample": true`, **Latest runs** lists the local run log, and a file changed outside the app asks again (with a banner on the main window). See [`docs/events.md`](docs/events.md#hooks-in-the-desktop-app).

**Settings → Local cost** reads the local session logs of Claude Code and Codex CLI to count their tokens per day and per model, off by default and per source: turning a source on first shows a dialog with exactly the directories read, what for and that only numbers (time, model, token counts, an anonymous session digest) are extracted, and nothing is read until you accept it. A low-priority background scan then reads only what grew, pauses in energy-saving mode and never delays the quota refresh; the section shows what it found (logs followed, invalid lines ignored, or no access), the period shown and the index retention (120 days by default), and turning a source off or **Erase the index** removes everything derived. The same sources are managed with `cuotai --cost-*` (see below); the estimated cost at API prices arrives with PRD 0015 RF3 and RF4.

Every card and its details also say, discreetly, **how that account's usage is read**: `Official` (an interface the vendor documents), `Undocumented` (an internal interface that may change without notice), `Derived` (inferred from metrics or rate-limit headers) or `Unavailable` (no usage query exists), with a one-line summary. An `Unavailable` account shows why instead of a generic error, and when an undocumented interface stops matching, the card says it may have changed and names the public contract Cuotai follows. The same levels appear when you add an account.

<div align="center">
  <img src="docs/assets/desktop-main.png" alt="Cuotai desktop main window" width="100%" />
  <p><em>The Cuotai desktop main window showing 5-hour and weekly windows, rate limits and account status.</em></p>
</div>

<div align="center">
  <img src="docs/assets/desktop-tray.png" alt="Cuotai desktop tray popover" width="420" />
  <p><em>The tray popover in the compact donut view, with an alert summary in the footer.</em></p>
</div>

---

## Supported providers

Cuotai ships **36 integrations**. For parameters, default variables and limitations of each provider, see **[`docs/providers.md`](docs/providers.md)**.

| Category | Providers |
|---|---|
| **Assistants and subscriptions** | **Claude** (`claude`), **OpenAI Codex / ChatGPT** (`chat_gpt`), **GitHub Copilot** (`copilot`), **Cursor** (`cursor` — personal and Teams), **Windsurf** (`windsurf`), **Kiro / AWS** (`kiro`), **Antigravity** (`antigravity`), **Cline** (`cline`), **Zed** (`zed`), **CodeBuddy** (`code_buddy`), **Qoder** (`qoder`), **OpenCode Go** (`open_code_go`) |
| **CLIs and coding plans** | **Gemini CLI** (`gemini_ai`), **Grok CLI / xAI** (`grok`), **Kimi Coding & API** (`kimi`), **MiniMax Token/Coding Plan** (`mini_max`), **Alibaba Coding Plan** (`alibaba_coding_plan`), **Alibaba Token Plan / Qwen Cloud** (`alibaba_token_plan`), **Xiaomi MiMo** (`xiaomi_mimo`), **Ollama Cloud** (`ollama_cloud`), **Qwen Code** (`qwen`) |
| **Cloud billing and admin APIs** | **OpenAI Admin API** (`open_ai_api`), **Anthropic Admin API** (`anthropic_api`), **Google Gemini API Monitoring** (`gemini_api`), **Google Vertex AI** (`vertex_ai`), **AWS Bedrock Cost Explorer** (`aws_bedrock`), **Azure OpenAI Cost Management** (`azure_open_ai`), **Alibaba Cloud BSS** (`alibaba_cloud`), **Mistral Admin** (`mistral`), **Perplexity Computer Analytics** (`perplexity`), **Kimchi** (`kimchi`), **Groq** (`groq`) |
| **Additional catalog** | **Alibaba Model Studio** (`alibaba_model_studio`), **Cloudflare AI** (`cloudflare_ai`), **NVIDIA NIM** (`nvidia_nim`), **Poolside** (`poolside`) |

---

## Installation

Package-manager channels ship with the release pipeline but stay **off until
the maintainer enables them**, one at a time. The commands below are what
each channel installs when it is live; until then use the direct download or
the source build below. See [`docs/releasing.md`](docs/releasing.md) for the
channel status and [`packaging/README.md`](packaging/README.md) for the
manifests.

| Platform | Command | Status |
|---|---|---|
| macOS (desktop) | `brew install --cask jmarcon/cuotai/cuotai` | available once the maintainer enables the tap |
| macOS (TUI/CLI) | `brew install jmarcon/cuotai/cuotai` | available once the maintainer enables the tap |
| Windows (desktop) | `winget install Cuotai.Cuotai` | available once the first winget submission is accepted |
| Windows (TUI/CLI) | `scoop bucket add cuotai && scoop install cuotai` | available once the maintainer enables the bucket |
| Linux (desktop, Arch) | `yay -S cuotai-bin` | available once the AUR package is published |
| Linux (desktop, Debian/Ubuntu) | download the `.deb` and `sudo apt install ./Cuotai_<version>_amd64.deb` | available with every release |
| Linux (desktop, Fedora) | download the `.rpm` and `sudo dnf install ./Cuotai_<version>_amd64.rpm` | available with every release |
| Linux (TUI/CLI) | download `cuotai-<version>-<target>.tar.gz`, extract and run `./cuotai` | available with every release |
| Any (TUI/CLI) | `cargo install --git https://github.com/jmarcon/cuotai cuotai-tui --locked` | works today |
| Any (TUI/CLI) | `cargo install cuotai-tui --locked` (or `cargo binstall cuotai-tui`) | available once `cuotai-core` and `cuotai-tui` are on crates.io |
| Any (desktop) | download the `.dmg`, `.exe`, `.msi`, `.AppImage`, `.deb` or `.rpm` from [Releases](https://github.com/jmarcon/cuotai/releases) | available with every release |

Unsigned releases stay honest: macOS shows the Gatekeeper override note, and
Windows may warn about an unknown publisher. Nothing claims a signature the
artifact does not have.

### Verify your download

```sh
shasum -a 256 -c SHA256SUMS                        # every asset in one go
gh attestation verify Cuotai_0.68.0_universal.dmg --repo jmarcon/cuotai
```

### Uninstalling

Uninstalling removes the application or binary only. Your configuration
(`<OS config dir>/cuotai/config.toml`), the local databases and the OS vault
entries (macOS Keychain, Windows Credential Manager, Linux Secret Service)
stay behind on purpose; removing them is a documented manual step in
[`docs/releasing.md`](docs/releasing.md#uninstall-keeps-your-data-rf82). With
Homebrew, `brew zap --cask cuotai` additionally removes the app's caches and
preference plist, and only that.

---

## Getting started

The steps below build Cuotai from source. For prebuilt packages, see
[Installation](#installation).

### Prerequisites

- **Rust / Cargo** (2021 edition)
- *(Optional)* **AWS CLI v2** on your `PATH` for `aws_bedrock` accounts
- *(Optional)* **Bailian CLI (`bl`)** for the console login of `alibaba_token_plan`

### 1. Desktop app (Tauri)

```sh
cd apps/desktop/src-tauri
cargo run
```

To build an installable package for your operating system:

```sh
cargo install tauri-cli
cd apps/desktop/src-tauri && cargo tauri build
```

### 2. Terminal TUI and CLI

Run the interactive dashboard in your terminal (it opens the wizard automatically on first run):

```sh
cargo run -p cuotai-tui

# Announce quota resets and usage alerts (75%, 90%, exhausted, available again)
# while the dashboard is open (header banner, bell, OSC 9)
cargo run -p cuotai-tui -- --notify

# Show the providers' public status (off by default; also CUOTAI_STATUS=1)
cargo run -p cuotai-tui -- --status
```

Or install the `cuotai` binary locally:

```sh
cargo install --path crates/cuotai-tui --locked
```

Quick command-line queries (without opening the interactive interface):

```sh
# List the 36 available providers
cargo run -p cuotai-tui -- --list-providers

# Human-readable summary in the terminal (single run)
cargo run -p cuotai-tui -- --config cuotai.toml --once

# Structured JSON snapshot for scripts and status bars
cargo run -p cuotai-tui -- --config cuotai.toml --json

# The versioned export::v1 contract (class, age, binding metric, pace, access level)
cargo run -p cuotai-tui -- --config cuotai.toml --json-v1

# Read-only local server on 127.0.0.1:6738, refreshed in the background (Ctrl+C stops it)
cargo run -p cuotai-tui -- --config cuotai.toml --serve
curl -s http://127.0.0.1:6738/v1/limits

# One NDJSON event per real transition (threshold crossed, exhausted, reset, failure…)
cargo run -p cuotai-tui -- --config cuotai.toml --watch --jsonl | jq -c 'select(.event == "quota_exhausted")'

# Recorded history of every account (or one), without querying providers
cargo run -p cuotai-tui -- --config cuotai.toml --history "Personal Codex" --history-days 30
```

In the JSON output, each metric carries a `kind` (`quota`, `rate_limit`, `balance`, `credits`, `spend` or `usage`) and a `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other` or `null`), classified by the provider adapter. Consumers do not need to parse `label` or `period` to tell quotas, rate limits and windows apart.

`cuotai --json-v1` prints the versioned `export::v1` contract, which Rust integrations can also build from `cuotai-core`: the last known state of every account with its age, freshness, preserved stale reading, binding metric, pace and usage access level, with no credentials, credential variable names or e-mails, and `null` for anything unknown. See [`docs/export-v1.md`](docs/export-v1.md). `cuotai --serve [--port N] [--token-file FILE]` (or, with the core's `serve` feature, `serve::LocalServer`) publishes it read-only on `127.0.0.1:6738` (`/health`, `/v1/snapshot`, `/v1/accounts/{key}`, `/v1/limits`) from the state the refresh cycle already holds, never querying a provider per request, with DNS-rebinding and browser protection and an optional bearer token; it is off until you start it, and stops cleanly on `SIGINT`/`SIGTERM`. See [`docs/local-server.md`](docs/local-server.md).

`cuotai --watch [--interval S] [--events LIST] [--jsonl]` keeps the same refresh cycle in the foreground and emits one event per real transition between consecutive readings (a quota crossed 75% or 90%, ran out, recovered or reset, an account was blocked, a query failed or recovered, authentication is needed, usage changed), never repeating a condition that persists and treating the first reading of each account as its baseline; `--jsonl` writes NDJSON on stdout with every message on stderr, and `--serve` can share the same cycle. Payloads carry no credentials, URLs or raw error messages. See [`docs/events.md`](docs/events.md).

`cuotai --watch --hooks` also runs your own commands on those events, from a `hooks.toml` next to the accounts' TOML (off by default): each rule lists its events, optional provider/account/threshold filters and a command as an argument list, never a shell string. The exact commands run only after you confirm them in a terminal, again after any change; a hook gets the event as JSON on stdin and in `CUOTAI_*` variables, an environment reduced to `PATH`, `HOME` and `LANG` (no credentials), a time limit that kills it, bounded output and concurrency, and its failure never stops the watch. See [`docs/events.md`](docs/events.md#hooks-cuotai---watch---hooks).

Script extensions (PRD 0017) add your own usage sources without recompiling Cuotai: a folder under `<OS local data dir>/cuotai/extensions/<id>/` holds a `manifest.toml` (id, name, version, `schema = 1`, the command as an argument list whose program is a file inside the folder, the interval — 60 s at least — and time limit, and the **names** of the environment variables the extension asks for) and a local program that prints one JSON object (`{ "schema": 1, "status", "metrics", "notes" }`) to stdout. Every extension is born disabled: `cuotai --extension-check <id>` lists the exact command, the directory, the requested variables and the interval, and runs only after you type the confirmation — and only while the manifest and the program keep the SHA-256 recorded at the approval, so any change disables it again until you approve it anew. The run has no shell, a closed stdin, the extension's folder as working directory, an environment reduced to `PATH`, `HOME`, `LANG` and the approved variables (Cuotai never hands it credentials, and its stderr never reaches a report), a time limit that kills the whole process tree, bounded output and at most two extensions at once. **Cuotai does not restrict the extension's network or files — it runs with your user's powers, so review the code before approving it** — and Cuotai never installs, downloads or updates an extension. Exit codes: `0` when the reading is ready, `2` when the run failed or the output broke the contract, `1` for local errors; `--json` prints the same result as JSON.

`cuotai --cost-enable claude_code|codex_cli [--cost-path DIR]` turns on reading that tool's local session logs (off by default) after printing exactly which directories are read (by default those of `CLAUDE_CONFIG_DIR`/`~/.config/claude` and `~/.claude`, or `CODEX_HOME`/`~/.codex`) and that only the time, the model, the token counts and an anonymous session digest are extracted, and asking you to confirm in a terminal; conversation content is never read into the index. `--cost-sources [--json]` lists the sources and what they read, `--cost-scan [--json]` reads what grew now (the TUI and the desktop app do it in the background), `--cost-period` and `--cost-retention-days` set the period and the retention, and `--cost-disable SOURCE` or `--cost-erase` remove everything derived. The private index lives in `<OS local data dir>/cuotai/cost/cost.sqlite3`. See [`docs/tui.md`](docs/tui.md#configuration-precedence-and-exit-codes).

`cuotai --cost [--days N] [--json]` reads what grew in the logs of the sources that are on, then prints the estimate at API prices in USD (not a charge): per source today, yesterday and the saved period (or the last `N` days, 1 to 730), by model and by day, with models without a known price shown as cost unknown, never zero; exit `0`, `2` when no source is on or one has no logs or no access, `1` for local errors. The TUI shows the same section in the details of Claude and ChatGPT / Codex accounts, the desktop shows a "Local usage" block on those cards with the breakdown in the details (Settings → Local cost also chooses each source's directories and shows the prices in effect), and `--json-v1`/`--serve` publish it as `local_cost` in `export::v1`.

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

Every successful reading (TUI, desktop app, `--once`, `--json`, `--json-v1`, `--serve` and `--watch`) also enters a local measurement history on this computer only: `<OS local data dir>/cuotai/history/history.sqlite3`, private to your user, holding numbers and window metadata, never credentials, account names or provider text. On opening, each account shows its last recorded reading, marked stale, until the first query answers, and the consumption pace projects from the recent slope. The history keeps 90 days by default (`--history-days 7…365` on the command line, **Settings → History** on the desktop), keeps one sample per hour after 7 days (each window's peak stays) and stays under 64 MiB, warning when it had to delete old samples. The desktop account details draw an area chart of each metric over the last 24 hours, 7 days or 30 days, with the last reading highlighted and a marker at each reset (and a table for screen readers); **Settings → History** turns recording off (on by default; what was recorded stays until you erase it), shows the size on disk, exports to CSV or JSON and erases one account's history or all of it, after a confirmation. The TUI account details draw a sparkline of each metric over the last 24 hours, 7 days or 30 days (keys `1`, `2`, `3`), and `--history [ACCOUNT]` prints the recorded series (`--json` for scripts; `--history-days` picks the days shown, 7 by default).

---

## Documentation

The detailed technical documentation lives in **[`docs/`](docs/)**. Some documents are still being translated from Portuguese to English.

- **[TUI, CLI and account wizard guide (`docs/tui.md`)](docs/tui.md)** — Keyboard and mouse shortcuts (including the Models section, `m`, with local search and `s` to sync models), the step-by-step wizard flow, configuration file precedence and CLI exit codes.
- **[OAuth login, sessions and vault (`docs/oauth.md`)](docs/oauth.md)** — How native browser and Device Code logins work, the assisted Cursor import, OAuth preparation for Google/Azure/AWS SSO and secure storage in the Keychain.
- **[Provider catalog and limitations (`docs/providers.md`)](docs/providers.md)** — The full table of the 36 providers, the usage access matrix (level, credential, plan, scope and evidence of each provider and mode, generated from the core), default environment variables, `9router` compatibility, metric semantics and per-query cost warnings (such as AWS Cost Explorer).
- **[Executable extensions (`docs/extensions.md`)](docs/extensions.md)** — Manifest and output schema 1, Shell and Python examples, consent and integrity rules, lifecycle actions, security boundaries and compatibility policy.
- **[Export contract v1 (`docs/export-v1.md`)](docs/export-v1.md)** — The versioned `export::v1` contract for integrations: fields, stability and deprecation policy, redaction rules, JSON Schema and reference output.
- **[Local HTTP server (`docs/local-server.md`)](docs/local-server.md)** — `cuotai --serve`, the desktop's Settings → Integration and the read-only loopback server of `cuotai-core` (`serve` feature): routes, port and fallback, exit codes, limits, token, DNS-rebinding and browser protection.
- **[Events and `cuotai --watch` (`docs/events.md`)](docs/events.md)** — The edge events between consecutive readings, their payload and failure categories, and `cuotai --watch` with `--jsonl`, `--events` and `--interval`: baselines, shutdown and exit codes; hooks (`--hooks`, `hooks.toml`): confirmation, isolation, limits and the run log.
- **[Architecture, Rust library and verification (`docs/architecture.md`)](docs/architecture.md)** — The split between `cuotai-core`, `cuotai-tui` and `apps/desktop`, using `cuotai-core` as a Rust library, the internationalization contract and the test suite.
- **[Desktop app guide (`apps/desktop/README.md`)](apps/desktop/README.md)** — Details specific to the Tauri 2 shell, themes and the user customization folder.
- **[Configuration example (`cuotai.example.toml`)](cuotai.example.toml)** — A commented reference of every `[[accounts]]` entry and TOML parameter.
- **[Roadmap (`docs/plans/`)](docs/plans/0000_control.md)** — Product requirement documents (PRDs) with their implementation status and dependencies.
- **[Releasing (`docs/releasing.md`)](docs/releasing.md)** — The tag-driven release pipeline: dry run, artifact matrix, signing states, verification commands and the owner prerequisites.
- **[Platform support (`docs/platforms.md`)](docs/platforms.md)** — What works on macOS, Windows and Linux: credential vault, tray, notifications, browser login, the `aws`/`bl` CLI tools, local cost sources and the local server.
- **[Release policy (`docs/release-policy.md`)](docs/release-policy.md)** — Semantic versioning in 0.x and the breaking-change contracts, migration expectations, latest-stable support, and reproducibility (lockfiles, pinned toolchain, per-release SBOM).

---

## Contributing

Contributions are welcome. Read [`CONTRIBUTING.md`](CONTRIBUTING.md) first, and report security issues privately as described in [`SECURITY.md`](SECURITY.md).

---

## License

Cuotai is distributed under the terms of either the [MIT license](LICENSE-MIT) or the [Apache License, Version 2.0](LICENSE-APACHE), at your option. Third-party notices are in [`THIRD_PARTY_NOTICES`](THIRD_PARTY_NOTICES).

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in Cuotai, as defined in the Apache-2.0 license, is dual licensed as above, without any additional terms or conditions.
