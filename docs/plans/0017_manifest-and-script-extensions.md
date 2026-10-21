# PRD 0017 — Manifest and script extensions

| Field | Value |
|---|---|
| ID | 0017 |
| Status | In progress |
| Owner | `cuotai-core`, TUI and desktop |
| Consumers | `crates/cuotai-tui`, `apps/desktop`, extension authors |
| Depends on | PRD 0005 (RF1: the `Extension` level) |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-21)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.64.0 | `cuotai_core::extensions`. RF1.1: `default_dir()` is `<OS local data dir>/cuotai/extensions`; each folder holds a `manifest.toml` (`MANIFEST_FILE`) with `id` (a slug: lowercase letters, digits, `-` and `_`, up to 64 characters), `name` (up to 64 characters), `version` (up to 32), `schema` (`SCHEMA` = 1), `command` (an argument vector whose program is a relative path without `..` naming a file inside the folder, resolved through links and executable on Unix; never an absolute path or a `PATH` name), `interval_seconds` (at least `MIN_INTERVAL_SECONDS` = 60), `timeout_seconds` (`DEFAULT_TIMEOUT_SECONDS` = 15, 1 to `MAX_TIMEOUT_SECONDS` = 60), optional `description` (up to 500 characters) and `homepage` (`http(s)` without credentials) and `env` (up to 32 variable names, repetitions dropped). RF1.2: `discover(root)` runs nothing: every non-hidden folder becomes a `Manifest` (with the resolved `dir` and `program`) or a `Rejected { dir, id, reason }` whose `extensions.error.*` reason names the field or line, never the file's content (a missing manifest, one that is not a regular file (never opened, so a FIFO cannot block discovery), over 64 KiB, not UTF-8, invalid TOML, a missing or newer `schema` checked before the other fields, an unknown field, each invalid value, a command outside the folder, every folder sharing an `id`); a missing directory has no extensions and an unlistable one is a single rejection, so nothing else stops loading. RF1.3: `Discovery::catalog()` gives each valid extension's `CatalogEntry` (`extensions.catalog.label` "Extensão: {name}", the new `access::Level::Extension` and its warning `access.risk.extension` "Executa um programa local; revise o código antes de ativar."), in pt-BR, en-US and es; `docs/export-v1.schema.json` and `docs/export-v1.md` list `extension` among the `access.level` values (additive within schema 1). The TUI wizard and the desktop dialog do not offer it yet (decision below). |
| RF2 | 0.65.0 | `cuotai_core::extensions::output`. RF2.1: `check(id, stdout)` reads one JSON object `{ "schema": 1, "status": …, "metrics": […], "notes": […] }`; each metric carries the `Metric` fields (`label`, `unit` and `kind` required; `used`, `limit`, `remaining`, `window`, `period` and `resets_at` absent or `null` stay unknown, never zero); fields this version does not know are ignored, so schema 1 can grow additively. RF2.2: the output is refused past `MAX_BYTES` (256 KiB), when it is not UTF-8, empty, not valid JSON (`NaN`, `Infinity` and `1e999` included, reported by line and column), not an object or without `schema` = 1 (checked before anything else); otherwise every violation is listed by field path (`metrics[2].kind`), never by value: more than `MAX_METRICS` (64) metrics or `MAX_NOTES` (16) notes, a missing or mistyped field, a non-finite number, a date that is not RFC 3339, an unknown `status`, `kind` or `window` (the message lists the accepted names), a `unit` over `UNIT_MAX_CHARS` (16) or with control characters, a label empty once its markup is removed. An invalid output becomes `Status::Error` without metrics and with the single note `extensions.output.invalid` ("{count} problem(s); no value was used"), which `events::FailureCategory::of` classifies as the new `invalid_output` category (`docs/events.md`). RF2.3: `Checked::into_report(account, provider, checked_at)` gives the `Report` with `source = "extension:<id>"`; labels (`LABEL_MAX_CHARS` 80), periods (`PERIOD_MAX_CHARS` 64) and notes (`NOTE_MAX_CHARS` 500) go through the status feeds' markup cleaning (`incidents::plain_text`) and truncation, notes left empty are dropped and each note keeps the id `extensions.output.note`, so no text can pose as a Cuotai note; any `status` other than `ready` is a plain `Error` without metrics, led by `extensions.output.failed` (never a login, configuration or rate-limit state). Texts in pt-BR, en-US and es. RF2.4 is delivered with the RF3 runner and RF4 consent below. |
| RF2.4, RF3, RF4 | 0.66.0 | `cuotai_core::extensions::{consent, runner}` and `cuotai --extension-check <id>`. RF3.1: no shell — the manifest's vector is the `argv`, the working directory is the extension's folder, `stdin` is closed. RF3.2: the environment is reduced to `PATH`, `HOME`, `LANG` (`SystemRoot` on Windows) and only the `env` names the manifest asks for and the user approved; no Cuotai/vault/account credential is ever injected. RF3.3: the manifest's time limit kills the whole process tree (a process group on Unix, a kill-on-close Job Object on Windows; cancellation/drop covered by `kill_on_drop` plus the tree guard), `stdout` is captured up to `output::MAX_BYTES` + 1 byte and `stderr` drained and discarded (neither reaches reports, logs or errors), at most `runner::MAX_RUNNING` = 2 extensions at once, at low OS priority; network and files are not restricted, and the disclosure and docs say so. RF3.4: each extension id has its own schedule with `RefreshCycle`'s backoff (`backoff_for`, `BACKOFF_CAP_SECONDS`); a slow or failing extension never delays another. RF4.1: every extension is born disabled; `consent::disclosure` lists deterministically the exact command, the directory, the requested variables and the interval, and a typed confirmation approves. RF4.2: `consent::Consent` stores atomically (SHA-256 of the manifest and the program, the approved `env` set); `check` and `Approved::verify` refuse any change until a new approval, and `runner::Runner::run` accepts only an `Approved`. RF4.3: Cuotai never installs, downloads or updates extensions. RF2.4: `--extension-check` runs an approved extension once and shows the normalized reading or the contract violations, never the raw output (exit 0 ready, 2 failure/violations, 1 local error). |

