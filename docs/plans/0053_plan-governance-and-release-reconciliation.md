# PRD 0053 — Plan governance and release reconciliation

| Field | Value |
|---|---|
| ID | 0053 |
| Status | Proposed (manual reconciliation and some supply-chain items done; the gates are open) |
| Owner | repository automation (`xtask`, GitHub Actions, release scripts), `cuotai-core::changelog`, desktop and TUI release-note presenters |
| Consumers | maintainers, agents, release approvers, package users, every PRD author and every release channel |
| Depends on | PRD 0039 RF1–RF9/RF11 (commit, plan and release automation), PRD 0021 RF1–RF6/RF9 (release pipeline and maintenance), PRD 0019 RF1/RF5–RF7 (licensing, support and changelog), PRD 0018 RF1/RF2 (updater manifest and notes) |
| Benefits from | PRD 0022 (desktop component and accessibility patterns) |
| Extends | PRD 0039 RF2/RF4–RF8/RF11; PRD 0021 RF2/RF6/RF9; PRD 0019 RF1/RF5/RF6/RF7; PRD 0018 RF1.4/RF2.2 |
| Date | 2026-10-27 |

> **Note on terminology:** “RF” (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **strong dependency** blocks delivery; a **weak dependency** improves it without blocking it. **Reconciliation** means bringing repository documentation and plan status into agreement with an already-published release. Code references are to `origin/main` at `7a1d93c3`.

## Implementation status (2026-10-28)

Written on 2026-10-27 as PRD 0043 by the 2026-10-25 PRD audit against `origin/main` (0.80.0) and renumbered 0053 when it moved onto `integration/next`, whose 0040–0049 are other plans. This table was re-evaluated on 2026-10-28 against `integration/next` at `18009a04`; the §1 code references still point at `origin/main` at `7a1d93c3`, and the current location is named here where it moved.

| RF | Version | Delivery |
|---|---|---|
| RF1 | — | Proposed. No `docs/plans/TEMPLATE.md`. |
| RF2 | — | Proposed. No `plans-check`; `changelog-check` still validates only strong graph edges (RF2.3 partial, already on `origin/main`), and `changelog-gate` still skips reference resolution when plans cannot be read. |
| RF3 | — | Proposed. §3 is still generated from every `Refs:` trailer; the 0.80.0 row still credits 0018 RF1.1/RF3.1 and 0039 RF5/RF7. |
| RF4 | — | Proposed. RF4.2/RF4.3 partial by hand: `40da9f4e` and `33f37d3c` made `README.md`, `docs/installing.md` and the plan statuses match the published 0.80.0; no typed `ReleaseState` or `release reconcile`. |
| RF5 | — | Proposed. RF5.3 partial: `2d16fd09` adds two `release` approval gates; no distinct-reviewer requirement. `docs/releasing.md` still says a red `CI result` cannot merge, and PRD 0039 Appendix A still says "Do not tag". |
| RF6 | — | Proposed. RF6.2 partial: `2d16fd09` adds the weekly `advisories.yml`, which does not open an issue. RF6.7 partial: the release refusal of the dev store and the `.gitignore` entries exist, without a release-profile regression test. No Dependabot, MSRV job, secret scanning, bundled third-party licences or OS/EOL policy. |
| RF7 | — | Proposed. RF7.4 partial: the desktop What's new (`b2d8cbdd`, `0a931fa8`) renders the core's structured sections, but from raw entry text with references and hashes; `latest.json` still copies raw Markdown. |

## 1. Context

The first stable release exposed gaps between the repository’s declared process and its observable state:

- `changelog-check` reads plans and checks only strong dependency edges (`xtask/src/plan_links.rs:20-30`, `:405-477`). `Plans::missing` accepts an RF mentioned anywhere in a plan (`:77-99`), while `changelog-gate` silently disables reference resolution if plans cannot be read (`xtask/src/changelog_gate.rs:63-70`, `:180-188`).
- `release prepare` generates control §3 from every `Refs:` trailer (`xtask/src/release/prepare.rs:281-291`; `xtask/src/release/render.rs:356-383`). Its own test therefore treats all cited RFs as delivered (`render.rs:628-650`), even when a plan records delivery in another version or still calls the RF partial.
- Release preparation replaces `vNEXT` and updates the control header before publication (`xtask/src/release/prepare.rs:281-292`); tagging checks the selected release commit but has no post-publication phase (`xtask/src/release/tag.rs:19-98`). The workflow publishes and then verifies public assets (`.github/workflows/release.yml:1125-1217`) without reconciling docs.
- User documentation still says no release exists (`README.md:47-58`; `docs/installing.md:5-21`). The release pipeline already exposes verified `manifest.json`, `signing-state.json` and repository channel variables after publication (`release.yml:1219-1263`), which are better sources than hand-written status.
- CI runs dependency advisories only when a change is classified as code (`.github/workflows/ci.yml:313-330`), declares Rust 1.89 while testing 1.99.1 (`Cargo.toml:9-12`; `ci.yml:183-190`), and its privacy guard scans personal mailbox addresses, not secrets (`xtask/tests/privacy.rs:1-25`, `:61-78`).
- CLI archives contain only the binary and two project licences (`scripts/release/package-cli.sh:14-30`), and the smoke enforces that exact layout (`scripts/release/smoke-cli.sh:56-64`). The desktop bundle has no licence resources (`apps/desktop/src-tauri/tauri.conf.json:44-53`).
- The changelog parser preserves raw Markdown bodies (`crates/cuotai-core/src/changelog.rs:82-94`); `latest.json` copies that string (`xtask/src/updater.rs:188-193`), and the desktop escapes it as undifferentiated text (`apps/desktop/ui/js/updater.js:119-129`). PRD 0018 RF1.4 instead requires plain text, and PRD 0019 RF7.7 still lacks its desktop surface.
- The main ruleset documentation says the shared admin identity has pull-request bypass (`docs/releasing.md:746-754`) and then claims a red `CI result` cannot merge (`:757-758`). The bypass allowing that merge is `[INFERENCE]`; it has not been exercised. PRD 0039 Appendix A also says “Do not tag” after RF5.2 and decision D8 permit conversation-scoped agent tagging (`docs/plans/0039_release-flow-and-changelog-automation.md:170-177`, `:328-354`).
- The plaintext development credential store is intentionally debug-only and rejected before filesystem access in release builds (`crates/cuotai-core/src/oauth/store.rs:1-9`, `:22-39`), while both front ends expose explicit development entry points (`crates/cuotai-tui/src/main.rs:150-151`, `:498-501`; `apps/desktop/src-tauri/src/backend.rs:991-1003`). This boundary needs a release regression check, not only documentation.

## 2. Problem

Plan status, dependency diagrams, commit references and published-release documentation can disagree while all current gates pass. Release notes expose implementation references to users and omit intermediate versions. Maintenance automation notices policy drift but not new Rust advisories or dependency updates. The repository also lacks enforceable MSRV, secret-scanning, third-party-licence and post-release governance contracts.

## 3. Goals

- Make every machine-readable view of plans agree and fail closed.
- Reconcile a published release from live, verified evidence rather than memory.
- Keep branch, environment and agent privileges honest and independently approved.
- Add scheduled dependency, advisory, secret and licence controls.
- Render one user-facing release-note model across updater, GitHub, desktop and TUI.

## 4. Non-goals

- Replacing PRD 0039’s conventional commits, release PR or immutable tags.
- Publishing a channel that its repository variable leaves disabled.
- Changing updater signature verification or package-manager ownership detection.
- Treating generated licence inventories as legal advice.
- Storing live GitHub responses, credentials or release tokens in the repository.

## 5. Functional requirements

### RF1 — Canonical PRD template (high priority; prerequisite)

- **RF1.1** Add `docs/plans/TEMPLATE.md` with the exact PRD 0032-derived header fields and sections used here: terminology, dated implementation table, sections 1–13, and section 14 integration notes. Placeholder IDs are non-resolving examples, not a numbered plan.
- **RF1.2** The template requires `### RFn — Title (priority; qualifier)`, concrete `**RFn.m**` items and one final `**Acceptance criterion:**` per RF. It states that business rules belong in `cuotai-core`, frontends only present them, absent values never become zero, secrets never enter TOML/snapshots/exports, and errors expose no bodies, credential-bearing URLs or tokens.
- **RF1.3** The template explains `vNEXT`, decision-J fix rows, strong/weak dependencies, `Extends`, and section 14’s exact control-row payload. Examples keep `vNEXT` in code spans so release checks do not replace them.
- **RF1.4** `plans-check --base <ref>` applies the full template contract to every added numbered plan; all plans are checked for parseable shared fields. `TEMPLATE.md`, assets and `0000_control.md` are not counted as plans.
- **Acceptance criterion:** pure unit tests accept a filled template and reject each missing field, section, RF acceptance block and integration-notes item; a temp-repo xtask test rejects a newly added malformed plan.

### RF2 — Fail-closed `cargo xtask plans-check` (high priority; release gate)

- **RF2.1** Add the `plans-check [--base REF] [--release REV]` subcommand. CI runs it on every pull request and `main` push in the always-on `changelog-check` job; `release prepare`, `release prepare --check` and `release.yml`’s `check` job call `--release` in-process.
- **RF2.2** Parse `0000_control.md` §2 into one row per numbered plan. Header totals must equal row counts by normalized status; each plan header status and explicit implementation rows must agree with §2 status and delivered RF set. Partial delivery is represented by explicit RF/sub-RF rows, never inferred from prose.
- **RF2.3** Parse §4 Mermaid. Every §2 strong dependency is exactly one solid edge, every weak dependency exactly one dashed edge, with no extra edges; node class maps exactly to `Proposed`, `In progress`, `Awaiting owner verification`, `Implemented`, `Superseded` or `Cancelled`. “What each plan unblocks” covers both dependency kinds in distinct columns.
- **RF2.4** Resolve every reachable commit trailer `Refs: PRD NNNN RFk`: `RFk` requires an exact `### RFk —` heading; `RFk.m` requires an exact bold item under that heading; decision-J IDs require their declared fix row. Any unreadable/missing plan is an error, replacing `Plans::read(repo).ok()`.
- **RF2.5** `Extended by`/`Extends` header relationships must equal §4 “Extensions between plans” in both directions. Every §2.1 owner-checklist item and every dependency qualifier names an RF definition that RF2.4 would accept.
- **RF2.6** In release mode, no replaceable `vNEXT` remains outside template examples, and all numbered plans pass an English-language classifier over Markdown prose (code, paths, identifiers and proper-name allow-list excluded; files below 200 alphabetic characters are manually classified).
- **RF2.7** Errors are aggregated and identify file, line, plan, view and expected value. No missing section, unreadable file, unknown status or parser fallback is silently skipped.
- **Acceptance criterion:** xtask fixture tests cover wrong header counts, status/delivery drift, each edge/class mismatch, mention-only and unreadable-plan references, extension drift, bad §2.1 IDs, release `vNEXT`, and a Portuguese prose fixture; an xtask test passes the real plan tree.

### RF3 — Honest “Delivered by version” rendering (high priority; correctness fix)

- **RF3.1** Replace `release::render::control_rows(version, commits)` with a renderer that also receives parsed plan implementation state. A row lists only RFs whose own implementation table changes from `—`/`Proposed` to `vNEXT` for this release; `Refs:` supplies traceability hashes but never delivery by itself.
- **RF3.2** A hardening, documentation or fix commit that cites an already-delivered RF may contribute its hash only when the plan explicitly marks a new sub-requirement delivered. A cited partial/pending/owner action is omitted. An RF marked delivered without a matching delivering commit fails instead of producing a hashless row.
- **RF3.3** Ordering is plan number, then numeric RF/sub-RF order; duplicate references and hashes collapse deterministically. Refreshing a release reproduces the same rows from the release-base plan state and current release plan state.
- **Acceptance criterion:** a pure renderer regression fixture for 0.80.0 excludes PRD 0039 RF5/RF7 and PRD 0018 RF1.1/RF3.1 while retaining only RFs their plans mark newly delivered; separate tests cover sub-RFs, hardening references, duplicates and refresh.

### RF4 — Post-release reconciliation (high priority; stable releases)

- **RF4.1** Add `cargo xtask release reconcile --tag vX.Y.Z [--check|--write]`. It accepts only a stable, published Latest release, downloads public `manifest.json` and `signing-state.json`, reads `PUBLISH_*` repository variables through an injected GitHub client, and rejects mismatched tag/version, missing assets or unverifiable signing data. Tests inject fixtures; no test contacts GitHub.
- **RF4.2** Generate the README and `docs/installing.md` channel-status tables from one typed `ReleaseState`: published artifacts, direct-download availability, per-platform signed/notarized state, enabled channel, and separately observed channel publication. “Enabled” never means “published”; unknown stays unknown, never false or zero.
- **RF4.3** Reconcile `0000_control.md` §2.1 with date and version, append one idempotent §8 history row, and update the status/evidence text of PRDs 0018, 0021 and 0039 only where live evidence satisfies their named acceptance. A same-repository rehearsal cannot satisfy a required fork rehearsal.
- **RF4.4** After `verify-public`, stable `release.yml` runs `reconcile --write`, opens or refreshes `docs/reconcile-vX.Y.Z` through a dedicated GitHub App, and reports the PR URL. A scheduled/read-only `reconcile --check` fails while `main` says “not published” or “no release” and GitHub reports a Latest release.
- **RF4.5** Reconciliation never changes a tag or released asset, never pushes to `main`, and logs only asset names and public signing classifications. RC publication records evidence but does not advertise stable channels.
- **Acceptance criterion:** an xtask test with v0.80.0 fixtures generates stable direct-download rows, signed/notarized macOS and unsigned Windows without claiming channel publication; idempotence and stale-“not published” red cases are tested, plus a secretless workflow-dispatch smoke that opens a draft reconciliation PR in a fixture fork.

### RF5 — Honest rulesets and independent release approval (high priority; owner security decision)

- **RF5.1** `docs/releasing.md` states the effective admin `pull_request` bypass honestly: `[INFERENCE]` the owner/agent admin token can merge a red `CI result`; post-merge CI detects but does not prevent that merge. The owner chooses D1: remove that bypass, or give agents a separate non-bypass GitHub App/ruleset identity.
- **RF5.2** `workflow-check` validates the chosen checked-in policy and a sanitized exported-ruleset fixture. A deliberate red sacrificial PR records the owner’s observed GitHub behavior; untested semantics remain labelled `[INFERENCE]`.
- **RF5.3** The `release` environment reviewer is a human account different from the principal used by agents or the reconciliation App; self-review is disabled. Agent tagging remains allowed only after the conversation-scoped owner request and checks in AGENTS.md, but publishing still requires that independent approval.
- **RF5.4** Amend PRD 0039 Appendix A, RF5.5 and D8 so they all describe the same agent-tag rule; remove the obsolete “Do not tag” sentence and the claim that the owner’s shared token is itself a human gate.
- **Acceptance criterion:** an xtask policy test rejects a ruleset fixture with the forbidden bypass under the selected D1 option and rejects identical releaser/reviewer identities; an owner smoke records red-PR merge behavior and a release-environment approval by the distinct human reviewer.

### RF6 — Supply-chain and maintenance baseline (high priority; extends PRD 0021 RF9)

- **RF6.1** Add `.github/dependabot.yml`: weekly Cargo updates for the root and desktop manifests and weekly GitHub Actions updates, grouped by compatible ecosystem/security scope, with one open-PR limit per group and no automatic merge.
- **RF6.2** Add a weekly and manual advisory workflow that runs `cargo deny check advisories` for both manifests regardless of file changes. It opens or refreshes one issue on findings; network/tool failure is “could not check”, never clean.
- **RF6.3** Default D2 is to keep `rust-version = "1.89"` and add a Linux CI job running `cargo +1.89 check --workspace --all-targets --locked` plus the desktop manifest. If the owner chooses removal instead, remove the field and document “pinned toolchain only”; declaring an untested floor is forbidden.
- **RF6.4** Add secret scanning on every PR, `main`, weekly schedule and release preflight with gitleaks (or equivalent) pinned by full action hash. Scan commit range on PRs and full reachable history on schedule/release; redact matches. `.gitleaks.toml` may allow only path+rule-specific RFC 2606 fixtures, never broad token patterns.
- **RF6.5** Generate `THIRD-PARTY-LICENSES` with a Cargo-installed, version-pinned tool from both lockfiles. Ship it and `THIRD_PARTY_NOTICES` in every CLI archive and as Tauri bundle resources; CLI and desktop artifact smokes fail when either is absent, symlinked, empty or outside the package.
- **RF6.6** `docs/installing.md` states tested minimum macOS, Windows/WebView2 and Linux/glibc versions, how floors are exercised, and support/EOL for stable, RC and older releases. Unknown floors are not guessed; the owner selects and CI proves each advertised floor before publication.
- **RF6.7** Preserve the debug credential-store boundary: release tests prove `--dev-credentials-sqlite` and `CUOTAI_DEV_CREDENTIALS_SQLITE` cannot open SQLite, `.gitignore` keeps SQLite sidecars excluded, and release archives contain no database/vault-index file. Secret-scan allow-lists do not exempt this path.
- **Acceptance criterion:** workflow-check fixtures prove schedules and hash pins; a planted advisory and fake secret fail their workflows; MSRV CI follows D2; CLI and desktop artifact smokes inspect both licence files; release-mode CLI/desktop tests reject a temporary dev SQLite path without creating it.

### RF7 — One user-facing release-note renderer (medium priority; compatibility)

- **RF7.1** Add a dependency-free renderer beside `cuotai_core::changelog` that consumes parsed `Release`/`Section` values and returns structured categories plus plain text and Markdown views. It strips PRD/issue references, commit hashes and Markdown syntax from user text while preserving the reviewed sentence and breaking migration.
- **RF7.2** The release pipeline calls this renderer once: plain text (UTF-8, LF, at most 256 KiB) feeds `latest.json.notes`; Markdown feeds the release-body notes before signing/download tables. Raw `Release::body` is no longer a user-facing renderer.
- **RF7.3** Given installed version A and target B, the model includes every published version `A < version ≤ B`, newest first, so skipping versions loses no notes. The manifest carries all versions in the documented supported update window; overflow fails release preparation rather than truncating silently.
- **RF7.4** TUI About/`--changelog` and desktop About → What’s new consume the same structured model offline. The desktop opens once after update, honours its setting, uses semantic headings/lists rather than escaped raw Markdown, and localizes chrome/category labels in all built-in locales while entries remain English.
- **RF7.5** Empty notes, invalid bounds, malformed references and unsupported old versions return typed fixed-string errors; no changelog URL, body fragment, token or response body enters an error.
- **Acceptance criterion:** pure golden tests prove refs/hashes/Markdown are absent from plain text and that A→B includes two skipped releases; release-format tests prove `latest.json` and GitHub notes share entries; a `TestBackend` TUI render and browser-free desktop UI script show identical versions/categories in all built-in locales.

## 6. Non-functional requirements

- **Fail closed:** missing plans, live evidence, licence output, advisory results or reviewer separation blocks the owning gate.
- **Determinism:** repository-only inputs render byte-identically; live reconciliation first normalizes and stores no response body.
- **Least privilege:** PR checks are secretless; publishing and reconciliation credentials are separate and step-owned.
- **Privacy:** secret findings are redacted; no fixture allow-list contains a real host, mailbox or credential.
- **Maintainability:** one parser per plan/release-note contract; no regex-only second interpretation in a frontend.
- **Portability:** xtask logic is Rust; workflows use pinned actions and both Cargo lockfiles.

## 7. Data and contract changes

| Surface | Change | Compatibility |
|---|---|---|
| `docs/plans/TEMPLATE.md` | canonical PRD structure and integration payload | New authoring contract |
| `cargo xtask plans-check` | plan/control/commit consistency CLI | Additive; CI becomes stricter |
| `cargo xtask release reconcile` | typed live-release reconciliation | Additive; stable release step |
| `0000_control.md` §3 | delivery comes from plan status, not all refs | Corrective regeneration |
| `latest.json.notes` | shared plain-text, multi-version notes | Same JSON field; content corrected |
| changelog API | structured user-facing note views | Additive; raw body remains parser data only |
| release artifacts | `THIRD-PARTY-LICENSES`, `THIRD_PARTY_NOTICES` | Additive files |
| workflows/config | Dependabot, advisories, secrets, MSRV, reviewer policy | New gates and schedules |
| TOML/export/snapshots | No product data-contract change | — |

## 8. Surfaces

| Layer | Owns |
|---|---|
| `cuotai-core` | parsed changelog and dependency-free user-facing release-note model/renderer |
| Desktop | accessible What’s new presentation and multi-version update notes only |
| TUI/CLI | About and `--changelog` presentation of the shared model only |
| `xtask` / workflows | template/plan checks, delivery rows, reconciliation, supply-chain gates and release rendering |
| Docs / owner settings | generated channel truth, support floors, ruleset choice and independent reviewer evidence |

## 9. Dependencies and sequencing

RF1 precedes RF2. RF2 and RF3 land before another release is prepared. RF5 precedes any agent-pushed release. RF6 is independent after its owner floor choices. RF7 precedes RF4 so reconciliation can compare the published note contract. RF4 is last and reconciles the first release evidence. Place the plan in wave 5a before feature waves.

## 10. Risks

| Risk | Mitigation |
|---|---|
| Existing plans are too irregular for strict parsing | one migration in the RF1/RF2 change; no permanent legacy mode |
| Live channel state changes during reconciliation | distinguish enabled, observed and unknown; idempotent rerun |
| Admin bypass semantics differ from documentation | sacrificial PR evidence; retain `[INFERENCE]` until observed |
| Secret scanner flags synthetic fixtures | narrow path+rule allow-list using RFC 2606 only |
| Licence inventory differs by workspace | generate from both lockfiles and smoke final artifacts |
| Multi-version notes grow indefinitely | bounded supported window and hard 256 KiB release failure |

## 11. Order of execution and verification

1. RF1, then RF2 with the one-time plan migration.
2. RF3 and its v0.80.0 regression fixture.
3. RF5 owner settings and checked-in policy truth.
4. RF6 maintenance, secret, MSRV and licence lanes.
5. RF7 shared renderer and both frontend consumers.
6. RF4 reconciliation against a fixture fork, then the live stable-release flow.

At each step run repository and desktop fmt, Clippy and tests; `workflow-check` and `actionlint` for workflows; `node --check` and browser-free scripts for desktop JavaScript; artifact smokes for packaging; and `plans-check` itself. A release is reconciled only when the live check, generated docs, plan statuses and user-facing notes agree.

## 12. Open questions

| Question | Recommendation |
|---|---|
| Should reconciliation commit directly to `main`? | No; a dedicated App opens/refreshes a PR, and normal CI remains mandatory. |
| How long should old stable releases receive support? | Latest stable plus the previous stable for 30 days; RCs until superseded. Owner confirms the public wording. |
| Should release notes be plain text or structured JSON in `latest.json`? | Keep the updater-compatible plain-text field; derive it from the same structured in-process model. |
| Should advisory issues include dependency names? | Yes; public crate/advisory IDs are safe, but scanner output never includes matched secret text. |

## 13. Decisions needed from the owner

| Decision | Options | Recommendation |
|---|---|---|
| D1 — `main` admin bypass | Remove it; or move agents to a non-bypass App/ruleset identity | Remove status-check bypass; retain no route that can merge red CI. |
| D2 — MSRV | Test declared 1.89; or remove `rust-version` and promise only 1.99.1 | Test 1.89 in CI; if dependencies make it false, remove the declaration immediately. |
| D3 — independent release approver | Separate human reviewer; or no agent releases | Separate human reviewer with self-review disabled. |
| D4 — public OS floors | Owner-selected tested macOS/Windows/Linux floors | Publish only floors exercised by artifact smokes; do not infer support from build success. |
| D5 — support window | latest only; or latest + previous for 30 days | Latest + previous for 30 days; security fixes target latest immediately. |

## 14. Integration notes (for 0000_control.md)

**Proposed §2 row:**

| ID | Plan | Status | Delivered | Depends on (strong) | Depends on (weak) | Notes |
|---|---|---|---|---|---|---|
| 0053 | Plan governance and release reconciliation | Proposed | — | 0039 (RF1–RF9, RF11); 0021 (RF1–RF6, RF9); 0019 (RF1, RF5–RF7); 0018 (RF1, RF2) | 0022 | Fail-closed plan governance, honest release reconciliation, supply-chain cadence and one user-facing release-note contract. |

**Suggested wave:** 5a, after the existing release-flow foundation and before further feature waves or the next stable release.

**Existing PRDs extended:**

- **PRD 0039 RF2/RF4–RF8/RF11:** adds full plan consistency, delivery-aware control rows, post-publication reconciliation and honest privilege gates; supersedes RF11.2’s allowance for incomplete weak edges.
- **PRD 0021 RF2/RF6/RF9:** adds scheduled advisories/dependency updates, secret scanning, licence payloads and independent release approval; no package channel is enabled by this plan.
- **PRD 0019 RF1/RF5/RF6/RF7:** adds tested MSRV/OS/EOL policy, bundled third-party licences and the shared user-facing notes renderer; supersedes raw changelog bodies as release-note presentation.
- **PRD 0018 RF1.4/RF2.2:** makes `latest.json.notes` genuinely plain text, multi-version and presentation-ready; updater verification and consent remain unchanged.

**Decision-J fix requirements for existing delivered plans:**

- **PRD 0039 FIX-PROC-01:** correct RF7/docs ruleset claims and Appendix A’s “Do not tag” contradiction; record the owner’s D1 outcome and observed bypass behavior.
- **PRD 0039 FIX-PROC-02:** make `Refs:` resolution definitional and fail closed, and regenerate §3 from newly delivered plan rows rather than all references (the v0.80.0 regression cases in RF2/RF3).
- **PRD 0018 FIX-UPD-02:** correct RF1.4’s falsely delivered raw-Markdown notes and stale first-release/environment status text through RF4/RF7.
- **PRD 0019 FIX-REL-01:** correct RF7.7’s delivered claim while desktop What’s new is absent, then close it only with RF7’s shared-renderer smoke.
