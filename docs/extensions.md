# Executable extensions

Cuotai extensions are local programs that return quota metrics as one JSON object. They are intended for services that are not built into Cuotai. An extension runs as the current operating-system user and is **not a sandbox**.

## Install layout

Create one directory per extension under the platform's local data directory:

```text
<local data>/cuotai/extensions/<id>/
├── manifest.toml
└── <program and its private files>
```

Cuotai only discovers these files; it never downloads, installs, updates, edits or removes them. The folder name is not the identity—the `id` in `manifest.toml` is. IDs must be unique.

## Manifest schema 1

A schema-1 `manifest.toml` accepts exactly these fields:

```toml
schema = 1
id = "team-quota"
name = "Team quota"
version = "1.0.0"
description = "Reads the internal quota service" # optional
homepage = "https://example.invalid/team-quota"   # optional, http(s), no credentials
command = ["./team-quota", "--json"]
env = ["TEAM_QUOTA_URL", "TEAM_QUOTA_TOKEN"]
interval_seconds = 300
timeout_seconds = 15 # optional; default 15
```

| Field | Contract |
|---|---|
| `schema` | Required integer. Only `1` is supported. |
| `id` | Required 1–64 character slug: lowercase ASCII letters, digits, `-` and `_`; starts with a letter or digit. |
| `name` | Required, non-empty single line, at most 64 characters. |
| `version` | Required, non-empty single line, at most 32 characters. |
| `command` | Required non-empty array. Element 0 is a relative program path inside the extension directory; absolute paths, `..`, missing files and symlink escapes are rejected, as are interpreter wrappers (`.cmd`, `.bat`, `.com`, `.ps1`, `.vbs`, any case). Arguments are passed exactly as separate `argv` elements—never through a shell. On Unix the program must be executable. NUL bytes are rejected. |
| `interval_seconds` | Required integer, at least 60. It is the per-extension normal refresh interval. |
| `timeout_seconds` | Optional integer from 1 through 60; default 15. The process tree is killed at the limit. |
| `description` | Optional text, at most 500 characters. Newlines are allowed; other control characters are not. |
| `homepage` | Optional `http` or `https` URL, at most 2,048 characters, with a host and without embedded username/password. |
| `env` | Optional array of at most 32 environment-variable names. Each matches `[A-Za-z_][A-Za-z0-9_]*` and is at most 128 bytes. Repeated names are deduplicated in first-seen order. |

A manifest larger than 64 KiB is rejected. Unknown manifest fields are rejected; an unsupported schema is rejected. A folder whose manifest cannot be read is still listed by both frontends with the reason (state `Invalid`) and can never be enabled; one bad folder does not prevent other extensions or built-in accounts from loading. Two valid folders sharing an `id` are both rejected, and the row says so. When a configured account's folder stops parsing, that account's own row becomes `Invalid` with the reason instead of appearing twice or as missing. One extension maps to exactly one account: two accounts pointing at the same `extension_id` are refused as a configuration error.

## Output schema 1

A successful process must exit with code 0 and write one UTF-8 JSON object to standard output:

```json
{
  "schema": 1,
  "status": "ready",
  "metrics": [
    {
      "label": "Monthly requests",
      "used": 1250,
      "limit": 5000,
      "remaining": 3750,
      "unit": "requests",
      "kind": "quota",
      "window": "monthly",
      "period": "October 2026",
      "resets_at": "2026-11-01T00:00:00Z"
    }
  ],
  "notes": ["Values may be delayed by five minutes"]
}
```

Top-level fields:

- `schema` is required and must be `1`.
- `status` is required and is one of `ready`, `needs_configuration`, `unsupported`, `authentication_error`, `rate_limited`, or `error`. Only `ready` yields metrics; every other value becomes a normal isolated error reading.
- `metrics` is required and contains at most 64 objects.
- `notes` is optional and contains at most 16 strings.

Metric fields:

| Field | Contract |
|---|---|
| `label` | Required text; markup/control content is removed and the result is limited to 80 characters. |
| `used`, `limit`, `remaining` | Optional finite JSON numbers. Missing or `null` means unknown, never zero. |
| `unit` | Required text of at most 16 characters, without control characters. An empty string is allowed for unitless values. |
| `kind` | Required: `quota`, `rate_limit`, `balance`, `credits`, `spend`, or `usage`. |
| `window` | Optional: `window_5h`, `daily`, `weekly`, `monthly`, or `other`. |
| `period` | Optional text; cleaned and limited to 64 characters. |
| `resets_at` | Optional RFC 3339 timestamp. |

Standard output is limited to 256 KiB. Notes are cleaned and limited to 500 characters. Invalid JSON, non-finite numbers, wrong field shapes and contract violations become an `Error` report; diagnostics name fields but never reproduce raw output. Standard error is always discarded.

Unknown **output** fields are ignored and may be added compatibly within schema 1. Existing field meaning and required fields remain stable within schema 1. A future incompatible contract uses a new schema; unsupported schemas fail closed.

## Shell example

`manifest.toml`:

```toml
schema = 1
id = "shell-quota"
name = "Shell quota example"
version = "1.0.0"
command = ["./quota.sh"]
interval_seconds = 300
timeout_seconds = 10
env = []
```

`quota.sh`:

```sh
#!/bin/sh
printf '%s\n' '{"schema":1,"status":"ready","metrics":[{"label":"Monthly requests","used":1250,"limit":5000,"remaining":3750,"unit":"requests","kind":"quota","window":"monthly","period":null,"resets_at":null}],"notes":[]}'
```

