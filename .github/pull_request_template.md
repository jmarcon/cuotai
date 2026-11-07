## What and why

<!-- What changed, why, and the PRD requirement or issue it delivers (Refs: PRD 0000 RF0, or #12). -->

## Checks

- [ ] `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass (or `cargo xtask preflight --push`).
- [ ] Each `feat`, `fix` or `revert` commit that touches `crates/` or `apps/` has a `Changelog:` trailer (or `Changelog: skip <reason>`).
- [ ] No version bump and no `CHANGELOG.md` edit.

## Catalog changes

<!-- Only when a file under crates/cuotai-core/locales, crates/cuotai-tui/locales or apps/desktop/ui/locales changes. Delete otherwise. -->

- [ ] A new or changed English text is in every built-in catalog of its layer (the parity tests enforce it).
- [ ] Texts drafted for a language nobody on this pull request writes are listed in that catalog's `_meta.pending_review`.
- [ ] The review checklist of [`apps/desktop/ui/locales/README.md`](../apps/desktop/ui/locales/README.md#review-checklist) was followed: placeholders intact, glossary terms and register, no brand name translated, lengths within budget.
- [ ] Native reviewer, if any: <!-- GitHub handle and scope (critical or full), also recorded in `_meta.reviewed` -->
