# Contributing to Cuotai

Thank you for wanting to help. Cuotai is a **read-only** monitor for AI-provider quotas, usage limits and costs. This guide covers how to build, test and propose changes. The detailed rules for people (and automated agents) working on the code are in [`AGENTS.md`](AGENTS.md).

## Before you start

- For small fixes, open a pull request directly.
- For anything larger (a new provider, a feature, a behavior change), open an issue first. The project roadmap is in [`docs/plans/`](docs/plans/), indexed by [`0000_control.md`](docs/plans/0000_control.md). If your idea already has a plan, cite the PRD number and requirements (for example, `PRD 0006 RF2`).
- Questions and bug reports go through the channels in [`SUPPORT.md`](SUPPORT.md).
- Do not report vulnerabilities in public issues: see [`SECURITY.md`](SECURITY.md).

## Setup

- Rust (2021 edition) and Cargo. We do not use a JavaScript package manager: dependencies come **only through Cargo**.
- Optional: AWS CLI v2 (`aws_bedrock` accounts) and the Bailian CLI (`bl`, Alibaba Token Plan login).

```sh
cargo build --workspace
cargo run -p cuotai-tui -- --list-providers
cargo run -p cuotai-tui                       # interactive TUI
sh run.sh tui                                 # the same TUI with the development credential store
sh run.sh gui                                 # desktop app in development mode (apps/desktop/src-tauri)
```

The desktop app is a separate workspace (`apps/desktop/src-tauri`). `run.sh` (`tui` or `gui`, `tui` by default; `apps/desktop/dev.sh` is a wrapper for `gui`) uses a SQLite file that stores **tokens in plain text**, for development only; release builds refuse that store. Never use it with credentials you cannot afford to lose.

## Checks before submitting

