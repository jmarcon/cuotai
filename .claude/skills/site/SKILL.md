---
name: site
description: Project site workflow for the GitHub Pages landing page in site/, served at jmarcon.github.io/cuotai. Use when creating, updating or redesigning the site, or when a change to its claims, screenshots or Pages pipeline is needed. Not for README.md, docs/ or the desktop UI in apps/desktop/ui.
---

# Project site

`site/index.html` and `site/styles.css`: static HTML and CSS, English only, no framework. The page is a summary of the docs, so it follows them and they stay as they are.

## Workflow

1. **Branch.** Open a worktree from a fresh `origin/main` on `docs/site-<slug>`, per "How to deliver a change" in `AGENTS.md`.
2. **Trace.** List every claim the change adds or alters and find each one in its source below. Done when every claim has a source line.
3. **Edit.** Apply the claims within the hard rules. Keep to `site/` and `docs/assets/screenshots/`.
4. **Verify.** Done when every item under "Verification" passes.
5. **Deliver.** Follow "How to deliver a change" with `docs(site): <subject>`: staging by explicit path, no version or `CHANGELOG.md` edit, a PR with a green `CI result`, Rebase and merge. A commit that leaves `crates/` and `apps/` alone needs no `Changelog:` trailer. Push, open the PR or merge only when the user asks.
6. **Confirm the deploy.** After the merge, watch the run with `gh run list --workflow pages.yml --limit 1` and `gh run watch <id>`, then open https://jmarcon.github.io/cuotai/ and confirm the change is live. A `main` push touching `site/**`, `docs/assets/screenshots/**`, `scripts/site-build.sh` or `.github/workflows/pages.yml` deploys; `gh workflow run pages.yml` deploys by hand. App releases play no part.

## Sources of truth

| Claim | Source |
|---|---|
| What the app does, pitch, audience | `README.md` |
| Install channels | `docs/installing.md`, the "Status today" column: **Live** rows only |
| Services, what each reads, limitations | `docs/providers.md` |
| Themes and their counts | `docs/themes.md` |
| Desktop features | `docs/gui-features.md` |

When two sources disagree, the specific document wins over `README.md`. Tell the user about the conflict and leave the docs alone in the site PR. Two known traps:

- `README.md` calls the package managers not live, and `docs/installing.md` marks several **Live**.
- `README.md` says every account is labelled Official, Undocumented, Derived or Unavailable. `docs/providers.md` records that the dashboard stopped showing those levels, so present them as the classification on that page, not as something on the cards.

## Hard rules

- **Versions:** the page carries none. Link to `https://github.com/jmarcon/cuotai/releases/latest`.
- **Data:** text and screenshots show made-up accounts on `example.com`, with no tokens and no personal e-mail.
- **Claims:** every claim and count traces to a source above. State Windows installers as unsigned and channels as not live for as long as the sources do.
- **Caveats:** keep visible that some interfaces are undocumented and may change, that a few services bill per query (AWS Cost Explorer), and that estimates are estimates.
- **Links:** relative for local pages and assets, because the site lives under `/cuotai/`. External links are plain `https://` anchors. No external scripts, no trackers.
- **Screenshots:** files live in `docs/assets/screenshots/`, shared with `README.md`, and the page references them as `assets/screenshots/<name>.png`.
- **Colors:** use the role names of `docs/design-system.md` section 3 (`--bg`, `--surface`, `--fg`, `--fg-muted`, `--accent`, `--good`, `--warn`, `--bad`). Define their values once, on `:root` in `site/styles.css`, for both modes, and reference only roles elsewhere. The contrast minimums of section 3.3 apply, and color never carries meaning alone. Meters show what is left, with the levels of DS-ID-5.

## Verification

- `sh scripts/site-build.sh` exits 0.
- `python3 -m http.server --directory _site 8000 --bind 127.0.0.1`, then at http://127.0.0.1:8000/ check each combination of 390 px and 1280 px with light and dark: no horizontal scroll, images load, nothing overlaps.
- Every image has alt text that says what it shows; decorative images use `alt=""`.
- Tab through the page: every link and control shows a visible focus ring, in reading order.
- `cargo xtask changelog-gate --base origin/main --head HEAD` and `cargo xtask changelog-check` pass.

## When something fails

- **`site-build.sh` fails:** fix the page and rerun until it exits 0. Keep the validation as it is; a rule that looks wrong goes to the user.
- **A claim has no source:** cut it, or ask the user. A doc change, if wanted, is its own PR and lands first.
- **The Pages run fails:** read `gh run view <id> --log-failed`. A build failure reproduces locally: fix it through a new PR. A deploy failure on a green build is usually the repository's Pages source setting, which must be GitHub Actions: report it to the owner, who changes that setting. Rerun with `gh run rerun <id> --failed` only for a transient error.
- **A screenshot would show real data:** discard it, including from any commit on the branch, and recapture from the browser demo: serve `apps/desktop/ui` statically and open `index.html?fixture=six`, whose accounts are all `example.com`. If real data reached `main` or a pushed branch, stop and tell the owner before anything else; a follow-up commit leaves it in history.
