# PRD 0013 — `watch` and event hooks

| Field | Value |
|---|---|
| ID | 0013 |
| Status | Proposed |
| Owner | `cuotai-core` (event engine and hooks), TUI/CLI and desktop |
| Consumers | `crates/cuotai-tui` (`--watch`), `apps/desktop` (Settings → Integration), user scripts |
| Depends on | PRD 0012 (RF1: the `export::v1` contract); it uses PRDs 0003, 0008, 0009 and 0011 when they exist |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## 1. Context

Cuotai only reacts to state changes inside its interfaces: the card changes color, the tray updates the ring. Anyone who wants to trigger another tool (play a sound, switch agents, write to a file, notify a chat of their own) has to poll `--json` in a loop and compare results by hand, repeating queries and losing the notion of a transition.

CodexBar has `hooks watch`, which polls the providers and fires rules on **real** quota and state transitions, with edge-triggered events (a condition that persists does not repeat), a limit of one attempt per 600 s per account and errors reported as a coarse category (`timeout`, `offline`, `auth_required`, `network_error`), without forwarding the provider's raw error. ClaudeBar offers Claude Code session hooks.

## 2. Problem

- Integrating Cuotai with scripts requires manual polling, with false positives and cost.
- There is no standardized set of events (quota crossed a threshold, ran out, reset, account failed).
- Running arbitrary commands from provider data is dangerous if badly designed (injection, leakage).

## 3. Goals

- Define a stable vocabulary of events derived from transitions between consecutive readings.
- Offer `cuotai --watch`, which emits the events as a stream (NDJSON) and/or fires configured hooks.
- Guarantee that hooks are opt-in, isolated, time-limited and secret-free.

## 4. Non-goals

- Running actions that change provider accounts (switching plan, spending credit, failover): hooks receive data, they do not control providers.
- A persistent queue or guaranteed delivery: events lost while the process is stopped are not recovered.
- HTTP webhooks to the internet in this version (a hook can call `curl` at the user's own risk).
- Replacing the desktop's native notifications (PRDs 0003 and 0008).

## 5. Functional requirements

### RF1 — Event engine in the core (high priority; prerequisite)

- **RF1.1** Create `events::diff(prev: &ExportAccount, next: &ExportAccount, state: &mut EventState, now) -> Vec<Event>`, a pure function that compares two consecutive readings and emits edge events. It reuses the reset planner (PRD 0003), the threshold evaluator (PRD 0008) and the binding metric (PRD 0009) when they exist, without duplicating their rules.
- **RF1.2** Event types (stable names): `quota_threshold_crossed`, `quota_exhausted`, `quota_recovered`, `quota_reset`, `account_blocked`, `account_unblocked`, `refresh_failed`, `refresh_recovered`, `auth_required`, `provider_incident` (if PRD 0011 exists) and `usage_updated`.
- **RF1.3** Common payload: `schema`, `event`, `occurred_at`, `account_key`, `account_name`, `provider`, `metric` (label and window, when applicable), relevant values and `previous`. `refresh_failed` carries only a category (`timeout`, `offline`, `auth_required`, `rate_limited`, `schema_changed`, `provider_error`), never the raw message, URL or body.
- **RF1.4** Edge firing: a condition that persists does not repeat the event. The first reading only establishes the baseline and emits no transition events; `usage_updated` is limited to one attempt per 600 s per account.
- **RF1.5** The transition state is in memory; restarting `watch` recreates the baselines. This is documented.
- **Acceptance criterion:** table tests with sequences of readings (rise, fall, reset, oscillation, error and recovery, first reading), and a test that no payload contains credentials, URLs or raw messages.

### RF2 — `cuotai --watch` (high priority)

- **RF2.1** `cuotai --watch [--interval S] [--events list] [--jsonl]`: runs the core's refresh cycle (without the TUI), compares each reading with the previous one and emits the selected events. A minimum interval of 60 s (smaller values are rejected, not silently adjusted). Accounts obey the effective interval and AWS Bedrock's 300 s floor.
- **RF2.2** `--jsonl` writes one event per line to `stdout` (NDJSON) with `schema` and a flush on each line; human messages and diagnostics go to `stderr`.
- **RF2.3** Without `--jsonl` and without hooks, it prints a readable summary of the transitions.
- **RF2.4** It shuts down cleanly on `SIGINT`, `SIGTERM` and `SIGHUP`. Codes: 0 normal shutdown; 1 local error; 2 if it ended with accounts in persistent failure.
- **RF2.5** It can coexist with `--serve` (PRD 0012) in the same process, sharing the cycle.
- **Acceptance criterion:** an integration test with a `wiremock` server and an accelerated clock; a smoke test with `jq` consuming the NDJSON; clean shutdown.

### RF3 — Configurable hooks (medium priority)

- **RF3.1** Configuration in a file of its own, `hooks.toml`, next to `cuotai.toml` (in the same configuration directory), never inside the accounts and without secrets. Each rule has: `name`, `events` (a list), optional filters (`provider`, `account`, `min_threshold`) and `command` as an **argument vector** (`["/path/script", "--flag"]`), with no implicit `sh -c`.
- **RF3.2** Off by default. Hooks only run with `--watch --hooks` or when enabled in the desktop app, and the first run shows the list of commands and requires explicit user confirmation. Changing the file requires a new confirmation (a hash of the commands).
- **RF3.3** Execution: the event arrives on `stdin` as JSON and through the environment variables `CUOTAI_EVENT`, `CUOTAI_ACCOUNT`, `CUOTAI_PROVIDER` and `CUOTAI_METRIC` (values always treated as data; never interpolated into the command). The child process environment is reduced to an allow-list (`PATH`, `HOME`, `LANG`), inheriting no credential variables. Neutral working directory.
- **RF3.4** Limits: a default timeout of 10 s (configurable up to 60 s), captured output limited to 16 KiB, at most 4 simultaneous hooks and a bounded queue that drops the oldest and warns. Hook failures never take `watch` down nor change the query cycle.
- **RF3.5** Each run records, in a local log without the payload, the rule name, the event, the duration and the exit code.
- **Acceptance criterion:** tests with fixture scripts: an argument with quotes and `;` arrives as data; reduced environment (credentials absent); a timeout kills the process; output above the limit is truncated; confirmation is mandatory; a changed hash requires reconfirmation.

### RF4 — Desktop (low priority)

- **RF4.1** In **Settings → Integration**, an editor for the rule list (name, events, command as an argument list), a test button that fires a sample event and a log of the latest runs. The desktop app uses the same core engine over the cycle already running.
- **RF4.2** The RF3.2 confirmation flow appears as a dialog with the exact list of commands.
- **Acceptance criterion:** a smoke test on the packaged app: create a rule, confirm, fire a test, see the log and turn it off.

## 6. Non-functional requirements

- **Security:** hooks run user code. The design avoids injection (arguments as a vector, data through `stdin` and environment), isolates the environment and requires confirmation. Cuotai never downloads, generates or modifies commands.
- **Privacy:** payloads contain no credentials, URLs, free-form notes or raw error messages.
- **Cost:** `watch` uses exactly the same scheduling as the app; it creates no extra queries.

## 7. Order of execution and verification

1. **RF1:** the event engine with table tests.
2. **RF2:** `--watch` and NDJSON.
3. **RF3:** hooks, with the isolation tests.
4. **RF4:** desktop.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when a script receives exactly one event per real transition, a persistent condition does not repeat it, hooks only run after confirmation, never receive credentials in the environment or the payload, and a hook failure does not affect monitoring.
