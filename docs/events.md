# Events (`events`)

`cuotai-core`'s event engine (PRD 0013 RF1) turns two consecutive readings of one account into **edge events**: one event per real transition, with a stable name and a payload free of secrets. It is the base of `cuotai --watch` and of the hooks that later PRD 0013 requirements add; today it is a library API.

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

## Edge rules

- **Baseline.** The first reading of an account in an `EventState` only records the baseline and emits nothing. The same goes for the first `Ready` reading of an account whose readings had only failed (quota events start from it) and for the first known provider status.
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
| `quota_reset` | A quota's window instance ended (its `resets_at` came) and the reading reports a new one, more than 2 minutes later. A postponed reset is not a reset. | The quota | The metric's values; `previous.resets_at` is the instance that ended |
| `account_blocked` | The account's binding metric (PRD 0009) became exhausted. | The binding metric | The metric's values; `current.blocked_until` when known |
| `account_unblocked` | The binding metric is no longer exhausted. | The metric that blocked | The metric's values; `previous.blocked_until` when it was known |
| `refresh_failed` | A reading failed after a successful one. | `null` | `status` and `category`; `previous.status` |
| `refresh_recovered` | A reading succeeded after a failed one. | `null` | `status`; `previous.status` and `previous.category` |
| `auth_required` | The failure category became `auth_required` (also after another kind of failure). | `null` | `status` and `category` |
| `provider_incident` | The provider's public status (PRD 0011, only while status queries are on) changed. An unreadable or stale status is no change. | `null` | `incident` (`operational`, `degraded`, `partial_outage`, `major_outage`, `maintenance`) |
| `usage_updated` | See the edge rules. | `null` | `metrics`: every metric of the reading |

One reading can fire several events; they come in this order: refresh events, `quota_reset`, threshold/exhaustion/recovery events, blocking events, `usage_updated`, `provider_incident`. A reset after an exhaustion, for example, fires `quota_reset`, `quota_recovered` and `account_unblocked` together.

## Payload

| Field | Type | Meaning |
|---|---|---|
| `schema` | integer | Always `1`. Within version 1 the payload is additive: consumers must ignore fields and names they do not know. |
| `event` | string | One of the names above. |
| `occurred_at` | RFC 3339 | When the engine observed the transition. |
| `account_key` | string | `<provider slug>:<account name>`, as in `export::v1`. |
| `account_name` | string | The account name in the configuration. |
| `provider` | string | Provider slug. |
| `metric` | object or `null` | `label` and `window` (`window_5h`, `daily`, `weekly`, `monthly`, `other` or `null`). |
| `current` | object | The values after the transition. |
| `previous` | object or `null` | The same values before it; `null` when nothing earlier is known. |

The value objects hold only the fields that apply and are known; a missing field is never zero: `status` (the `Report` status), `category`, `threshold`, `used`, `limit`, `remaining`, `unit`, `used_ratio` (consumed fraction of the cap, 0 to 1), `resets_at`, `blocked_until`, `incident` and `metrics` (each with `label`, `window`, `used`, `limit`, `remaining`, `unit`, `used_ratio` and `resets_at`, `null` when unknown).

```json
{"schema":1,"event":"quota_threshold_crossed","occurred_at":"2026-10-20T12:02:00Z","account_key":"claude:Work","account_name":"Work","provider":"claude","metric":{"label":"Sessão (5h)","window":"window_5h"},"current":{"threshold":75,"used":80.0,"limit":100.0,"unit":"%","used_ratio":0.8,"resets_at":"2026-10-20T12:20:00Z"},"previous":{"used":50.0,"limit":100.0,"unit":"%","used_ratio":0.5,"resets_at":"2026-10-20T12:20:00Z"}}
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
| `provider_error` | Any other failure, including an HTTP error status and a provider that does not offer the query. |

## Privacy

Payloads carry no credentials, credential variable names, e-mails, URLs (neither the query's nor the status page's), provider notes, incident titles or raw error messages. A test feeds readings full of them through every event kind and checks that none reaches a payload.
