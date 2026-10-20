# PRD 0005 — Usage access level per provider

| Field | Value |
|---|---|
| ID | 0005 |
| Status | Implemented |
| Owner | `cuotai-core`, TUI and desktop |
| Consumers | `crates/cuotai-tui` (`wizard.rs`, `dashboard.rs`), `apps/desktop` (`account-dialog.js`, cards) |
| Depends on | none (it is a prerequisite of PRDs 0016 and 0017) |
| Date | 2026-10-19 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable.

## Implementation status (2026-10-20)

| RF | Version | Delivery |
|---|---|---|
| RF1 | 0.33.0 | `cuotai_core::access::describe(&Account) -> UsageAccess` (pure, no network, no credential read) with `level` (`Official`/`Undocumented`/`Derived`/`Unavailable`), `credential`, `plan_requirement`, `scope`, `metered` (from `Provider::is_metered`), `evidence` (`Documentation` URL or `Reference` file) with `verified_on`, `summary`, `credential_hint`, `risk` and `reason` (`Some` exactly for `Unavailable`). Exhaustive `match` on `Provider` with no wildcard. Variants: personal Cursor (`query_mode = personal` or login) vs Teams, Copilot `quota` (or login) vs billing (`mode = org` widens the scope), Kimi `coding` (or login), Grok `cli` (or login), Gemini AI `cli` (or login), Perplexity `computer`, Qwen OAuth, Cline organization, Alibaba Coding/Token Plan `session_env`. Every `Official` URL was checked against the vendor page on 2026-10-20. Deviations from the RF1.4 draft, following the adapters and the docs: Cline is `Official` (its balance endpoint is in the public Enterprise API reference); Perplexity Sonar (no `product = computer`) and Qwen OAuth are `Unavailable`; Mistral, like Windsurf and Perplexity Computer, carries the `Enterprise` plan requirement (the Admin API is Enterprise-only). Table tests cover the 36 providers and the switching modes, the `Unavailable`-has-reason and `Official`-has-documentation invariants, and that no text or URL carries credentials. |
| RF2 | 0.33.0 | Ids `access.level.*`, `access.credential.*`, `access.credential_hint.*`, `access.scope.*`, `access.risk.*`, `access.plan.*`, `access.reason.<slug>` and `access.summary.<slug>` (one per provider/mode) in `crates/cuotai-core/locales/en-US.json` and `es.json`; Portuguese stays in the fallback. Evidence is a public documentation URL or the name of the 9router/community-client file. A test resolves every emitted text in pt, en-US and es and fails if a desktop `ui/locales` catalog defines an `access.*` id. |
| RF3 | 0.34.0 | `account_edit::draft_access(provider, auth_mode, &[DraftField]) -> UsageAccess` describes the account being edited with the same rules as `describe` (choice indices map to the descriptor's values; undeclared keys are ignored); `UsageAccess::notes()` (credential hint, risk, `access.requires_plan`, reason), `cost_advisory()` (`access.cost_advisory`, `Some` exactly when `metered`; `Provider::cost_advisory`, the `account_edit::cost_advisory` shim and the `cost_advisory.aws_bedrock` ids were removed) and `save_confirmation()` (`access.confirm_unavailable`, `Some` exactly for `Unavailable`). The TUI wizard labels every provider with its default-mode level, adds the notes under the credential help, shows the level/summary and warnings in the review, and saves an `Unavailable` account only on a second Enter (`[Gravar mesmo assim]`; any other key cancels). The desktop `list_providers` carries `level`, the new `draft_access` command feeds the notes under the credential help (refreshed on every field, mode and provider change) and the cost warning, and the first submit of an `Unavailable` draft shows the reason and turns Save into "Save anyway" (`dialog.saveAnyway`). Smoked in the TUI (tmux) and the browser demo in pt-BR, en-US and es with Official, Undocumented, Derived and Unavailable providers. |
| RF4 | 0.35.0 | `UsageAccess::explain(&Report) -> Option<Localized>` is the core's RF4.2 rule: `None` for a `Ready` report; an `Unavailable` account gets its reason whatever the failure (no query exists to blame); an `Undocumented` account whose report is `Error` with the `query.schema` note gets `access.interface_changed` ("may have changed without notice. Reference contract: {evidence}"); every other failure keeps its own note. Undocumented interfaces have no vendor documentation, so the pointer is the descriptor's evidence (the public 9router/community-client contract). The TUI card carries "level · summary" on its bottom border (muted, cut with "…" to the card) and, without metrics, the explanation wrapped over the free rows instead of the adapter's first note; the details add an `ACESSO` line (level · summary) and the explanation leading `QUOTAS E CONSUMO` in place of "Provedor sem métricas", with the adapter's notes still listed. The desktop `get_view` `ReportView.access` is the flattened descriptor plus `label` and `explanation`; the card shows a one-line muted "level · summary" under the summary and the explanation in place of the first note (also as the cause of a stale reading); the details (dialog and tray popover) show the line in full and the explanation as the cause, followed by every adapter note. RF4.3: `UsageAccess` (and `Level`, `Credential`, `Scope`, `Evidence` as `{ kind, url }`) serializes with snake_case slugs, the shape PRD 0012 RF1.1 embeds as `AccountExport.access` when it creates `export::v1` (not yet delivered); `Report`, `Snapshot` and the current `--json` are unchanged. Tests: the core rule with a real `Monitor` refresh of Poolside (`Unsupported`) and a `query.schema` failure on Claude, the serialized shape, TUI `TestBackend` cards/details with Poolside (`Unsupported`), Zed (`Error`, `query.schema`) and OpenAI (transport error, unexplained), and the desktop view. Smoked in the TUI (tmux, `CUOTAI_LANG` pt-BR/en-US/es with Poolside and Gemini AI key accounts), `--json` (no `access` key) and the browser demo with the core catalogs in pt-BR, en-US and es. |
| RF5 | 0.35.1 | `docs/providers.md` "Usage access matrix" carries the Level, Credential, Plan, Scope and Evidence columns for every provider and for each mode (an interface-switching `params` value from the `access` tests' `MODES` list, or the native login when `oauth::supports_login`) whose descriptor differs from the default; modes with identical cells share a row, and the providers billed per query are listed under the table. The test `access::tests::providers_doc_matrix_matches_the_descriptors` renders the table from `access::describe` (labels resolved in `en-US`, CRLF normalized) and compares it with the block between the `access-matrix` markers; on drift it fails with "Regenerate it with `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_matrix`", and that variable rewrites the block. Verified by flipping Groq to `Official`: the test failed until regenerated. The new-provider guides in `AGENTS.md`, `docs/architecture.md` (new "Adding a provider" section) and `CONTRIBUTING.md` list the access descriptor as a mandatory step. |

## 1. Context

The catalog has 36 providers (`Provider::ALL` in `crates/cuotai-core/src/model.rs`). Each adapter queries a different interface, and their reliability varies a lot: documented admin APIs (OpenAI, Anthropic, AWS Cost Explorer), own endpoints reachable with the account's key (Kimi, Kimchi, OpenCode Go, Ollama Cloud), internal interfaces inferred from community clients and 9router (ChatGPT/Codex, Claude, Kiro, Antigravity, Zed) and providers with no public query at all (Qwen, NVIDIA NIM, Poolside, Alibaba Model Studio, Cloudflare AI, Gemini AI with an API key).

Today this information exists only as free text: `docs/providers.md`, adapter notes and `Provider::cost_advisory()` (which covers only AWS Bedrock's per-query cost). The model catalog already solved a similar problem: `models::support(account)` returns, per account, a localized source (`models.source.live`, `models.source.undocumented`, `models.source.builtin`) or a reason for being unavailable, and the interfaces only display it.

The 2026-10-19 market analysis showed that no competitor describes the origin and reliability of each number in a structured way. Quotum explains the risk of reading unpublished quotas, but only as page text.

## 2. Problem

- Someone adding an account does not know whether the query is official, whether it depends on an internal interface that may break, whether it needs an admin key instead of the inference key, whether it needs an Enterprise plan or whether it bills per query.
- Picking the wrong credential class is the most common failure and produces generic errors (a real case: Ollama Cloud, "Response incompatible with the query interface").
- Providers without a public query show up as normal accounts and end in an error or an empty card, with no explanation.
- When a new provider is added, nothing forces the author to declare this context.

## 3. Goals

- Describe, in the core and per account, **how** usage is obtained: reliability level, credential class, plan requirement, scope, per-query cost and evidence.
- Show it in the provider picker, in the credential step and on the cards, with no per-provider rules in the interfaces.
- Require, by compilation and by test, that every new provider declares the descriptor.
- Generate the `docs/providers.md` matrix from the same source.

## 4. Non-goals

- Changing what each adapter queries, or blocking account creation for providers without a query (the warning is mandatory; blocking is not part of this PRD).
- Checking availability in real time (that is PRD 0011) or the age of the data (PRD 0010).
- Changing `Report`, `Snapshot` or the current `--json` output.
- Proving the vendor's documentation: the `Official` level requires a cited documentation URL, but the PRD does not guarantee the interface is stable.

## 5. Functional requirements

### RF1 — Access descriptor in the core (high priority; prerequisite)

- **RF1.1** Create the `access` module in `cuotai-core` with the pure function `describe(account: &Account) -> UsageAccess`, with no network. The result varies by provider, authentication type (credential or OAuth) and by `params.product`/`params.mode` when they change the interface used (personal Cursor vs Teams, Copilot `quota` vs `billing`, Kimi `default` vs `coding`, Grok `management` vs `cli`, Gemini AI `default` vs `cli`).
- **RF1.2** `UsageAccess` contains:
  - `level`: `Official` (an interface documented by the vendor for that credential), `Undocumented` (an internal or inferred interface), `Derived` (usage is inferred from another source, such as Cloud Monitoring metrics or rate-limit headers), `Unavailable` (there is no query; a reason is mandatory). PRDs 0016 and 0017 add `UserSession` and `Extension`.
  - `credential`: `InferenceKey`, `AdminKey`, `SubscriptionSession`, `CloudIdentity`, `LocalCli`, `UserSession`.
  - `plan_requirement`: an optional localized text ("Enterprise", "Teams or Enterprise").
  - `scope`: `Account`, `Organization`, `WholeCloud` (a bill for all products, like Alibaba Cloud) or `Product`.
  - `metered`: reuses `Provider::is_metered()`; it does not duplicate the list.
  - `evidence`: a public documentation URL **or** a reference to the 9router/community-client contract, plus the date (`verified_on`) of the last real verification.
  - `summary`, `credential_hint` and `risk` as `text::Localized` with a Portuguese fallback.
- **RF1.3** The function uses an exhaustive `match` on `Provider`, with no wildcard arm, so a new provider does not compile without a classification.
- **RF1.4** Initial classification, to be confirmed against each vendor's documentation before marking `Official`:
  - `Official`: OpenAI and Anthropic (admin key), AWS Bedrock (Cost Explorer, `metered`), Azure OpenAI, Alibaba Cloud (`WholeCloud`), Cursor Teams, Windsurf (Enterprise), Mistral (admin key), Perplexity Computer (Analytics), Kimi by key (balance), Grok `management`.
  - `Derived`: Gemini API and Vertex AI (Cloud Monitoring), Groq (`x-ratelimit-*` headers).
  - `Undocumented`: ChatGPT/Codex, Claude, Antigravity, Gemini CLI, Grok CLI, Kiro, CodeBuddy, Zed, personal Cursor, Copilot `quota`, Alibaba Coding Plan and Token Plan, Xiaomi MIMO, Qoder, MiniMax, OpenCode Go, Ollama Cloud, Kimchi, Cline.
  - `Unavailable`: Qwen (key), Alibaba Model Studio, NVIDIA NIM, Cloudflare AI, Gemini AI with an API key, Poolside (the known endpoint needs a platform token, not the inference key).
- **Acceptance criterion:** table tests cover the 36 providers and the modes that change the interface; `Unavailable` always carries a reason; `Official` always carries a URL; every emitted `id` exists in the `en-US` catalog (the `every_descriptor_id_is_translated_in_every_catalog` test already covers the pattern).

### RF2 — Texts and catalogs (high priority)

- **RF2.1** Create stable ids `access.level.*`, `access.credential.*`, `access.scope.*` and `access.summary.<slug>` (one per provider/mode) in `locales/en-US.json` and `es.json`; Portuguese stays in the `fallback`.
- **RF2.2** No text cites an authenticated URL, a token or a response body. The evidence is a public documentation URL or the name of the reference file.
- **Acceptance criterion:** a complete-catalog test in the three languages; no access string appears in the desktop `ui/locales` (the core owns them).

### RF3 — Display in the picker and the credential step (high priority)

- **RF3.1** The TUI (`wizard.rs`) and the desktop app (`account-dialog.js`) show, next to each provider, a short level label (`Official`, `Undocumented`, `Derived`, `Unavailable`), coming from `describe`.
- **RF3.2** In the credential step, show `credential_hint`: "Needs an admin key, not the inference key", "Internal interface, may change without notice", "Requires the Enterprise plan". It reuses the area where `credential_help.<slug>` already appears, without creating a second block for the same subject.
- **RF3.3** `metered` providers keep the current cost warning (`cost_advisory`), now derived from the same descriptor.
- **RF3.4** `Unavailable` providers remain selectable. Choosing one makes the interface show the reason before saving and ask for explicit confirmation ("Save anyway").
- **Acceptance criterion:** a TUI and desktop smoke test (three languages) showing a provider of each level; choosing `Unavailable` requires confirmation; no branching on `Provider` in the interfaces.

### RF4 — Display on the card and details (medium priority)

- **RF4.1** The card and the account details show the level and the summary, discreetly, without competing with the quota.
- **RF4.2** An `Unavailable` account shows the summary and the reason in place of "Error" or an empty card. An `Undocumented` account with `Status::Error` due to an incompatible schema shows that the interface may have changed and points to the provider's documentation.
- **RF4.3** The export JSON (PRD 0012) includes `access` per account; the current `--json` does not change.
- **Acceptance criterion:** a presentation test with an `Error` report from a schema mismatch on an `Undocumented` provider and an `Unsupported` one on an `Unavailable` provider.

### RF5 — Generated documentation and a drift test (medium priority)

- **RF5.1** `docs/providers.md` gets the Level, Credential, Plan, Scope and Evidence columns, generated by a test that compares the file with `access::describe` (the same pattern as `about.rs`'s drift tests). The test fails with instructions to regenerate.
- **RF5.2** The new-provider guide in `AGENTS.md` and `docs/architecture.md` starts listing the access descriptor as a mandatory step.
- **Acceptance criterion:** changing a level without updating the document breaks the test.

## 6. Non-functional requirements

- **Honesty:** `Undocumented` is never presented as a guarantee; the text says "may change without notice". `Official` does not promise permission on the user's account.
- **No network and no secrets:** `describe` is pure and does not read credentials.
- **Compatibility:** the TOML, `Report`, `Snapshot` and the current `--json` do not change.

## 7. Order of execution and verification

1. **RF1:** the module, the initial classification and table tests.
2. **RF2:** ids and catalogs.
3. **RF3 and RF4:** TUI and desktop, one at a time, with a smoke test.
4. **RF5:** the generated matrix and the new-provider guide.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests, and a smoke test of the changed interface.

The requirement is only delivered when a new provider does not compile without declaring its access, the wizard shows the level and the credential hint in the three languages, an `Unavailable` account explains the reason instead of showing a generic error, and the `docs/providers.md` matrix is verified by a test.
