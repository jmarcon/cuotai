# Export contract v1 (`export::v1`)

`export::v1` is Cuotai's versioned, read-only description of the last known state of every account, for status bars, scripts and other integrations (PRD 0012 RF1). It is independent of transport: `cuotai-core` builds it in memory, the core's [local server](local-server.md) publishes it (PRD 0012 RF2/RF3; `cuotai --serve` since RF4), and `cuotai --json-v1` prints it after one query (PRD 0012 RF4; an account whose query failed carries its last recorded reading as `last_good`). The `--json` output (`Snapshot`) is still there and keeps its shape.

- Schema: [`export-v1.schema.json`](export-v1.schema.json) (JSON Schema 2020-12).
- Reference output: [`export-v1.example.json`](export-v1.example.json), produced by the core from a fixed fixture and checked by the test `export::v1::tests::export_v1_reference_matches_the_document`.

## Producing it (Rust)

```rust
use cuotai_core::export::v1::{Export, Options};

// `cycle` is the RefreshCycle the front end already maintains and
// `monitor.incident_statuses()` the provider statuses already read (empty while
// status queries are off); building the export never queries a provider.
let export = Export::from_cycle(
    &config,
    &config_identity,
    &cycle,
    &monitor.incident_statuses(),
    chrono::Utc::now(),
    Options::default(),
);
println!("{}", serde_json::to_string(&export)?);
```

`Options::include_identity` publishes account e-mails; it is off by default.

## Stability and deprecation policy

- `schema` is `1`. Within version 1 the contract is **additive**: new fields, new objects and new enum values may appear (`incident` joined in 0.49.0 with PRD 0011). Consumers must ignore properties they do not know and handle unknown enum values gracefully.
- Removing a field, renaming it, changing its type or changing its meaning requires `schema: 2`. When version 2 exists, the producers keep offering version 1 for at least one minor release, and the changelog announces the removal in advance.
- The schema and the reference file change together with the code. The reference test fails on any difference; regenerate it with `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core export_v1_reference` only after checking that the change is additive. The schema test fails when the export emits a field that the schema does not declare.

## Top level

| Field | Type | Meaning |
|---|---|---|
| `schema` | integer | Always `1`. |
| `generated_at` | RFC 3339 | When the export was built. |
| `config_identity` | string | Lowercase hex SHA-256 of the configuration identity (the canonical TOML path, or an explicit id). It tells configurations apart without publishing the path. |
| `accounts` | array | One entry per configured account, in configuration order. |
| `local_cost` | object or `null` | PRD 0015 RF4.3 (since 0.59.0): the estimated cost at API prices from this computer's local CLI logs, apart from every account and never summed with a provider's billing; `null` while every local cost source is off or when the producer does not read the cost index (`Export::with_local_cost`). `currency` (always `USD`), the localized `notice` ("not a charge"), the `span` (`period` or `days`, `label`, `first_day`, `last_day`), `sources` (each with `source`, `label`, the `providers` whose accounts show it, `state` `off`/`ready`/`partial`/`no_logs`/`no_access`/`not_read`, a localized `note` and, while on, `usage` with `today`, `yesterday`, `period` estimates and `days`), the span's `total` and `pricing_error`. An estimate carries tokens, requests, `total` (`null` when nothing is priced), `unknown_tokens` and `unknown_share`, per model its `cost` (`null`: unknown, never 0) and price, and the price tables used. It never holds a path, a file name or conversation content. `cuotai --cost --json` prints the same object. |

## Account (`AccountExport`)

