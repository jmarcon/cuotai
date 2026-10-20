# TUI, CLI and Account Wizard Guide

The `cuotai-tui` binary provides both the interactive terminal dashboard (Ratatui) and single-run modes (`--once` and `--json`) for scripts and automation.

The TUI's own captions (shortcuts, frames, buttons) are shown in Portuguese. This guide quotes them as they appear on screen, with an English gloss where it helps.

## Running

Requires Rust/Cargo. AWS Bedrock also requires AWS CLI v2 on your `PATH`.

```sh
cargo run -p cuotai-tui -- --list-providers
cargo run -p cuotai-tui
```

The TUI resolves the texts delivered by the core (login labels,
fields, help, edit errors, report notes, cost warnings) against the
core catalog, using the locale set in `CUOTAI_LANG`
(RFC 5646 codes: `en-US`, `es`, `pt-BR`...). Without `CUOTAI_LANG`,
it keeps the `pt-BR` default so the chrome stays in Portuguese. The
TUI frame itself (shortcuts, menus, Ratatui's own messages) remains in
Portuguese, outside the scope of RF8.

For a single run or a local install:

```sh
cargo run -p cuotai-tui -- --config cuotai.toml --once
cargo run -p cuotai-tui -- --config cuotai.toml --json
cargo install --path crates/cuotai-tui --locked
```

### Configuration precedence and exit codes

- **Precedence:** `--config`, `CUOTAI_CONFIG`, an **existing** `./cuotai.toml`, the system configuration directory (`<OS config dir>/cuotai/config.toml`; `~/Library/Application Support/cuotai/config.toml` on macOS). The header shows the effective path and its origin (`--config`, `CUOTAI_CONFIG`, `local` or `OS default`).
- **Different-files warning:** the TUI and the desktop app record the last opened file in `<OS data dir>/cuotai/last-config-*.json` (only the normalized path and the date). If the desktop app used another file, the header shows both paths and reminds you that accounts and model caches are separate; align them with `--config` or `CUOTAI_CONFIG`.
- With no file found in interactive mode, the wizard creates `<OS config dir>/cuotai/config.toml` (the same default as the desktop app). To create a local file, pass it explicitly: `--config ./cuotai.toml`. Existing files are never migrated, merged or overwritten automatically.
- In `--once`/`--json` mode, with no file the default catalog is still used (the wizard does not open), and a missing or invalid explicit file returns an error. An invalid file is never ignored.
- HTTP queries use the default variables when present; AWS only queries after `params.profile` is configured explicitly.
- **Binding limit (`--once` and the dashboard):** `--once` opens with one line per account naming the limit that restricts it the most, chosen by the core (`Claude  bloqueada até 14:30 pela janela semanal`, `Work  mais restritiva: janela semanal em 62%`; an unknown return says so, never "now"), in `CUOTAI_LANG` and local time; accounts without one show their status. Each dashboard card leads with that metric, and the details mark it with `◆ limitante` under a `LIMITE` line while every metric stays listed. `--json` is unchanged.
- **Measurement age (`--once` and the dashboard):** each card ends its status line with the age of its reading (`há 3 min`, computed by the core from `checked_at`), the details add it to `consultado` with the time in UTC and in the local time zone, and each `--once` account heading ends with `consultado há 3 min`. When a query fails after a success, the card and the details keep the last valid metrics, still and marked `(desatualizada) há 42 min` with their original age, while the details' notes keep the cause of the failure; the next successful query removes the mark and an authentication failure drops the old values. The reading lasts for the session only; `--json` never carries it.
- **Consumption pace (`--once` and the dashboard):** each eligible metric (a quota in a 5 h, daily, weekly or monthly window with a limit and a future reset) shows how its use compares with a steady pace until the reset. On a card its label starts with a stage icon: `↓` in reserve, `→` on pace, `↑` above the pace, `✕` limit reached (the shape carries the stage; color only repeats it). The details add one line per metric, such as `↑ 6 pontos acima do ritmo · acaba às 18:49, 11h 10m antes da renovação · esperado 82% usado · estimativa linear`, and `--once` prints `Ritmo: …` under the metric. The texts follow `CUOTAI_LANG`. It is a linear estimate from the average since the window start, not a promise. Windows that opened less than 5% (or 10 minutes) ago, rolling rate limits, metrics without a reset and the stale last reading show no pace. `--json` is unchanged.
- **Exit codes (`--once` and `--json`):**
  - `0`: every account refreshed successfully (`ready`)
  - `2`: results with missing configuration or an account failure
  - `1`: local error (invalid arguments or file)

## TUI keyboard and mouse shortcuts

- **Accounts:** `a` adds an account, `e` edits the selected one, `d` or `Delete` deletes the selected account (confirm with `s` to write or `n`/`Esc` to cancel).
- **Navigation:** `↑↓`/`j k` select, `PgUp/PgDn` page, `Home/End` jump to the ends, `Enter` opens the account details (`←→` switch between visible accounts in the details; `↑↓` or `[`/`]` scroll the details; `Enter`/`Esc` go back).
- **Filter and view:** `/` filters, `Enter/Esc` end filter editing, `c` clears the filter, `u` shows or hides accounts without data (visible by default).
- **Refresh:** `r` refreshes all accounts and `R` refreshes only the selected one.
- **Models:** in the details, `m` opens the account's Models; in the wizard's Provider step, `m` lists the provider's **already configured** accounts and `Enter` opens the chosen one (with no account, it shows only the support status and guidance). The first open queries only if the provider is supported and has never been queried; afterwards it reads the cache. `/` searches locally by ID/name (separate from the account filter), `c` clears, `s` is **Sync models**, `↑↓`/`PgUp/PgDn`/`Home/End`/wheel scroll, `Esc` (or right click) goes back. The query runs in the background, does not refresh quotas, and is discarded if the account changes (edit, delete, move, reload). `r`, `R`, `--once` and `--json` never sync models.
- **Reordering:** `K`/`J` move the selected account up/down in the order persisted in `cuotai.toml` (shared with the desktop app, preserving each account's comments), without triggering any provider query.
- **About:** `?` (or the `? sobre` footer entry) opens Cuotai's version, license and repository, plus the list of open-source libraries in use, with license, the component that uses each and its repository. The list comes from `cuotai_core::about`, shared with the desktop app; only direct dependencies appear (the full inventory is in `Cargo.lock`). `↑↓`/`PgUp/PgDn`/`Home/End`/wheel scroll; `Esc`, `q`, `?`, `Enter` or right click go back.
- **Quit:** `q` or `Ctrl-C`.
- **Minimum terminal size:** dashboard `45 × 18`; wizard `60 × 20`. Dracula theme.

### Mouse support

- A click selects a card; clicking the already selected card opens its details.
- In the details, click the visible part of a single HTTPS source to open it in the browser; composite sources stay scrollable text. Right click goes back from the details.
- The mouse wheel moves through the card rows and scrolls the details, the wizard lists and the login instructions.
- The footer has clickable actions; the wizard offers `[Voltar]` (Back), `[Próximo]` (Next), `[Gravar]` (Save) and `[Cancelar]` (Cancel). The wheel never confirms a login, a save or a deletion. Mouse capture is released on exit or on panic.

## Quota dashboard

- The cards (two columns on wide terminals) keep the order of the configuration file, even when a query result changes.
- Independent queries run in parallel up to `concurrency`; during a refresh the previous data stays visible. The header shows the countdown to the next query of the selected account.
- Bars represent the **remaining** percentage; used values above 100% are preserved and only the drawing is clamped to the bar width.
- Costs and balances with no known limit show plain text values, without inventing a percentage or "unlimited". An extra balance of zero does not make a subscription that still has limited quota available count as exhausted.
- Deleting an account removes only the configuration entry: it does not delete environment variables or Keychain entries, and it does not revoke access at the provider.

### Animations

- After a refresh, card gauges and detail bars slide from the old value to the new one (~300 ms) at 1/8-cell resolution. A class change (`Disponível`/`Esgotado`/`Sem dados`) flashes the label twice (2 flashes/s). A braille spinner (`⠋⠙⠹…`) precedes the card title while that account refreshes. Opening or closing the details, `Sobre` and `Modelos` uses a 120 ms top-down wipe.
- With `COLORTERM=truecolor` (or `24bit`) bar colours follow a red → yellow → green gradient by level; otherwise the usual red/yellow/green steps are used.
- The loop draws at ~30 fps only while an effect runs; when idle it keeps the usual pace (redraw on events and the 1 s countdown tick).
- **Off:** `--no-animations`, `CUOTAI_NO_ANIMATIONS=1` (any non-empty value except `0`), or `TERM=dumb` (always). Under SSH (`SSH_CONNECTION` or `SSH_TTY`) they are off by default; `CUOTAI_ANIMATIONS=1` turns them back on, but never overrides the opt-outs above. With animations off the screen renders exactly as before. `--once`/`--json` never animate.

## Configuring an account with the wizard

On the first interactive run with no configuration file (or with an empty account list), the TUI opens the **account wizard**. You do not need to edit the TOML by hand (for manual configuration, see [`cuotai.example.toml`](../cuotai.example.toml)).

1. **Environment (optional):** for an environment credential, export the access token in the terminal that will run Cuotai (`export MY_TOKEN`). Cuotai does not load `.env` automatically. For the built-in OAuth login, see [OAuth and the vault](oauth.md).
2. **Open the wizard:** run `cargo run -p cuotai-tui`. If a configuration already exists, press `a` to add, or select an account and press `e` to edit.
3. **Provider:** select with the arrow keys and confirm with `Enter`.
4. **Account name:** enter a unique name, such as `Personal Codex`.
5. **Credential:** this step lists **Manual credential** (the name of the environment variable, or the AWS profile, depending on the provider) and each **login mode** the core offers for the provider (`Login no navegador` (browser login), `AWS SSO`, `Console Bailian`, `Importar sessão Cursor` (import Cursor session), etc. The list, labels and help come from `cuotai_core::login`, never decided by the TUI). Press **Tab** to switch between them; `Enter` moves through the selected mode's fields and `F2` starts or redoes the login of the current mode (with no provider-specific shortcut). The wizard shows `PRESENTE`/`AUSENTE` (present/absent) for variables, without revealing values.
   - Reauthenticating (F2, or **Refazer login** on the desktop) reuses the `credential_id` of the account being edited: the token is overwritten in the vault, leaving no orphaned credential. If an option that makes up the vault key changes (Kiro method/region, `client_id`, tenant, etc.), the old credential is removed automatically **after** the TOML is saved, unless another account still references it; if that cleanup fails, the state is reported as non-fatal (configuration and new credential saved, previous credential kept in the vault).
   - Modes whose login lives outside Cuotai's vault (AWS SSO, Bailian's `bl`) write only public parameters to the account (`params.profile`, `params.cli_profile`); cancelling the login step does not undo what the external CLI already wrote.
6. **Parameters:** fill in the provider-specific fields; `Enter` advances between them. Optional ones may stay empty. Copilot offers choices with `←→`, showing the organization only in `org` mode.
7. **Interval:** enter `30` to `86400` seconds for **this account** (minimum `300` s for AWS Bedrock); empty inherits the global `refresh_seconds` of the TOML (default: `300`). Accounts with their own intervals are scheduled separately. AWS shows a per-query billing warning; consider `3600` seconds or more.
8. **Review:** check the account, parameters and target file; `↑↓` scroll the summary. Only `Enter` on this step writes and applies the configuration, starting the query on the dashboard.

`Shift+Tab` goes back while keeping the draft; `Ctrl+U` clears the field; `Backspace` deletes; `Esc` cancels without saving. Changing the provider resets the fields and the credential reference, to avoid reusing the previous provider's token.

### Atomic TOML persistence

Writing uses a temporary file and an atomic replace, with `0600` permission for new files on Unix. Edits to `[[accounts]]` tables preserve the order, the account's comments and the other parameters and settings. If the file changes externally, the wizard refuses to overwrite it and tells you to close and reopen Cuotai. On a write failure, the draft stays on the review step so you can try again.

Tokens supplied through variables remain managed externally: if you update a variable in the shell, restart Cuotai. Credentials obtained through the built-in login are recovered from the vault and renewed automatically before the query when close to expiring.
