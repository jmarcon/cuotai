# PRD 0045 — Breakdowns by key, project, workspace and team for admin and aggregator accounts

| Field | Value |
|---|---|
| ID | 0045 |
| Status | Planned (next version) |
| Owner | `cuotai-core` (`Scope` extension, breakdown rows on `Data`, per-provider breakdown fetchers, caps, redaction, access descriptors), desktop (Breakdown section in details, card sub-rows), `cuotai-tui` (details, `--once`) |
| Consumers | Every front end; `export::v1` (additive `breakdown`), `/v1/accounts/{id}`, PRD 0041 (budgets on a key), PRD 0043 (limit changes on keys) |
| Depends on | PRD 0005 (descriptors per mode), PRD 0023 (`Metric::scope`, `billing`), PRD 0029 (account id), PRD 0033 (metered cadence) |
| Benefits from | PRD 0036 (aggregator section pattern), PRD 0038 (OpenRouter management key), PRD 0028 (canary for the extra endpoints), PRD 0022 (details layout) |
| Extends | PRD 0023's `Scope` (new variants), PRD 0036 (connection cards become one kind of breakdown), PRD 0038 RF (optional credit breakdown) |
| Date | 2026-10-25 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **breakdown** is a set of sub-rows under one account's metric, each for a **sub-scope** the provider reports: an API key, a project, a workspace, a team or a virtual key. It is **not** a per-user report: Cuotai never shows people's names or e-mails.

## Implementation status (2026-10-25)

| RF | Version | Delivery |
|---|---|---|
| RF1–RF8 | — | Proposed. |

## 1. Context

