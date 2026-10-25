# Local contract canary

`cuotai --canary` is a maintainer tool (PRD 0028 RF4). It runs on your own machine with your own accounts and the tokens already in the OS vault, once, and tells you when a provider changed the interface Cuotai reads. CI never holds a provider secret, and the canary never files an issue or writes to the repository by itself.

## What it does

1. Runs one refresh of the configured accounts with the normal per-account deadlines. Accounts billed per query (AWS Cost Explorer) are left out unless you pass `--canary-include-metered`.
2. Writes a **shape record** per provider and connection mode to `<OS data dir>/cuotai/canary/run-<stamp>.json` (`0600`, the last 30 runs kept): the status, the failure category, the drifted field paths and their types, the response digest of a failed decode, and the metrics present by label, kind, window and unit. No value, account name, e-mail, organization, URL, header or body is ever written.
3. Compares it with the previous run of the same provider and mode: a metric new or absent, a new `schema_changed` or `auth_required` failure. Undocumented and session interfaces are listed first. Two runs with nothing changed produce no alert.
4. Prints a value-free Markdown report (also saved as `report.md`) and one issue draft per provider and category, built by the same function as `--report-problem` plus the shape diff. `--canary-open` opens each GitHub link in the browser; otherwise the drafts are only printed.
5. Writes `verified_on.json`: the interfaces that answered with no drift today, as a list for you to review before raising their `verified_on` date. It is not applied automatically.

Exit code: `0` no drift, `2` drift, `1` local error. `--dry-run` computes and prints everything without writing a file. `--out DIR` chooses another directory.

Not covered yet: the response fingerprint of a successful answer (only failed decodes carry one, so a field that vanishes from an answer Cuotai does not read goes unseen), the status-component check (PRD 0011) and `cargo xtask access-verified --apply`.

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
