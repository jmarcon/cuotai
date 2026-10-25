# Local contract canary

`cuotai --canary` is a maintainer tool (PRD 0028 RF4). It runs on your own machine with your own accounts and the tokens already in the OS vault, once, and tells you when a provider changed the interface Cuotai reads. CI never holds a provider secret, and the canary never files an issue or writes to the repository by itself.

## What it does

1. Runs one refresh of the configured accounts with the normal per-account deadlines. Accounts billed per query (AWS Cost Explorer) are left out unless you pass `--canary-include-metered`.
2. Writes a **shape record** per provider and connection mode to `<OS data dir>/cuotai/canary/run-<stamp>.json` (`0600`, the last 30 runs kept): the status, the failure category, the drifted field paths and their types, the response fingerprint (every key path of the answer with its JSON type, never a value) with its digest, and the metrics present by label, kind, window and unit. No value, account name, e-mail, organization, URL, header or body is ever written.
3. Compares it with the previous run of the same provider and mode: a field present before and gone from a good answer, a type that changed (a value turning `null` is an optional field, not drift), a metric new or absent, a new `schema_changed` or `auth_required` failure. With `--status` (or `CUOTAI_STATUS`), it also checks that every fixed component of a provider's public Statuspage still exists, and reports a renamed or removed one as `status_component` drift instead of the silent *unknown* the app would show. Undocumented and session interfaces are listed first. Two runs with nothing changed produce no alert.
4. Prints a value-free Markdown report (also saved as `report.md`) and one issue draft per provider and category, built by the same function as `--report-problem` plus the shape diff. `--canary-open` opens each GitHub link in the browser; otherwise the drafts are only printed.
5. Writes `verified_on.json`: the interfaces that answered with no drift today, as a list for you to review before raising their `verified_on` date. It is not applied automatically: review it, then run `cargo xtask access-verified --apply <canary dir>/verified_on.json` from a branch. It only raises dates (never to a day after today, never lowering one), edits `crates/cuotai-core/src/access.rs`, names the providers it cannot map, and tells you to regenerate the matrix with `CUOTAI_REGENERATE_DOCS=1 cargo test -p cuotai-core providers_doc_matrix`; commit the result like any change.

Exit code: `0` no drift, `2` drift, `1` local error. `--dry-run` computes and prints everything without writing a file. `--out DIR` chooses another directory.

Fingerprints are recorded for the adapters that decode through the shared JSON helpers; an account answered from the metered-query record (a provider billed per query, inside its minimum interval) makes no request and so carries no fingerprint that run.

CI never holds a provider secret: its `canary-dry-run` job runs the detector, the report, the issue draft and the evidence list against a fixture provider with one fixture mutated between runs (`crates/cuotai-tui/tests/canary.rs`).

## Scheduling (daily)

Run it as your own user so the vault is reachable. Cuotai installs nothing.

### macOS (launchd)

`~/Library/LaunchAgents/dev.cuotai.canary.plist`, then `launchctl load ~/Library/LaunchAgents/dev.cuotai.canary.plist`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>Label</key><string>dev.cuotai.canary</string>
  <key>ProgramArguments</key><array>
    <string>/usr/local/bin/cuotai</string><string>--canary</string>
  </array>
  <key>StartCalendarInterval</key><dict>
    <key>Hour</key><integer>9</integer><key>Minute</key><integer>0</integer>
  </dict>
</dict></plist>
```

### Linux (systemd user timer)

`~/.config/systemd/user/cuotai-canary.service`:

```ini
[Service]
Type=oneshot
ExecStart=/usr/local/bin/cuotai --canary
SuccessExitStatus=2
```

`~/.config/systemd/user/cuotai-canary.timer`, then `systemctl --user enable --now cuotai-canary.timer`:

```ini
[Timer]
OnCalendar=daily
Persistent=true

[Install]
WantedBy=timers.target
```

### Windows (Task Scheduler)

```powershell
schtasks /Create /SC DAILY /ST 09:00 /TN "Cuotai canary" /TR "\"C:\Program Files\Cuotai\cuotai.exe\" --canary"
```

Adjust the path to where `cuotai` is installed. Read `report.md` in the canary directory after a run.