Several Cuotai accounts are **organizational**: `open_ai_api` (optional `params.project_ids`), `anthropic_api` (`params.workspace_ids`), `copilot` in `org` mode, `cursor` Teams, `mistral`, `perplexity` Computer, `windsurf` Enterprise, `litellm`, `nine_router` and `open_router` with a management key. Today they report one total (or a filter chosen in the TOML). The provider dashboards they replace do the opposite: OpenRouter's Activity page "rolls up spend, request, and token metrics you can slice by model, provider, API key, app, or user" and each key reports `usage_daily`, `usage_weekly`, `usage_monthly` and `limit_remaining` ([OpenRouter docs, Activity, read 2026-10-25](https://openrouter.ai/docs/guides/features/activity.md); [team spend controls](https://openrouter.ai/blog/tutorials/team-spend-controls-setup/)); LiteLLM's dashboard "breaks spend and request volume down per virtual key, per user ID, and per team", with budgets at each level ([LiteLLM proxy docs, read 2026-10-25](https://docs.litellm.ai/docs/proxy/alerting.md); [Mittwald LiteLLM guide](https://developer.mittwald.de/docs/v2/platform/aihosting/dedicated/litellm/)); the OpenAI Admin usage endpoints accept `group_by` on `project_id`, `api_key_id` and `model` ([OpenAI Admin API reference](https://developers.openai.com/api/reference/resources/admin/subresources/organization)).

Cuotai already has the primitive: PRD 0023 added `Metric::scope` for a metric that "limits one model (or another sub-scope) rather than the whole account", and PRD 0036 renders an aggregator's connections as sub-cards with pool sentences. What is missing is a generic breakdown for keys, projects, workspaces and teams, with caps, redaction and an opt-in per account.

## 2. Problem

- A team lead watching one OpenAI organization sees $1 240 this month and cannot tell which project moved; the TOML filter is static and needs an edit per question.
- An OpenRouter management key knows each key's limit and usage; Cuotai shows the credit balance only, so a per-key cap about to be hit (the blocking control OpenRouter recommends) is invisible.
- LiteLLM budgets per key and team are the very limits a 429 will enforce, and Cuotai's `litellm` account shows the proxy total.
- Breakdowns are extra requests, sometimes paged and sometimes billed (`aws_bedrock`): they must be opt-in and bounded.

## 3. Goals

- A generic **breakdown** on an account: rows `{ scope: Key | Project | Workspace | Team | VirtualKey | Model, id (opaque), label (redacted), metrics[] }` produced by the adapter when the account opts in, capped, cached and never carrying personal data.
- Per-row metrics are ordinary `Metric`s (spend, usage, quota with limit and reset when the provider has one), so a row with a cap behaves like a quota (alerts and pace through PRD 0041 budgets or the provider's own limit).
- Delivered for `open_router` (keys), `litellm` (virtual keys and teams), `open_ai_api` (projects, api keys), `anthropic_api` (workspaces), `copilot` org (none: seat-level data is per user, excluded), `cursor` Teams (none for the same reason), with the pattern documented for later providers.
- Opt-in per account (`breakdown = ["keys"]`), with the extra request count stated in the dialog and refused for metered providers.

## 4. Non-goals

- Per-user rows (names, e-mails, user ids): excluded by design even when the API offers them; a `Team` or `Project` is the smallest unit.
- Editing limits, keys or budgets on the provider (read-only).
- Unbounded history of breakdown rows: the history (PRD 0006) records the top rows only (RF5).
- Aggregator connections (PRD 0036) are not re-implemented; they are one instance of the same visual pattern and keep their own PRD.

## 5. Functional requirements

### RF1 — Model (high priority; prerequisite)

- **RF1.1** `Scope` gains `Key`, `VirtualKey`, `Project`, `Workspace`, `Team` (serialized in snake_case; `Model` exists). `Data` (the adapter output) gains `breakdown: Vec<BreakdownRow>`; `Report` and `AccountExport` carry it; `BreakdownRow { scope, id: String (opaque, ≤ 64, provider id or a digest), label: String (≤ 48, redacted by RF1.3), metrics: Vec<Metric>, truncated: bool }`.
- **RF1.2** Caps: at most **25 rows** per account per refresh, ordered by the provider's primary spend/usage descending; the rest are summed into one row `label = "Others (N)"`, `id = "others"`. A row's metrics are at most 6.
- **RF1.3** Redaction: labels go through the export's scrubber (e-mails, URLs, tokens removed; a label that is only an e-mail becomes "Key ab12…"); a label matching a key prefix pattern (`sk-…`, `or-…`) is replaced by its last 4 characters; the account TOML never stores labels.
- **RF1.4** Account field `breakdown = [ "keys" | "projects" | "workspaces" | "teams" ]` (array; the allowed values come from the provider's descriptor; `Config::validate` rejects others); default empty (no extra request).
- **Acceptance criterion:** serialization round trips, the cap with "Others", the redaction table, validation of the array per provider, `Data` without breakdown unchanged for every existing adapter test.

### RF2 — OpenRouter keys (high priority)

- With a **management key** (PRD 0038), `breakdown = ["keys"]` lists the organization's keys (paged, at most 3 pages) and emits per key: `Spend` (usage, USD), `Quota` when the key has a `limit` (`used`, `limit`, `remaining = limit_remaining`, `window` from the key's reset interval when present: daily/weekly/monthly, `resets_at` computed from it), `Usage` daily/weekly/monthly when reported. Disabled keys are rows with the note `note.breakdown.disabled`. The inference-key mode has no breakdown (its own key is the account).
- **Acceptance criterion:** `wiremock` fixtures (3 keys, 40 keys → cap, a key with limit and reset, disabled key), redaction of key names, no key hash or secret in any output.

### RF3 — LiteLLM virtual keys and teams (high priority)

- With the proxy admin credential the `litellm` account already uses, `breakdown = ["keys"]` reads the key list with spend and `max_budget`/`budget_reset_at` (one `Quota` with `limit`, `used`, `resets_at` when present, else `Spend`), and `["teams"]` reads teams with spend and budget. Rows are labelled by the key alias or team alias (redacted), never by `user_id`. Soft budgets become the PRD 0041 floor-like warning only if the proxy reports them (otherwise ignored).
- **Acceptance criterion:** fixtures with aliases, missing aliases (labelled by the last 4 of the key id), budgets with and without reset, paging, and the user-id field never appearing.

### RF4 — OpenAI projects and keys, Anthropic workspaces (medium priority)

- `open_ai_api` with `breakdown = ["projects"]` (or `["keys"]`): the costs endpoint with `group_by=project_id` (or `api_key_id`) for the UTC month; project names from the projects list when the admin key may read it, otherwise the id; `anthropic_api` with `["workspaces"]`: cost report grouped by `workspace_id`, names from the workspaces list. Both are extra requests per refresh (stated: "+2 requests"), unpaged beyond 3 pages, never on metered providers.
- **Acceptance criterion:** fixtures (names available, names unavailable, more than 25 projects), the request count asserted, the UTC-month note reused.

### RF5 — History, alerts and budgets on rows (medium priority)

- **RF5.1** The history records the top **10** rows per account (series key `breakdown:<scope>:<id>`), hourly after 7 days like other series; rows that disappear age out.
- **RF5.2** A row metric with `limit` and `resets_at` is eligible for threshold alerts and pace **only when the person turns on "Alert on breakdown rows"** for the account (off by default, like PRD 0036's "Alert on each connection"); a PRD 0041 budget may target a row by `scope` and `id`.
- **RF5.3** Row-level events (`quota_threshold_crossed` etc.) carry `scope` and the redacted label, so hooks and channels can route them.
- **Acceptance criterion:** history recording with the cap, the alert gate, a budget on a row, event payloads.

### RF6 — Surfaces (medium priority)

- **RF6.1** Desktop details: a **Breakdown** section (DS-CMP-SUB rows: label, meter when a limit exists, spend otherwise, reset chip), sortable by spend or percentage, with the "Others" row and the truncated note; the card shows "Top: Proj-A $412 · Proj-B $298" as one line when the breakdown is on (display preset Detailed only, PRD 0025 RF9).
- **RF6.2** TUI details list the rows; `--once` prints up to 5 rows under the account; `--json` keeps its shape and `--json-v1` carries `breakdown`.
- **RF6.3** Account dialog and wizard: a **Breakdown** multi-select from the descriptor with the request count and the metered refusal explained.
- **Acceptance criterion:** fixture renders (0, 5, 26 rows), sorting, the `--once` cap, the dialog with a metered provider (control disabled with the reason).

### RF7 — Access descriptors and documentation (high priority; prerequisite for RF2–RF4)

- Each breakdown is a **mode** in `access::classify` (`open_router` / `breakdown = keys` → `Official`, management key, `Organization`, with evidence and `verified_on`; same for `litellm`, `open_ai_api`, `anthropic_api`); the matrix is regenerated; `docs/providers.md` gains a "Breakdowns" column; `cuotai.example.toml` shows `breakdown = ["keys"]` on the OpenRouter account.

### RF8 — Export and server (low priority)

- `export::v1` `AccountExport.breakdown[]` (additive), `/v1/accounts/{id}` includes it, `/v1/limits` excludes it (summary stays small); `docs/export-v1.md` and the schema updated.

## 6. Non-functional requirements

- **Privacy:** no user-level rows; labels redacted; ids opaque; the TOML holds no labels; history holds digests of row ids only (`id` hashed when it is not already opaque).
- **Cost control:** opt-in per account; the extra requests are counted and shown; metered providers refuse the option; paging capped at 3 pages; one breakdown fetch per refresh, cached for the account's interval.
- **Resilience:** a failing breakdown request never fails the account: the main report stands and the breakdown carries the note `note.breakdown.unavailable` with the error class.
- **Honesty:** absent values stay `None`; "Others" is labelled with its count; truncation is stated.

## 7. Data and contract changes

- **Core:** `Scope` variants; `Data.breakdown`, `BreakdownRow`; account field `breakdown[]`; descriptor modes.
- **`export::v1` (additive):** `AccountExport.breakdown[]`; events gain `scope` and `scope_label` on row-level events.
- **History:** series `breakdown:<scope>:<digest>`.
- **TOML:** `breakdown = [...]` (additive; add to the PRD 0029 RF0 tolerance list one release earlier).

## 8. Surfaces

| Surface | Change |
|---|---|
| `cuotai-core` | model, four adapters, descriptors, redaction, history, alert gate, export |
| Desktop | details section, card line, dialog multi-select |
| TUI/CLI | details rows, `--once`, wizard |
| Docs | providers (column and modes), example TOML, export-v1, events |

## 9. Dependencies and sequencing

RF1 and RF7 first; RF2 and RF3 (the two providers whose keys carry real caps) next; RF5 and RF6; RF4 and RF8 last. After PRD 0038 (management key) and PRD 0036 (visual pattern). Lane: model + four adapters + details; coordinate with provider lanes touching the same adapters.

## 10. Risks

| Risk | Mitigation |
|---|---|
| A provider lists users under "keys" | Per-provider mapping reviewed; user-id fields never read into the row; tests assert absence |
| Request volume on big organizations | 25-row cap, 3 pages, one fetch per refresh, opt-in, counts shown |
| Row ids change (key rotation) | Rows are rebuilt each refresh; history series age out; no alert on disappearance |
| Label leaks a secret-like alias | Redaction rules of RF1.3 with tests on key-prefix patterns |
| Confusion with aggregator connections | Different component (sub-rows inside details, not cards), documented difference |

## 11. Order of execution and verification

1. **RF1**, **RF7**.
2. **RF2**, **RF3**, **RF6** (desktop and TUI minimal).
3. **RF5**, **RF4**, **RF8**.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests; a smoke with the owner's OpenRouter management key: the keys appear with their caps, a key at 92% of its monthly limit shows the meter and (with the gate on) alerts once, and no key hash appears in any output or file.

## 12. Open questions and decisions to take

| Question | Recommendation |
|---|---|
| Should `params.project_ids`/`workspace_ids` filters be replaced by breakdowns? | No: the filter stays for the account total; the breakdown is additional. |
| Row cap 25 and history cap 10 | Keep; configurable later if people ask. |
| Alerts on rows default | Off, like PRD 0036's connections; one switch per account. |
| Copilot org seats and Cursor Teams members | Excluded (per-user data). Document the reason in `providers.md`. |

### Owner decisions (2026-10-25)

- **TOML tolerance for `breakdown` ships in the current release: yes.** Accepted and ignored before this PRD ships, so the delivering version is downgrade-safe; delivered in 0.81.0 (the account table accepts and ignores the key).
- Scheduled for the next version, not this release.