**Decision (2026-10-21, orchestrator):** the RF1.3 catalog entry stays out of the TUI wizard and the desktop account dialog: RF5 delivers listing, enabling, testing and creation of extension accounts in those front ends. No account type is offered before RF5 can provide that interface, even though the core runner and consent are now available.

**Decision (2026-10-21, orchestrator):** RF2.4 `--extension-check` ran the extension with the RF3 runner and RF4 consent, so it was delivered in 0.66.0; RF2 is now complete.

## 1. Context

Every Cuotai provider is a Rust adapter compiled into `cuotai-core` (`providers/*.rs`), chosen by an exhaustive `match` on `Provider` (36 variants). Adding a source, however small (a company-internal dashboard, the API of a small provider, the balance of your own proxy), requires changing the code, recompiling and publishing a new version. The model catalog and the About screen already follow the idea that lists and texts belong to the core, but there is no extension point for third-party data.

In the 2026-10-19 market analysis, ClaudeBar accepts "any quota source a script prints" through a manifest and a script in `~/.claudebar/extensions/`; CodexBar documents a provider-authoring guide and a custom-provider design.

## 2. Problem

- Sources specific to a user or a company will never enter the official catalog.
- Today the only way out is a fork.
- A badly made extension mechanism runs arbitrary code with the user's powers and turns the read-only monitor into a risk.

## 3. Goals

- Let the user add usage sources through a declarative manifest and a local executable that prints JSON, without recompiling Cuotai.
- Keep the read-only principle: the extension reports numbers; Cuotai never hands it the user's credentials nor grants it other powers.
- Treat the extension as untrusted code from the data point of view: validate everything it returns.

## 4. Non-goals

- Plugins loaded in-process (dynamic libraries or WASM) and a graphical interface API.
- Distribution, a store or extension updates.
- Running extensions in the background without the user having enabled each one.
- A full operating-system sandbox (the PRD defines mitigations; it does not promise strong isolation).

## 5. Functional requirements

### RF1 — Manifest and discovery (high priority; prerequisite)

