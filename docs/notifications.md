# Notification channels

Cuotai can send the notifications it shows on this computer to destinations you own: a **generic webhook**, an **ntfy** topic (on `ntfy.sh` or your own server), a **Slack** incoming webhook or a **Discord** webhook (PRD 0044). You need no script. For anything else, [hooks](events.md) still run your own commands.

Nothing is sent until you add a channel. A channel receives **the same notifications** the desktop or `cuotai --notify` shows, after the same preferences, quiet hours and private texts. It never receives readings in bulk.

## Adding a channel

- **Desktop:** Settings › Alerts › **Notification channels** › **Add channel**. Pick the kind, give it a name, paste the address and choose what it receives. **Continue** shows the consent step: the kind, the host, the filters in words and an example payload, followed by the sentence "Cuotai will send these texts to this address when a notification is due. Nothing else leaves this computer." **Save** is your consent, and the channel starts on. **Send test** posts a sample marked as a test.
- **Terminal:** `cuotai --channel-add <webhook|ntfy|slack|discord> --label NAME [--filter key=value]… [--sign]` reads the address from standard input or a prompt, never from an argument, so it never enters your shell history. It prints the same consent and asks for a confirmation in the terminal. When the address is piped, pass `--yes` after reading the consent. See [advanced usage](advanced.md#notification-channels).

You can add up to **5 channels**. Editing a channel with a new address asks for consent again; turning a channel off asks nothing. **Remove** deletes the address from the vault.

### Accepted addresses

| Kind | Address |
|---|---|
| Webhook | any `https://` address. A query string is allowed (some relays put a key there). |
| ntfy | `https://<server>/<topic>`, with a topic of letters, digits, `-` or `_` (1 to 64). Plain `http://` is accepted only for a server on loopback or a private network (RFC 1918, `localhost`), with a warning. |
| Slack | `https://hooks.slack.com/services/…` |
| Discord | `https://discord.com/api/webhooks/…` (or `discordapp.com`) |

Addresses with a user name or password, a `#` fragment, more than 2048 bytes or a provider API host are refused. Redirects are never followed.

## Filters

| Filter | Default | Values |
|---|---|---|
| Kinds | every kind but the weekly digest | `reset`, `threshold`, `exhausted`, `recovered`, `projected`, `overage`, `floor`, `session_expiring`, `limit_changed`, `digest` |
| Accounts | every account | stable account ids (`--channel-list --json` and the TOML show them) |
| Minimum level | every level | `info` (resets, recovery, limit changes, digest), `warning` (thresholds, projection, extra usage, session expiry), `critical` (exhaustion, balance floors) |
| Account names | follows "Hide account names in notifications" (hidden by default) | `on`, `off`, `default` |

From the CLI: `--filter kinds=threshold,exhausted,floor --filter accounts=acc_x --filter min_severity=warning --filter names=on`.

## What is sent

Every channel gets the notification's title and text in your language. When names are hidden, the text carries neither the account name nor the metric label. E-mail-shaped text and web addresses are replaced, and no credential, variable name or raw provider text is ever included.

**Webhook:** `POST` with `Content-Type: application/json` and `User-Agent: cuotai/<version>`:

```json
{
  "schema": 1,
  "kind": "threshold",
  "severity": "warning",
  "title": "Claude: 90% of the 5-hour window used",
  "body": "Resets 16:00",
  "occurred_at": "2026-11-02T14:00:00Z",
  "provider": "claude",
  "account": { "id": "acc_77ykc27avm82" },
  "metric": { "window": "window_5h" },
  "current": { "percent": 90.0 },
  "resets_at": "2026-11-02T16:00:00Z"
}
```

`account.name` and `metric.label` appear only when the channel shows names. A test adds `"sample": true`, with `kind` set to `test`. The hourly summary uses `kind` `rate_limit`. `link` is reserved for the `cuotai://` deep link, which is omitted until the desktop registers that scheme. With **Sign each message** (`--sign`), Cuotai generates a secret, shows it once and adds `X-Cuotai-Signature: sha256=<hex>`, the HMAC-SHA256 of the exact body with that secret.

**ntfy:** `POST` of the plain text with the headers `Title` (RFC 2047-encoded when not ASCII), `Priority` (3 info, 4 warning, 5 critical) and `Tags` (an emoji short-code per kind, for example `warning`, `rotating_light`, `calendar`).

**Slack:** `{"text": "<title>\n<body>"}` plus a header block and a section block. No attachments.

**Discord:** `{"content": "**<title>**\n<body>", "allowed_mentions": {"parse": []}}`, so no message ever pings anyone.

Bodies are capped (4 KiB for webhook and ntfy, 3 KiB for Slack, 1,900 characters for Discord) and end with an ellipsis when cut. The weekly digest (PRD 0043) is Markdown for webhook, Slack and Discord and plain text for ntfy. It is off by default for each channel; turn on the `digest` kind to receive it.

## Delivery

- Deliveries run in the background, at most 2 at a time, each with a 10-second timeout. A network error or a `5xx` is retried twice (after 5 s and 30 s). A `4xx` or a redirect is not retried, and the channel shows as failing with the status.
- At most **20 deliveries per channel per hour**. When the hour ends, the rest become one "N more notifications were not sent (rate limit)" message. In the first minute after Cuotai starts, only critical notifications go out.
- Quiet hours hold channel deliveries exactly like system notices, and an exhaustion breaks through. The desktop and `cuotai --notify` share `notified.json`, so one crossing reaches a channel once even when both are running.
- A channel failing for 24 hours gets one system notification ("Channel Phone is failing (403)") and a banner in Settings. It is never turned off automatically.
- A delivery that a channel accepts is also visible to [hooks](events.md) as `notification_sent`.

## The delivery log

`<OS local data dir>/cuotai/channels/log.jsonl` (`0600`, 7 days) holds one line per outcome: the time, the channel id, the kind, the outcome (`delivered`, `failed`, `rate_limited`, `held`, `failing_notice`), the HTTP status and the attempt. It never holds the body, the address or the label. Settings shows the latest entries, and `cuotai --channel-log [--json]` prints them.

## What never leaves this computer

Only the notification texts described above go to the addresses you added. The definitions in `desktop.json` (`channels`) keep the kind, the name, the **host only**, the filters and the switch. The full address (which usually embeds a token) and the signing secret live in the system vault as `channel_<id>` and `channel_<id>_secret`. In debug builds, `--dev-credentials-sqlite` keeps them in the development store, as with OAuth tokens; release builds need the system vault. `cuotai --vault-audit` lists these entries as channels and never as orphans.

## Self-hosted ntfy

Run ntfy on your own server and use `https://ntfy.example.com/<topic>`. For a server on this computer or your private network, `http://192.168.1.20/<topic>` is accepted with a warning. Anyone on that network can read the messages, so prefer HTTPS when the server is reachable from elsewhere. Pick a hard-to-guess topic on `ntfy.sh`: topics there are public to whoever knows the name.