Make it executable on Unix with `chmod +x quota.sh`. Cuotai starts `quota.sh` directly; it does not concatenate or evaluate a shell command string.

## Python example

`manifest.toml`:

```toml
schema = 1
id = "python-quota"
name = "Python quota example"
version = "1.0.0"
command = ["./quota.py"]
interval_seconds = 300
timeout_seconds = 10
env = ["QUOTA_LIMIT", "QUOTA_USED"]
```

`quota.py`:

```python
#!/usr/bin/env python3
import json
import os

limit = float(os.environ["QUOTA_LIMIT"])
used = float(os.environ["QUOTA_USED"])
print(json.dumps({
    "schema": 1,
    "status": "ready",
    "metrics": [{
        "label": "Monthly quota",
        "used": used,
        "limit": limit,
        "remaining": max(0, limit - used),
        "unit": "requests",
        "kind": "quota",
        "window": "monthly",
        "period": None,
        "resets_at": None,
    }],
    "notes": [],
}))
```

Make it executable on Unix with `chmod +x quota.py`. Requested values are inherited only when their names are listed in `env` and the user approves them.

## Running on macOS, Linux and Windows

`command[0]` is always a **file inside the extension folder**: Cuotai never looks it up in `PATH`, never concatenates arguments into a command line, and never runs it through a shell or an interpreter. On Windows a batch or script wrapper would be handed to `cmd.exe` or PowerShell, which would re-parse the exact argument vector this contract fixes, so those file names are **rejected by validation** (case-insensitively: `.cmd`, `.bat`, `.com`, `.ps1`, `.vbs`).

**macOS and Linux.** The kernel handles the file's shebang line, so `#!/bin/sh` and `#!/usr/bin/env python3` work as written, provided the file is executable (`chmod +x`) and the interpreter exists at that path. `PATH` is still inherited (the environment is reduced to `PATH`, `HOME`, `LANG` and the approved requested names), so `#!/usr/bin/env python3` resolves `python3` through `PATH`.

**Windows.** There is no shebang and no `PATH` lookup: `command[0]` must be a file the OS executes directly. Use a packaged executable — `quota.exe` built with PyInstaller, Go, Rust, .NET or similar, placed in the folder with `command = ["./quota.exe"]`, or a program distributed as a self-contained executable for the interpreter it needs. It runs as one direct process, with no shell parsing in between.

The two examples above are Unix-only as written. On Windows, ship an executable.

## Enable, test, disable and remove

- **TUI:** press `x` on the dashboard, select an extension, then use `e` (enable), `t` (test), `d` (disable), or `x`/Delete (normal account removal). `e` on an extension account's card opens this screen instead of the account wizard, which has no form for a dynamic extension.
- **Desktop:** open **Extensions** from the main toolbar and use Enable, Test, Disable, or Remove account. Editing an extension account from a card opens this screen too.
- **CLI check:** `cuotai --extension-check <id>` displays the disclosure and asks for confirmation when approval is absent or stale; add `--json` for the normalized result after approval.

Enabling always shows the exact argument vector, canonical working directory, requested environment-variable **names**, interval, timeout, and residual risk. Only an explicit confirmation stores approval. Enabling creates or updates the matching `Extension` account through the normal account catalog flow; it then participates in regular refreshes and produces ordinary cards/reports with access label `Extension`.

Disabling revokes approval immediately. Disable stays available whenever an approval exists, including for a row whose files changed, vanished or stopped parsing — revoking is what stops execution. The account stays configured but cannot execute. Removing uses the same account-removal flow as built-in providers; neither action deletes the extension directory. Re-enable after reviewing the disclosure to approve the current files.

## Security boundaries

Cuotai hashes the manifest and executable at approval. A never-approved extension is disabled, and changing either file invalidates approval before the next process starts. Approval records contain digests and requested environment names, not environment values.

The approval is bound to exactly the files the disclosure showed: enabling hands back an opaque core-issued token, and the approval only lands if those same bytes are still on disk. If anything changed in the meantime the confirmation fails closed and must be repeated against a fresh disclosure. The same confirmation cannot be replayed. Approval also resets the extension's backoff and last-run state, so a re-approved version runs at once.

Disabling takes effect immediately and permanently for in-flight runs: a run that has not started its process when the revocation completes never starts, even if it was queued behind another extension.

Runs have closed standard input, the extension directory as working directory, low process priority, a timeout with process-tree termination, capped output, at most two concurrent extensions, and an isolated per-extension schedule/backoff. They do **not** take a provider query slot, so a slow extension can never delay a built-in account's refresh. The environment is cleared and rebuilt from `PATH`, `HOME`, `LANG` (`SystemRoot` on Windows) plus approved requested names. Cuotai never injects vault tokens or credentials from another account, never shows raw stdout/stderr, and never invokes a shell on behalf of the manifest.

One process-wide extension manager is shared by every monitor and dropped with the last one, so the two-process cap, the schedules and the revocation guard survive a configuration reload: a reloaded app never runs two extensions in parallel beyond the cap, and nothing is retained after the last user of the directory closes it.

These controls are **not a sandbox**. An approved program runs as you and can access files and the network allowed to your user. Review the program and every requested variable, use least-privilege credentials, and install extensions only from sources you trust. A failure or timeout is isolated to that extension and does not stop other accounts from refreshing.