- **RF1.1** An extensions directory in the user's data directory (`<data>/cuotai/extensions/<id>/`). Each extension has a `manifest.toml` with: `id` (a slug), `name`, `version`, `schema` (the contract version, 1), `command` as an **argument vector** relative to the extension's directory, `interval_seconds` (minimum 60), `timeout_seconds` (default 15, maximum 60), an optional `description` and `homepage`, and `env` (the list of **names** of environment variables the extension asks for).
- **RF1.2** The core discovers the manifests without running anything, validates the fields (`deny_unknown_fields`) and exposes the list with the reason for each rejection. An invalid manifest never brings down the loading of the other extensions or of the rest of the configuration.
- **RF1.3** An extension appears as a catalog account type "Extension: {name}" in the wizard, with the `Extension` access level (PRD 0005) and the warning "runs a local program; review the code before enabling".
- **Acceptance criterion:** discovery tests with valid manifests, unknown fields, `interval_seconds` below the minimum, a command outside the extension's directory and duplicate ids.

### RF2 — Output contract (high priority)

- **RF2.1** The extension prints a single JSON object to `stdout`: `{ "schema": 1, "status": "ready", "metrics": [...], "notes": [...] }`. Each metric uses the same fields as `Metric` (`label`, `used`, `limit`, `remaining`, `unit`, `kind`, `window`, `period`, `resets_at`); absence is `null` and never zero.
- **RF2.2** The core validates the JSON against the contract: a maximum size of 256 KiB, at most 64 metrics, truncated texts, finite numbers, valid dates, known `kind` and `window`, a short `unit`. An invalid response produces an `Error` report with the category `invalid_output`, without forwarding the content.
- **RF2.3** The resulting `Report` uses `source = "extension:<id>"`, and the extension's free-form notes go through markup cleaning and truncation. The core never trusts `status` or text from the extension to decide behavior.
- **RF2.4** A `cuotai --extension-check <id>` command runs the extension once and shows the normalized report and the contract violations.
- **Acceptance criterion:** table tests with valid, partial, oversized, `NaN`/infinity, unknown `kind` and malformed JSON outputs.

### RF3 — Isolated execution (high priority)

- **RF3.1** Execution without a shell, with the manifest's vector as `argv`, the working directory in the extension's folder and `stdin` closed.
- **RF3.2** The child process environment is reduced to `PATH`, `HOME`, `LANG` and **only** the variables the manifest lists in `env` and the user approved. Cuotai never injects vault tokens nor other accounts' credentials.
- **RF3.3** Limits: the manifest's maximum time, limited output, orphan processes terminated when time runs out (a process group), at most 2 simultaneous extensions and low priority. Network and files are not restricted by Cuotai; the documentation says this clearly.
- **RF3.4** An extension's failure or slowness does not affect the cycle of the other accounts, applying the same per-account deadline and the same backoff as the `RefreshCycle`.
- **Acceptance criterion:** tests with fixture scripts: reduced environment, a timeout with an orphan child, excessive output, and a slow extension that does not delay other accounts.

### RF4 — Consent and integrity (high priority)

- **RF4.1** Every extension is born **disabled**. Enabling it requires a screen with the full command, the directory, the environment variables requested and the interval, and an explicit confirmation.
- **RF4.2** The core stores the digest (SHA-256) of the manifest and the executable at the moment of approval. Any change disables the extension until a new approval.
- **RF4.3** No extension is installed, downloaded or updated by Cuotai.
- **Acceptance criterion:** tests for approval, an executable change (disables) and a manifest change (disables).

### RF5 — Interfaces (medium priority)

- **RF5.1** The TUI and the desktop app list extensions (state, last run, error), allow enabling, disabling and testing, and create the matching account with the same flow as catalog accounts. The interfaces have no extension-specific rules beyond the list supplied by the core.
- **RF5.2** `docs/extensions.md` documentation with the contract, a minimal example in shell and another in Python, security guidance and the `schema` compatibility policy.
- **Acceptance criterion:** a smoke test in three languages: enable, see the metrics on the card with the `Extension` label, disable and remove.

## 6. Non-functional requirements

- **Security:** extensions run with the user's powers. The design limits what Cuotai hands them (no credentials), validates what they return and requires consent and integrity. The residual risk (the extension does whatever it wants with the user's network and files) is documented.
- **Read-only:** the contract has no actions; the extension returns data.
- **Compatibility:** a versioned `schema`, additive within the version.

## 7. Order of execution and verification

1. **RF1 and RF2:** manifest, contract and `--extension-check`.
2. **RF3 and RF4:** isolated execution, consent and integrity.
3. **RF5:** interfaces and documentation.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when a sample extension appears as an account, only runs after being approved, stops running if its executable changes, never receives credentials from Cuotai, and an invalid or slow output becomes an isolated error without affecting the other accounts.