From the repository root (CI runs the same checks on Linux, macOS and Windows for every pull request and every push to `main`; see [`docs/architecture.md`](docs/architecture.md#continuous-integration)):

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For desktop changes, also:

```sh
cd apps/desktop/src-tauri
cargo clippy --all-targets -- -D warnings
cargo test
```

Dependency policy (licenses, advisories, sources) is checked with [`cargo deny`](https://github.com/EmbarkStudios/cargo-deny) and the root [`deny.toml`](deny.toml):

```sh
cargo deny --config deny.toml check
cargo deny --manifest-path apps/desktop/src-tauri/Cargo.toml --config deny.toml check
```

GitHub code scanning (the CodeQL workflow `.github/workflows/codeql.yml`) can be reproduced locally with `sh scripts/codeql-local.sh` (needs the [CodeQL CLI](https://github.com/github/codeql-action/releases); `--swift` is opt-in, macOS-only and needs full Xcode; exit 3 means findings).

User-visible changes also need a smoke test of the affected interface (the TUI with `--once`, `--json` or the screen concerned; the desktop app running, in English and at least one other language when you add text).

## Project principles

- **Read-only, no spend:** never send inference requests or any operation that burns credits to measure a limit.
- **Secrets stay out of everything:** `cuotai.toml` holds only environment-variable **names** or vault references. Credentials never go into TOML, snapshots, events, logs, error messages, tests or screenshots.
- **Honest errors:** classify missing configuration, authentication, rate limits, network failures and incompatible schemas. Never expose a response body, an authenticated URL or a token. A missing value is never turned into zero.
- **Business rules live in the core:** `cuotai-core` decides validation, forms, refresh scheduling, queries and persistence. The TUI and the desktop app only present. Structured core texts use `Localized`: write the English text as the `fallback` in the code, then add the id to every catalog in `crates/cuotai-core/locales/` (`pt-BR`, `es`, `it` and the ten other languages) with the same `{placeholder}` names; a draft for a language you cannot write is fine, and native reviewers refine it later (see [`apps/desktop/ui/locales/README.md`](apps/desktop/ui/locales/README.md)). The core tests fail on a missing id, a renamed placeholder or Portuguese in an English fallback.
- **Undocumented interfaces:** state the limitation in `docs/providers.md` and prefer failing clearly over guessing.
- **Desktop UI follows the design system:** changes to `apps/desktop/ui` use the components, tokens, chart types, wording and motion rules of [`docs/design-system.md`](docs/design-system.md) and pass its PR checklist.

## Tests and fixtures

- Tests live next to the code in `#[cfg(test)]` blocks. Provider contract tests use local `wiremock` servers.
- Fixtures must be deterministic and **anonymized**: keep the shape of a response and replace identifiers, e-mails, organizations and real values with placeholders. Never paste a real account response.
- Cover missing and partial values and error classification.

## Adding a provider

1. Add the variant to `Provider` (`crates/cuotai-core/src/model.rs`), including `label`, `slug` and `ALL`.
2. Add the adapter under `crates/cuotai-core/src/providers/` and the dispatch in `lib.rs`.
3. Declare the access descriptor (mandatory): the provider's arm in the exhaustive `match` of `access::classify` (`crates/cuotai-core/src/access.rs`), one per `params` value or login that changes the queried interface (a new interface-switching param also goes into the `MODES` list of the `access` tests), and its `access.summary.*`/`access.reason.*` texts in the core catalogs. Then regenerate the usage access matrix in [`docs/providers.md`](docs/providers.md#usage-access-matrix) with `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_matrix`; without the variable the same test fails while the matrix differs from `access::describe`. See "Adding a provider" in [`docs/architecture.md`](docs/architecture.md).
4. Add account fields in `account_edit.rs`, the core catalog texts and the credential help.
5. Add an example to `cuotai.example.toml` and a row to `docs/providers.md`. If you reuse third-party code, add the notice to `THIRD_PARTY_NOTICES` and to the list in `about.rs`.
6. Add `wiremock` tests and, where available, model-listing support in `models.rs`.

## Commits and versions

- Work on a branch (`<type>/<slug>`) and open a pull request to `main`; it merges (rebase and merge) once the `CI result` check is green.
- Semantic commits: `type(scope): subject`, with the subject and the body written in **English**.
- A user-visible commit carries its changelog entry as a trailer, `Changelog: <one sentence for the user>.` (or `Changelog: skip <reason>`), plus `Refs: PRD 0006 RF2` and `BREAKING CHANGE: <migration>` when they apply.
- Do **not** change any version or `CHANGELOG.md`: both are produced at release time and CI rejects them in a normal commit. The full rules are in "How to deliver a change" and "Versioning, Commits & Changelog" in [`AGENTS.md`](AGENTS.md). Check your commits locally with `cargo xtask changelog-gate --base origin/main --head HEAD`.
- Keep pull requests focused and small enough to review.

## Releasing

Releases are prepared by the owner (PRD 0039) and published by a tag through
`.github/workflows/release.yml` (PRD 0021):

```sh
cargo xtask changelog-draft                         # what the next release would contain
cargo xtask release prepare --dry-run               # next version, section and diffs
cargo xtask release prepare --branch                # release commit on release/vX.Y.Z
cargo xtask release tag --rc                        # after the merge; owner, or an agent on request
```

The full procedure, the dry run (**Actions → Release → Run workflow**, which
builds everything and publishes nothing), the signing secrets and the fork
rehearsal are documented in [`docs/releasing.md`](docs/releasing.md); the version, support and reproducibility policies in [`docs/release-policy.md`](docs/release-policy.md).

## Translations

Cuotai ships fourteen languages: English (the source), Portuguese (Brazil), Spanish, Italian, Chinese (Simplified), French, German, Japanese, Russian, Korean, Hindi, Indonesian, Turkish and Vietnamese. Most of them start as machine drafts and improve through reviews like yours.

**Fix a string.** Each language has three catalogs: `crates/cuotai-core/locales/<code>.json` (texts the backend emits), `apps/desktop/ui/locales/<code>.json` (desktop labels and dialogs) and `crates/cuotai-tui/locales/<code>.json` (terminal captions). Find the id by searching the current text, edit only the value, keep every `{placeholder}` exactly as it is, and run `cargo test --workspace`: the parity tests fail on a lost or renamed placeholder, markup, a wrong plural shape or a non-NFC text, and `length_budget` fails when a label goes over its class budget (`docs/i18n-length-budgets.md`). Read the review checklist and the glossary in [`apps/desktop/ui/locales/README.md`](apps/desktop/ui/locales/README.md) first. If you cannot run the tests, open a **Translation fix** issue instead.

**Propose a new language.** Open an issue titled "Language: xx" (the BCP 47 code) and name at least one native reviewer. A new language needs its registry row in `crates/cuotai-core/src/text/registry.rs`, the three catalogs, drawn flag artwork in `apps/desktop/ui/js/flags.js` and, for a script other than Latin or Cyrillic, a `--font-script` stack in `apps/desktop/ui/css/themes.css`; the steps are in "Adding a built-in language" of the locales README. A language without a reviewer for its critical strings (credentials, consent, deletion, errors) is not shipped.

**Credit.** Reviewers are credited by GitHub handle in the catalog's `_meta.reviewed` list and in the status table of the locales README. Sign your review in the pull request.

**Commits.** A translation fix is `fix(i18n): ...` with a `Changelog:` sentence (for example `Changelog: Japanese texts for removing an account are clearer.`); a new language is `feat(i18n): ...`. Code comments, documentation, commits and issues stay in English: the product is multilingual, the repository is not.

## Documentation language

The README and all documentation are written in **English**, as is usual for open-source repositories. The product itself stays multilingual: UI strings live in the English source and the catalogs of the thirteen other languages listed under "Translations".

## License of contributions

Cuotai is distributed under **MIT OR Apache-2.0** (see [`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE)). Unless you explicitly state otherwise, any contribution you submit is licensed under the same terms, without additional conditions. Only submit code you have the right to license; if you adapt third-party code, record its origin and license in `THIRD_PARTY_NOTICES`.
