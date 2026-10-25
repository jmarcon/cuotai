# Events (`events`)

`cuotai-core`'s event engine (PRD 0013 RF1) turns two consecutive readings of one account into **edge events**: one event per real transition, with a stable name and a payload free of secrets. `cuotai --watch` streams them (PRD 0013 RF2), and `cuotai --watch --hooks` and the desktop app's Settings › Advanced › Integrations run your own commands on them (PRD 0013 RF3 and RF4).

## Watching from the command line (`cuotai --watch`)

```sh
cuotai --watch                                   # one readable line per transition on stdout
cuotai --watch --jsonl | jq -c 'select(.event == "quota_exhausted")'
cuotai --watch --jsonl --events quota_threshold_crossed,refresh_failed --interval 120
cuotai --watch --jsonl --serve                   # the local server on the same refresh cycle
```

- **Refresh cycle.** `--watch` runs the same refresh cycle as the dashboard, without the TUI and without needing a terminal: a first query of every account at start, then each account at its own interval with the core's backoff. Watching adds no query: events come from comparing each new reading with the previous one. With `--serve` (PRD 0012) the local server reads the same cycle, so one query feeds both.
- **`--interval S`** (60 to 86 400 seconds) replaces the global `refresh_seconds` for this run only (nothing is saved). An account with its own `refresh_seconds` keeps it, and AWS Bedrock, billed per query, is never queried more often than every 300 s. A smaller value is rejected with exit code `1`, never silently raised.
- **`--events list`** keeps only the named events (comma-separated payload names; all of them by default). An unknown name is rejected with exit code `1` and the list of valid ones. Events left out still advance the engine, so selecting never changes which transitions are real.
- **`--jsonl`** writes one payload (below) per line to stdout (NDJSON), flushed after each line; every human message and diagnostic goes to stderr, and with `--serve` the server's address goes to stderr too. Without `--jsonl` (and without `--hooks`, which leaves stdout empty unless `--jsonl` is given) each transition is one readable line in local time and the `CUOTAI_LANG` language (the metric label is the provider's, as on the cards), such as `2026-10-20 14:02:11  Work (Claude): Sessão (5h): 90% used`.
- **Baselines.** The first reading of each account is only its baseline: a run prints nothing until something changes. `provider_incident` needs `--status`.
- **Shutdown and exit codes.** `SIGINT`, `SIGTERM` and `SIGHUP` (Ctrl+C and Ctrl+Break on Windows) stop it cleanly, and so does a closed stdout when the next event is written (the consumer of the stream went away). `0`: stopped by a signal, whatever the accounts' state (PRD 0031 RF10; earlier releases exited `2` for a failing account); with **`--strict-exit`**, `2` when some account's latest reading had failed at shutdown (or, with `--serve`, no account was ever healthy), for supervisors that want that; `1`: local error (invalid arguments, interval, events or configuration).

## Hooks (`cuotai --watch --hooks`)

