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

- `schema` is `1`. Within version 1 the contract is **additive**: new fields, new objects and new enum values may appear (`incident` joined in 0.49.0 with PRD 0011). Consumers must ignore properties they do not know and treat an enum value they do not know as **"other"**.
- **Enums are open.** In [`export-v1.schema.json`](export-v1.schema.json) every enum is `anyOf` its known values and any `string`: the listed values are the ones documented today, not a closed set, so a strict validator accepts a value added later. The value lists in this document are the known values.
- Removing a field, renaming it, changing its type or changing its meaning requires `schema: 2` (a new number; the old meaning is never changed in place).
- **Support window (PRD 0031 RF5.1):** when schema 2 ships, schema 1 is still served for **at least 90 days** after that release: `/v1/...` stays while `/v2/...` exists and `cuotai --json-v1` keeps printing schema 1. `/health` lists the numbers served in `schemas` (today `[1]`), next to `schema`. The changelog announces the release of the new schema and the end of the old one's window in advance. The events payload ([`events.md`](events.md#contract-policy)) follows the same policy.
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
| `id` | string | Stable account id (PRD 0029): `acc_` plus 12 lowercase base32 characters, or a hand-written `[a-z0-9_-]{4,40}`. It survives renames, reordering and parameter edits; prefer it to `key`. |
| `key` | string | Legacy identity `<provider slug>:<name>` (`claude:Work`). It survives reordering; a rename gives a new key, so prefer `id`. Kept unchanged for compatibility. |
| `name`, `provider` | string | Account name and provider slug ([`providers.md`](providers.md)). |
| `email` | string or `null` | Only when the producer includes identity; `null` when the e-mail is unknown. Absent by default. |
| `class` | `available` / `exhausted` / `unmeasured` | Classification of `report` (`unmeasured` without a report or when it failed). |
| `report` | object or `null` | The current report (`account`, `provider`, `checked_at`, `status`, `metrics`, `notes`, `notes_l10n`, `source`), the same shape as in `--json`, after redaction. `null` until the account has a report (first query or an edit pending). |
| `age_seconds` | integer or `null` | Age of `report.checked_at`. |
| `freshness` | `fresh` / `aging` / `stale` or `null` | `report.checked_at` against the account's effective refresh interval: `fresh` up to 1.5 times, `aging` up to 3 times, `stale` above. |
| `clock_adjusted` | boolean | `report.checked_at` is in the future (the system clock went backwards); the age then reads `0`. |
| `stale` | boolean | `true` exactly when `last_good` is present: the current query failed (or is pending) and what is known of the account is an earlier reading. |
| `showing_last_good` | boolean | Alias of `stale` with the same value and a name that says what it means (additive). Prefer it in new code; `stale` is not going away within schema 1. |
| `last_good` | object or `null` | The last successful reading (`checked_at`, its original time; `age_seconds`; `metrics`), offered only while `report` is missing or failed. It is never current data and never feeds `class`, `binding` or `pace`. An authentication failure discards it. |
| `binding` | object or `null` | The metric that restricts the account most (`index` into `report.metrics`, `label`, `window`, `used_ratio`, `remaining`, `resets_at`, `reason` `exhausted`/`tightest`, `blocked_until`). `null` for a failed or unmeasured report. |
| `pace` | array | One entry per `report.metrics` item, in the same order: the consumption pace until the reset (`elapsed_fraction`, `expected_used_fraction`, `used_fraction`, `delta_points`, `delta_ratio` (the deviation relative to the expected use, `(used − expected) / expected`; `null` below a 10% expected share; added after 0.80.0), `stage`, `tone`, `source`, `resets_at`, `projected_exhaustion_at`, `lasts_until_reset`), or `null` for a metric without one (not a quota, no window, no cap, the window just opened, or reset since the reading). It is computed at the reading's `checked_at`, not at `generated_at`, and every entry is `null` while `freshness` is not `fresh`: an aging or stale reading has an age, never a pace. The shape is unchanged; since 0.80.0 a non-fresh reading reports `null` where earlier versions projected an old value against the current clock. Empty without a report. |
| `access` | object | How the account's usage is read: `level` (`official`, `undocumented`, `derived`, `unavailable`, `extension`), `credential` class, `plan_requirement`, `scope`, `metered`, public `evidence` with `verified_on`, and localized `summary`, `credential_hint`, `risk` and `reason`. |
| `incident` | object or `null` | The public status of the account's provider (PRD 0011): `state` (`operational`, `degraded`, `partial_outage`, `major_outage`, `maintenance`, `unknown`), its localized `label`, the most severe active `event` in the provider's scope (`title` without markup, `started_at`, public `url` on the status host) or `null`, the `page_url`, `read_at` (`null` before the first reading), `stale` (the latest status query failed and `state` is the previous reading, or `unknown` without one) and `expired`. A retained reading older than 30 minutes has `expired: true`, `state: "unknown"` and `event: null`, while `read_at` preserves when it was checked. `null` while status queries are off or when the provider has no verified source; it is never `operational` without a valid reading. |