| Field | Type | Meaning |
|---|---|---|
| `index` | integer | Position in the configuration. |
| `key` | string | Stable identity `<provider slug>:<name>` (`claude:Work`), the same key as reset notifications use. It survives reordering; a rename gives a new key. |
| `name`, `provider` | string | Account name and provider slug ([`providers.md`](providers.md)). |
| `email` | string or `null` | Only when the producer includes identity; `null` when the e-mail is unknown. Absent by default. |
| `class` | `available` / `exhausted` / `unmeasured` | Classification of `report` (`unmeasured` without a report or when it failed). |
| `report` | object or `null` | The current report (`account`, `provider`, `checked_at`, `status`, `metrics`, `notes`, `notes_l10n`, `source`), the same shape as in `--json`, after redaction. `null` until the account has a report (first query or an edit pending). |
| `age_seconds` | integer or `null` | Age of `report.checked_at`. |
| `freshness` | `fresh` / `aging` / `stale` or `null` | `report.checked_at` against the account's effective refresh interval: `fresh` up to 1.5 times, `aging` up to 3 times, `stale` above. |
| `clock_adjusted` | boolean | `report.checked_at` is in the future (the system clock went backwards); the age then reads `0`. |
| `stale` | boolean | `true` exactly when `last_good` is present: the current query failed (or is pending) and what is known of the account is an earlier reading. |
| `last_good` | object or `null` | The last successful reading (`checked_at`, its original time; `age_seconds`; `metrics`), offered only while `report` is missing or failed. It is never current data and never feeds `class`, `binding` or `pace`. An authentication failure discards it. |
| `binding` | object or `null` | The metric that restricts the account most (`index` into `report.metrics`, `label`, `window`, `used_ratio`, `remaining`, `resets_at`, `reason` `exhausted`/`tightest`, `blocked_until`). `null` for a failed or unmeasured report. |
| `pace` | array | One entry per `report.metrics` item, in the same order: the consumption pace until the reset (`elapsed_fraction`, `expected_used_fraction`, `used_fraction`, `delta_points`, `stage`, `tone`, `source`, `resets_at`, `projected_exhaustion_at`, `lasts_until_reset`), or `null` for a metric without one (not a quota, no window, no cap, the window just opened). Empty without a report. |
| `access` | object | How the account's usage is read: `level` (`official`, `undocumented`, `derived`, `unavailable`), `credential` class, `plan_requirement`, `scope`, `metered`, public `evidence` with `verified_on`, and localized `summary`, `credential_hint`, `risk` and `reason`. |
| `incident` | object or `null` | The public status of the account's provider (PRD 0011): `state` (`operational`, `degraded`, `partial_outage`, `major_outage`, `maintenance`, `unknown`), its localized `label`, the most severe active `event` in the provider's scope (`title` without markup, `started_at`, public `url` on the status host) or `null`, the `page_url`, `read_at` (`null` before the first reading) and `stale` (the latest status query failed and `state` is the previous reading, or `unknown` without one). `null` while status queries are off or when the provider has no verified source; it is never `operational` without a valid reading. |

Localized texts are objects `{ "id", "params", "fallback" }`: resolve `id` in your own catalog (Cuotai's are `crates/cuotai-core/locales/*.json`) or show the Portuguese `fallback`.

## Limits summary (`Limits`)

`Limits::from_export(&export)` is the body of the local server's `/v1/limits`: `schema`, `generated_at` and, per account in configuration order, `index`, `key`, `name`, `provider`, `class`, `binding`, `age_seconds`, `freshness` and `stale`, with the same meaning as in `AccountExport`. It is part of version 1 under the same additive policy and is declared as `$defs/Limits` in the schema (checked by `export::v1::tests::limits_summary_follows_its_schema`).

## Missing values

A value the provider did not report stays `null`; it never becomes `0`. This holds for metric `used`, `limit`, `remaining`, `window`, `period` and `resets_at` (including non-finite numbers), and for every derived field above that has nothing to derive from.

## What never appears

- Credentials, tokens or cookies, and the names of credential environment variables: in `notes`, `notes_l10n` (texts and parameters) and `source`, the account's `credential_env`, its `*_env` parameters, the provider's default variable and any `UPPER_SNAKE` name are replaced with `[redacted]`.
- Authenticated URLs: URLs in notes and `source` lose their user, password, query and fragment; a URL that does not parse becomes `[redacted]`.
- Response bodies: providers never put them in notes.
- E-mails, unless the producer turns on "include identity": the account's `email` field is then absent, and every e-mail address in the text of notes, `notes_l10n` and `source` (the account's own and any address a provider reported, such as Cursor's account or Perplexity's member filter) is replaced with `[redacted]`; URLs lose their user part as above.
- The TOML path (only its digest) and account configuration such as `credential_env`, `oauth` or `params`.

The export holds only what the dashboard already shows. Any process that can read it, for example any local process of the same user while the [local server](local-server.md) is on, can read your usage data.
