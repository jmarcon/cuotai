# The accounts file across versions

Cuotai's accounts file (`cuotai.toml`) is shared by the terminal, the command line and the desktop app, which you may update at different times. This page explains how each version reads a file another version wrote, how to find a mistake in it, and what the editors keep (PRD 0051). Where the file is, and the order Cuotai looks for it in, is in [`advanced.md`](advanced.md#the-accounts-file); the reference of every entry and parameter is [`../cuotai.example.toml`](../cuotai.example.toml).

## Two readers

- **The running app reads tolerantly.** The dashboard, `--once`, `--json`, `--serve`, `--watch` and the desktop app keep working when the file has something this version does not know:
  - an `[[accounts]]` table whose `provider` or one of whose keys is unknown is **set aside**: the other accounts still refresh, and a notice names its position and the key (`Account 2 uses accounts[1].provider, which this version of Cuotai does not know`), in the TUI's status line, a desktop banner and on stderr;
  - an unknown top-level key or table (a later release's `[updates]`-style table, say) is ignored with a notice.

  Syntax errors, a key written twice and a value of the wrong type still reject the whole file: a broken file is never half-read.
- **Checking and editing are strict.** `cuotai --check-config` reads the same file with the strict reader, which rejects every unknown provider and key, so a typo (`floor` for `floors`) is found instead of silently setting an account aside.

A set-aside account is not queried: an unknown key may change its credentials, its endpoint or its cost, so Cuotai never guesses.

## Finding the problem: `cuotai --check-config`

```sh
cuotai --check-config            # one line per problem; exit 0 when valid, 1 otherwise
cuotai --check-config --json     # {"valid": false, "diagnostics": [{"kind", "line", "column", "path"}]}
```

It follows the normal file lookup (`--config`, `CUOTAI_CONFIG`, `./cuotai.toml`, the OS path), needs no credential and makes no network request. Each diagnostic names the line, the column and the key path (`accounts[2].provider`), never the value, so it can be pasted into an issue. `cuotai --doctor` runs the same check among its others.

## What the editors keep

The TUI wizard and the desktop app edit the file in place:

- comments, blank lines and the order of what you wrote stay as they are;
- an account set aside because it needs a newer Cuotai, and an unknown top-level table, are kept **byte for byte** where they are, while you add, edit, rename, move or delete the other accounts;
- a file changed on disk behind the editor's back is never overwritten: the editor asks you to reload.

So an older version used side by side with a newer one never deletes what the newer one wrote.

## Fixing a set-aside account

1. Run `cuotai --check-config`: it names the key.
2. If the key is a typo, fix it (the strict reader's path and line say where).
3. If the key or provider is genuinely newer, update the Cuotai that shows the notice; until then that account is skipped and nothing about it changes.

## Moving the file

The history of readings is filed under the file's location, so a moved or renamed file starts with an empty history. When the file a front end opened last no longer exists and the new file holds **exactly the same accounts** (by their stable `id`, the first key of each account once Cuotai has saved the file), the TUI and the desktop app offer to carry that history over. Nothing moves unless you accept; names are never compared, and the command-line modes never adopt on their own. Details: [`advanced.md`](advanced.md#moving-the-accounts-file).

## Going back to an older version

Install the older version **before** opening the files with it. Versions from 0.83.0 on read a file with keys of a later release tolerantly; 0.82.3 and older reject it, and 0.80.0 also rejects `[[groups]]` and an account `group`. The local files Cuotai keeps beside it have their own rules: see [`local-state.md`](local-state.md#downgrading).