Hooks run commands of your own when events happen: play a sound, write to a file, notify a chat through a script of yours. They are **off by default**: they only run with `cuotai --watch --hooks` or while turned on in the desktop app ([below](#hooks-in-the-desktop-app)), and only after you confirmed their exact commands.

```toml
# hooks.toml, in the same directory as the accounts' TOML (cuotai.toml or config.toml).
# No accounts and no secrets here: a command that needs a secret reads it itself.

[[hooks]]
name = "sound"                                    # unique, up to 64 characters
events = ["quota_exhausted", "quota_threshold_crossed"]
provider = "claude"                               # optional: only this provider (slug, as in --list-providers)
account = "Work"                                  # optional: only the account with this name (an aggregator's name also covers its connections)
# account_id = "acc_0123456789ab"                 # optional: only the account with this id (survives renames)
min_threshold = 90                                # optional: crossings at or above 90% (other events pass)
command = ["/usr/bin/afplay", "/System/Library/Sounds/Glass.aiff"]
timeout_seconds = 10                              # optional: 1 to 60, 10 by default

[[hooks]]
name = "journal"
events = ["refresh_failed", "auth_required"]
command = ["/Users/me/bin/cuotai-journal.sh", "--append"]
```

```sh
cuotai --watch --hooks              # hooks only; stdout stays empty
cuotai --watch --hooks --jsonl      # hooks and the NDJSON stream
```

- **File.** `hooks.toml` lives next to the accounts' TOML (`--config ~/x/cuotai.toml` reads `~/x/hooks.toml`). Each `[[hooks]]` rule has `name`, `events` (payload names), the optional filters `provider`, `account` (the display name: the configured name, or the provider's name for an unnamed account; not unique across providers), `account_id` (the stable id of `--json-v1`, which a rename keeps) and `min_threshold`, a `command` and an optional `timeout_seconds`. Any other key is rejected, as is a file with no rule; an invalid file is a local error (exit `1`) that names the line or the rule, never a rule that is silently skipped. `--events` also limits what reaches the hooks.
- **The command is an argument vector,** never a shell string: `command = "script; rm -rf ~"` is rejected, and `["/path/script", "it's; $(id)"]` runs `/path/script` with that exact text as its one argument. The program is an absolute path or a bare name looked up in `PATH` (a relative path is rejected); `~` and variables are not expanded. To use a shell, say so explicitly (`["/bin/sh", "/path/script.sh"]`).
- **Confirmation.** The first time, and whenever the rules change (any name, event, filter, command or time limit; comments and layout do not count), `--watch --hooks` lists every rule with its exact command (as a JSON array, so quotes and separators are visible) on stderr and asks you to type `sim`/`yes`/`si` (per `CUOTAI_LANG`); any other answer declines and nothing runs. Without a terminal (a service, a pipe on stdin) unconfirmed hooks are a local error (exit `1`) before any query: confirm them once in a terminal or in the desktop app. The confirmation is the SHA-256 of the rules, kept per `hooks.toml` in `<OS local data dir>/cuotai/hooks/consent.json`; Cuotai never downloads, generates or changes a command (the desktop editor writes only what you typed).
- **What a hook receives.** The event payload (below) as one JSON line on stdin, and the variables `CUOTAI_EVENT` (the event name), `CUOTAI_ACCOUNT` (the account's display name), `CUOTAI_ACCOUNT_ID` (its stable id), `CUOTAI_PROVIDER` (the provider slug) and `CUOTAI_METRIC` (the metric label, empty for account-wide events). They are data, never part of the command line: quote them in your script (`"$CUOTAI_ACCOUNT"`).
- **Isolation.** The environment is reduced to `PATH`, `HOME` and `LANG` (plus `SystemRoot` on Windows, which processes need to start) and the `CUOTAI_*` variables: no credential variable (`OPENAI_ADMIN_KEY`, `AWS_*`, …) or `CUOTAI_CONFIG` reaches a hook. The working directory is Cuotai's private `<OS local data dir>/cuotai/hooks/work` (created `0700` on Unix; never the shared temporary directory, and a hook does not start when that folder cannot be made private, for example when it is a symbolic link). stdout and stderr are captured (never mixed into the event stream), up to 16 KiB per run.
- **Limits.** A hook that outlives its `timeout_seconds` is killed together with every process it started (its own process group on macOS and Linux, a kill-on-close Job Object on Windows), and whatever a hook leaves running when it exits is killed too, so a background process never outlives its hook or escapes the four-at-once limit; a hook that must start something long-lived should hand it to the system (for example `systemd-run --user` or `launchctl`). A process that starts its own session (`setsid`), or on Windows one started before the hook joined its Job Object, can still escape. At most 4 hooks run at once; up to 64 more wait in a queue, and beyond that the oldest waiting run is dropped with a warning on stderr. A hook that fails, hangs or cannot start never stops `--watch` or changes when accounts are queried: a run that did not succeed is one line on stderr (`cuotai: hook sound (quota_exhausted): exited with code 3 after 120 ms`). Stopping `--watch` drops the queue and kills the hooks still running.
- **Run log.** Each run appends one JSON line, without the payload or the output, to `<OS local data dir>/cuotai/hooks/runs.log` (private to your user; past 256 KiB it moves to `runs.log.1`): `{"at":"2026-10-20T12:02:00Z","rule":"sound","event":"quota_exhausted","duration_ms":120,"outcome":"exited","exit_code":0}`. `outcome` is `exited` (with `exit_code`), `terminated` (ended by a signal), `timed_out` or `not_started` (missing or not executable program).

A minimal hook script:

```sh
#!/bin/sh
# Append one line per event; the payload on stdin is JSON.
payload=$(cat)
printf '%s %s %s\n' "$(date -u +%FT%TZ)" "$CUOTAI_EVENT" "$CUOTAI_ACCOUNT" >> "$HOME/cuotai-events.log"
printf '%s\n' "$payload" | jq -r '.current.threshold // empty' >> "$HOME/cuotai-thresholds.log"
```

### Hooks in the desktop app

**Settings › Advanced › Integrations › Hooks** runs the same rules inside the desktop app, from the `hooks.toml` next to the accounts' file it has open.

- **Off by default.** The **Run hooks** switch is saved in `desktop.json`; while it is off no command runs.
- **Editor.** Each rule has a name, the events it runs on (a box per payload name) and the command as one field per argument, the first being the program (an absolute path or a bare name looked up in `PATH`); arguments are passed exactly as typed, without a shell. Saving goes through the same validation as `cuotai --watch --hooks` and never replaces a file edited outside the app since it was shown (reload it first). Filters and time limits written by hand in `hooks.toml` are shown and kept; comments of the rules you keep survive a save, those of a removed rule go with it.
- **Confirmation.** Turning hooks on with rules nobody confirmed, saving changed rules while they are on, or **Review commands** opens a dialog with the exact listing `cuotai --watch --hooks` prints (every rule's events, filters, time limit and command as a JSON array); nothing runs until you choose **Run these commands**. The confirmation is the same record the command line uses, so confirming in either one confirms both. When the file changes outside the app the rules stop, Settings asks again and a banner on the main window says so.
- **Events.** The confirmed rules run on the events of the app's own readings (the refresh cycle the window shows), so hooks add no query; the first reading after they start, or after the accounts' file changes, is a new baseline. A hook gets the event exactly as described above, with the same isolation and limits. A desktop app started from the Dock gets the system's short `PATH`, so prefer absolute program paths.
- **Test and log.** **Test** (on a confirmed, running rule) sends it a sample of its first event at once, built to pass its filters and marked `"sample": true`. **Latest runs** lists the last 20 lines of the shared `runs.log` (from the app and from `cuotai --watch --hooks`), newest first, updated when each run ends.

## Producing events (Rust)

```rust
use cuotai_core::events::{diff, EventState};

// One state for the life of the process; keep the previous export of each
// account (before its first reading, an export whose `report` is `null`).
let mut state = EventState::default();
for (prev, next) in previous.accounts.iter().zip(&current.accounts) {
    for event in diff(prev, next, &mut state, chrono::Utc::now()) {
        println!("{}", serde_json::to_string(&event)?);
    }
}
```

`previous` and `current` are `export::v1::Export`s built with `Export::from_cycle` from the refresh cycle the front end already runs ([`export-v1.md`](export-v1.md)): judging them never queries a provider. `EventState::new(thresholds)` picks the crossing thresholds (10 to 99%, 75 and 90 by default). `EventState::retain_accounts(keys)` forgets removed accounts, whose next reading is then a new baseline.

`watch::Watcher` does this bookkeeping for whole exports: `Watcher::new(state, selected).observe(&export, now)` keeps the previous export of each account, treats a new account as a baseline, forgets removed ones and keeps only the `selected` kinds (`watch::parse_events` reads the names). `headless::Driver` runs the refresh cycle without a screen and builds the exports `--watch` observes. `Event::summary()` is the readable line (`events.summary.*`, with the category's `events.category.*` text). `hooks::Hooks::load(&Hooks::path_beside(config_path))` reads `hooks.toml`; `hooks::Consent::check`/`confirm` turn it into a `Confirmed` set (the only input of `hooks::Runner::start(confirmed, log, on_run)`), and `Runner::submit(&event)` queues the matching rules without waiting, returning how many queued runs it dropped.

## Edge rules

- **Baseline.** The first reading of an account in an `EventState` only records the baseline and emits nothing. The same goes for the first `Ready` reading of an account whose readings had only failed (quota events start from it) and for the first status read of a provider.
- **No repetition.** A condition that persists does not repeat its event: a failure that goes on (even if its category changes), a quota that stays exhausted, a usage level that stays above a threshold. An export whose `report` is the one already judged (same `checked_at`) yields no reading events.
- **Thresholds** follow the usage alert rules (PRD 0008): each threshold fires once per window instance, a reading that crosses several fires only the highest, oscillating around a threshold stays silent and a threshold re-arms after consumption falls 5 points below it or in a new window instance. Only a `quota` metric with a positive cap counts.
- **`usage_updated`** fires when the usage values (`used`, `limit`, `remaining`) changed since the last `usage_updated` (or the baseline), at most once per 600 s per account; a drifting `resets_at` alone is no change.
- **In memory only.** Nothing is saved: restarting the process (for example `cuotai --watch`) builds new baselines, and transitions that happened while no process was watching are not recovered.

## Event names

| Event | When | `metric` | `current` / `previous` |
|---|---|---|---|
| `quota_threshold_crossed` | A quota's consumption reached a threshold above every level already reached in its window instance. | The quota | `threshold` (percent) and the metric's values |
| `quota_exhausted` | A quota is fully consumed (once per window instance). | The quota | The metric's values |
| `quota_recovered` | A quota whose exhaustion was reported, or was in the baseline, has slack in a new window instance. | The quota | The metric's values |
| `quota_exhaustion_projected` | At the recent pace a quota runs out before its reset, within the lead (60 minutes, or `--projected-lead 15\|30\|60` of `cuotai --watch`). A linear estimate from recent readings: only from the slope of at least 3 recorded samples spanning 30 minutes (or once 20% of the window elapsed), on a fresh reading, with at least 25% used, confirmed by the previous reading, and at least 5 minutes before the reset. Once per window instance; `min_threshold` does not apply to it. | The quota | `projected_at`, `resets_at`, `lead_minutes`, `used_ratio` and `source` (`recent`); `previous` is `null` |
| `quota_reset` | A quota's window instance ended (its `resets_at` came) and the reading reports a new one, more than 2 minutes later. A postponed reset is not a reset. | The quota | The metric's values; `previous.resets_at` is the instance that ended |
| `account_blocked` | The account's binding metric (PRD 0009) became exhausted. | The binding metric | The metric's values; `current.blocked_until` when known |
| `account_unblocked` | The binding metric is no longer exhausted. | The metric that blocked | The metric's values; `previous.blocked_until` when it was known |
| `refresh_failed` | A reading failed after a successful one. | `null` | `status` and `category`; `previous.status` |
| `refresh_recovered` | A reading succeeded after a failed one. | `null` | `status`; `previous.status` and `previous.category` |
| `auth_required` | The failure category became `auth_required` (also after another kind of failure). | `null` | `status` and `category` |
| `provider_incident` | The provider's public status (PRD 0011, only while status queries are on) changed. **One event per provider and status source**, not per account (PRD 0031 RF8): it names the first affected account in configuration order and lists all of them in `accounts`. A stale status is no change, and neither is a provider without a status source. A status read as `unknown` after a quiet state (and back) is silent; after an incident it is the resolution, reported once with `current.incident = unknown`. | `null` | `incident` (`operational`, `degraded`, `partial_outage`, `major_outage`, `maintenance`, `unknown`) |
| `usage_updated` | See the edge rules. | `null` | `metrics`: every metric of the reading |
| `overage_started` | The account began paying for extra usage (PRD 0023): its overage state moved from `disabled`, `armed`, `unknown` or no data to `active` (or straight to `cap_reached`). Once per billing cycle, so oscillating amounts and a restart with the saved state do not repeat it. | `null` | `overage_state`, `spent`, `unit`, `currency`, `cap` and `cycle_ends_at`; `previous` holds the same fields of the earlier reading |
| `overage_ended` | An active extra usage went back to `armed` or `disabled`, which is also what a new billing cycle with nothing spent reads. An unreadable amount (`unknown`) is no end. | `null` | The same overage fields |
| `session_expiring` | An account that uses a pasted session (PRD 0016) is within 3 days of the session's expiry; once per account and expiry date (a session pasted again with a new date is noticed again). The first reading of a run only records it, so a restart repeats nothing. Never carries the session value. | `null` | `expires_at` and `days_left` (whole days, from 0) |
| `quota_reset_soon` | A quota window with some consumption is within the reset lead (`--reset-lead MIN`, default 15; `0` turns it off) of its `resets_at`; once per window instance (a `resets_at` drifting by up to 2 minutes is the same instance). A window already inside the lead at the first reading is not reported. Readings further apart than the lead can miss it. Hooks pick their own events; they do not get a separate lead. | The quota | The metric's values; `current.resets_at` is the reset to come |
| `pool_exhausted` | Every active connection of one upstream of an aggregator account (PRD 0036) is exhausted. Sent on the aggregator account. | `label` is the pool's name (`Claude`), `window` is `null` | `resets_at`: the earliest time one connection returns, when known |
| `pool_recovered` | An exhausted pool has a connection available again. | As above | — |

**Aggregators.** Each connection of an aggregator account (for example every connection of a 9router instance) is diffed like an account of its own: its events carry `account_key` = `<aggregator key>/<sub id>`, `account_name` = the connection's label, `provider` = the mapped provider (the aggregator's when the upstream is unknown) and `parent` = the aggregator account's key. Connections that are off in the aggregator or duplicate a direct account have no events. A hook rule with `account = "<aggregator name>"` also runs on its connections' events.

One reading can fire several events; they come in this order: refresh events, `quota_reset`, threshold/exhaustion/recovery events, blocking events, `overage_ended`, `quota_reset_soon`, `usage_updated`, `provider_incident`. `overage_started` and `session_expiring` come with the threshold events. A reset after an exhaustion, for example, fires `quota_reset`, `quota_recovered` and `account_unblocked` together.

## Contract policy

The event payload follows the same policy as [`export::v1`](export-v1.md#stability-and-deprecation-policy) (PRD 0031 RF5):

- `schema` is `1`. Within a schema number the payload is **additive**: new events, new fields and new values of a known field (a `category`, an `incident` state, a `window`) may appear. A removed or redefined field, or a changed meaning, needs `schema: 2`.
- Treat every value you do not know as "other": ignore an event name you do not handle and render an unknown `category` or state generically. Listed names and values are the known ones, not a closed set.
- When schema 2 ships, schema 1 is still produced for **at least 90 days**; the changelog announces both dates.
- Every event carries `schema`, so the first line of `--jsonl` needs no header.

## Payload

| Field | Type | Meaning |
|---|---|---|
| `schema` | integer | Always `1`. Within version 1 the payload is additive: consumers must ignore fields and names they do not know. |
| `event` | string | One of the names above. |
| `occurred_at` | RFC 3339 | When the engine observed the transition. |
| `account_key` | string | `<provider slug>:<account name>`, as in `export::v1`; a rename changes it, prefer `account_id`. |
| `account_id` | string | The account's stable id (`id` in `export::v1`, PRD 0029): it survives renames, reordering and parameter edits. |
| `account_name` | string | The account's display name: its configured name, or the provider's name when it has none (`name` in `export::v1`). |
| `provider` | string | Provider slug. |
| `accounts` | array of strings, or absent | `provider_incident` only: the `account_key` of every account that reads the same provider status, in configuration order, the first being `account_key` (additive; PRD 0029 will add account ids beside the keys). |
| `parent` | string, or absent | Only for an event of an aggregator's connection: the aggregator account's key. |
| `metric` | object or `null` | `label` and `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other` or `null`). |
| `current` | object | The values after the transition. |
| `previous` | object or `null` | The same values before it; `null` when nothing earlier is known. |
| `sample` | `true`, or absent | Present only on the made-up event the desktop's **Test** button sends to a hook ([Hooks in the desktop app](#hooks-in-the-desktop-app)); a real transition never has it. |

The value objects hold only the fields that apply and are known; a missing field is never zero: `status` (the `Report` status), `category`, `threshold`, `used`, `limit`, `remaining`, `unit`, `used_ratio` (consumed fraction of the cap, 0 to 1), `resets_at`, `blocked_until`, `incident`, the extra-usage fields `overage_state` (`disabled`, `armed`, `active`, `cap_reached`, `unknown`), `spent`, `currency` (an ISO 4217 code when `unit` is money, absent for provider units), `cap` and `cycle_ends_at`, `expires_at`, `days_left` and `metrics` (each with `label`, `window`, `used`, `limit`, `remaining`, `unit`, `used_ratio` and `resets_at`, `null` when unknown).

```json
{"schema":1,"event":"quota_threshold_crossed","occurred_at":"2026-10-20T12:02:00Z","account_key":"claude:Work","account_id":"acc_0123456789ab","account_name":"Work","provider":"claude","metric":{"label":"Sessão (5h)","window":"window_5h"},"current":{"threshold":75,"used":80.0,"limit":100.0,"unit":"%","used_ratio":0.8,"resets_at":"2026-10-20T12:20:00Z"},"previous":{"used":50.0,"limit":100.0,"unit":"%","used_ratio":0.5,"resets_at":"2026-10-20T12:20:00Z"}}
```

### Failure categories

A failed reading carries only its category, never the provider's message, URL or response body:

| Category | Cause |
|---|---|
| `timeout` | The query ran out of time (the account deadline or the HTTP client's). |
| `offline` | The provider could not be reached (network failure). |
| `auth_required` | Credentials were refused or expired, a login or credential is missing, or the account needs configuring before it can be read. |
| `rate_limited` | The provider limited the query rate. |
| `schema_changed` | The answer did not match the interface Cuotai knows. |
| `invalid_output` | A script extension's output broke its output contract (PRD 0017 RF2). |
| `provider_error` | Any other failure, including an HTTP error status and a provider that does not offer the query. |

## Privacy

Payloads carry no credentials, credential variable names, e-mails, URLs (neither the query's nor the status page's), provider notes, incident titles or raw error messages. A test feeds readings full of them through every event kind and checks that none reaches a payload.