Localized texts are objects `{ "id", "params", "fallback" }`: resolve `id` in your own catalog (Cuotai's are `crates/cuotai-core/locales/*.json`) or show the Portuguese `fallback`.

## Overage and model scope

A metric whose amount is billed on top of the plan (Claude extra usage, Cursor and Grok on-demand, Kiro overage) carries `"billing": "overage"`; plan metrics omit the field. A metric that limits one model carries `"scope": {"kind": "model", "id", "label"}`; account-wide metrics omit it. Both are additive. Consumers that read `kind = "quota"` as the plan limit should skip metrics with `billing` and treat metrics with `scope` as limits of that model only, as Cuotai does for the binding limit and the account state.

## Limits summary (`Limits`)

`Limits::from_export(&export)` is the body of the local server's `/v1/limits`: `schema`, `generated_at` and, per account in configuration order, `index`, `id`, `key`, `name`, `provider`, `class`, `binding`, `age_seconds`, `freshness` and `stale`, with the same meaning as in `AccountExport`. It is part of version 1 under the same additive policy and is declared as `$defs/Limits` in the schema (checked by `export::v1::tests::limits_summary_follows_its_schema`).

## The three "stale" fields

Three different fields are called `stale`, or mean it; they are not interchangeable.

| Field | Where | Exact meaning |
|---|---|---|
| `accounts[].stale` (alias `showing_last_good`) | `AccountExport`, `Limits` accounts | A preserved earlier reading (`last_good`) is shown instead of a current one: the latest query failed or is still pending. It says nothing about how old the reading is. |
| `accounts[].freshness` = `stale` | `AccountExport`, `Limits` accounts | The age of `report.checked_at` is above 3 times the account's effective refresh interval (`fresh` up to 1.5, `aging` up to 3). It is about time only, whatever the status of the report. |
| `accounts[].incident.stale` | `AccountExport` | The latest query of the provider's **public status page** failed: `incident.state` is the previous status reading (or `unknown` without one). It is about the provider status, not the account's usage. |

No meaning changes within schema 1.

## Missing values

A value the provider did not report stays `null`; it never becomes `0`. This holds for metric `used`, `limit`, `remaining`, `window`, `period` and `resets_at` (including non-finite numbers), and for every derived field above that has nothing to derive from.

## What never appears

- Credentials, tokens or cookies, and the names of credential environment variables: in `notes`, `notes_l10n` (texts and parameters) and `source`, the account's `credential_env`, its `*_env` parameters, the provider's default variable and any `UPPER_SNAKE` name are replaced with `[redacted]`.
- Authenticated URLs: URLs in notes and `source` lose their user, password, query and fragment; a URL that does not parse becomes `[redacted]`.
- Response bodies: providers never put them in notes.
- E-mails, unless the producer turns on "include identity": the account's `email` field is then absent, and every e-mail address in the text of notes, `notes_l10n` and `source` (the account's own and any address a provider reported, such as Cursor's account or Perplexity's member filter) is replaced with `[redacted]`; URLs lose their user part as above.
- The TOML path (only its digest) and account configuration such as `credential_env`, `oauth` or `params`.

The export holds only what the dashboard already shows. Any process that can read it, for example any local process of the same user while the [local server](local-server.md) is on, can read your usage data.

## Widget snapshot (`widget.json`)

PRD 0014 projects a compact subset of this contract into a file for system widgets and third-party panels: `<local data>/cuotai/widget.json` (the *app group* container on a macOS build with the entitlement). It has **its own** `schema` version and is documented in [widgets.md](widgets.md): what it carries (binding metric, up to four metrics with a percentage, `checked_at`, freshness, pace and a short history series), what it never carries (notes, credentials, e-mail unless identity is on), the deterministic truncation at the account limit and 256 KiB, the atomic `0600` write, and the once-per-15-seconds rate limit.
