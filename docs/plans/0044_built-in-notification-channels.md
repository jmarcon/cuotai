# PRD 0044 — Built-in notification channels: webhook, ntfy, Slack and Discord

| Field | Value |
|---|---|
| ID | 0044 |
| Status | Proposed |
| Owner | `cuotai-core` (`channels` module: definitions, payloads, delivery, consent state, vault references), desktop (Settings › Alerts › Channels), `cuotai-tui` (`--channel-*`, `--notify` delivery) |
| Consumers | PRD 0003/0008/0023/0024/0032 notifications, PRD 0043 digest, PRD 0041 floors |
| Depends on | PRD 0032 (single preference source, delivery decision, quiet hours, private texts, cross-process ledger), PRD 0029 (account id in payloads), the OAuth vault store (URL with an embedded secret kept in the vault) |
| Benefits from | PRD 0013 (hooks remain the escape hatch for anything else), PRD 0031 (contract documentation), PRD 0028 (report-a-problem for a failing channel) |
| Extends | PRD 0032 RF3 (one more deliverer beside the system notification), PRD 0013 (a no-script alternative) |
| Date | 2026-10-25 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; the acronym is kept so references stay stable. A **channel** is a destination the person owns (a webhook URL, an ntfy topic, a Slack or Discord incoming webhook) to which Cuotai delivers the **same notifications** it would show on the system, after the same policy. It is not a general event stream: that is `--watch` and hooks (PRD 0013).

## Implementation status (2026-10-25)

| RF | Version | Delivery |
|---|---|---|
| RF1–RF7 | — | Proposed. |

## 1. Context

Cuotai notifies through the operating system, and anything else needs a hook: a `hooks.toml` rule, a command as an argument list, a terminal confirmation and a script the person writes (`docs/events.md`). That is right for automation and wrong for the intermediate user who only wants "tell my phone when Claude runs out" or "post in our team channel when the shared OpenRouter key is at 90%".

The tools in the space ship destinations, not scripts: LiteLLM's alerting sends soft-budget and failure alerts to a Slack webhook ([LiteLLM docs, alerting, read 2026-10-25](https://docs.litellm.ai/docs/proxy/alerting.md)); tokenkarma advertises "alerts before limits are reached" ([AlternativeTo, read 2026-10-25](https://alternativeto.net/software/quotum/)); Fetchy Pro's alerts are per account with 1% thresholds ([App Store listing](https://apps.apple.com/app/id6786673900)). None of them deliver to a phone through a self-hostable service such as ntfy, and none of them apply quiet hours or private texts to what leaves the machine.

Cuotai's principles frame the design: nothing is sent to anyone unless the person configures a destination; the destination is theirs; what is sent is the notification text already governed by PRD 0032 (private by default), never readings in bulk; the URL (which usually embeds a secret) lives in the vault, not in the TOML.

## 2. Problem

- A person away from the computer learns that a quota ran out when they come back.
- A team sharing a key or an aggregator has no way to get one message in their channel without someone writing and maintaining a script.
- Hooks require a terminal confirmation and a program; the desktop hooks editor still needs a command.
- Every incoming-webhook URL embeds a token, and people would paste it into `hooks.toml` next to the accounts file.

## 3. Goals

- Up to **5 channels** of four kinds: **generic webhook** (JSON POST), **ntfy** (topic on `ntfy.sh` or a self-hosted server), **Slack incoming webhook**, **Discord webhook**.
- Channels deliver exactly the notifications PRD 0032 decides to deliver (resets, thresholds, exhaustion, recovery, projection, overage, floors, session expiry, limit changes, the digest), with per-channel filters (kinds, accounts, minimum severity).
- The destination URL is stored in the **vault**; the TOML/preferences keep a reference and the host name only.
- Consent and transparency: adding a channel shows the host and an example payload; **Send test** proves it; every delivery is logged locally (route, status, time; never the body).
- Cross-process: the desktop and `cuotai --notify` share the ledger so one crossing is sent once.

## 4. Non-goals

- E-mail, SMS, push through a Cuotai service: there is no Cuotai service, and e-mail needs credentials Cuotai should not hold (decision O of PRD 0028 also keeps support off e-mail).
- Sending readings, snapshots or statistics on a schedule: that is `--serve`, `--watch` and hooks; a channel carries notification texts and the opt-in digest only.
- Two-way channels (buttons, commands from Slack): read-only principle; nothing comes back.
- Templates with arbitrary user-scripted payloads: the generic webhook has a fixed documented JSON; people needing another shape use a hook.

## 5. Functional requirements

### RF1 — Channel model and storage (high priority; prerequisite)

- **RF1.1** `channels::Channel { id, kind: Webhook | Ntfy | Slack | Discord, label, host, vault_ref, filters, enabled, created_at }` in the core; definitions in the PRD 0032 core-owned preferences file (`notify.channels[]`), the URL in the vault under `cuotai/channel/<id>`; a dev-SQLite store honours the same debug-only rules as OAuth tokens.
- **RF1.2** URL validation at the boundary: `https://` only (plain `http://` allowed only for loopback and RFC 1918 addresses, for self-hosted ntfy, with a warning), a host that is not an IP literal of a Cuotai-known provider, no userinfo, ≤ 2 048 bytes; Slack URLs must be under `hooks.slack.com`, Discord under `discord.com`/`discordapp.com` `/api/webhooks/`; ntfy is `<server>/<topic>` with a topic of `[A-Za-z0-9_-]{1,64}`.
- **RF1.3** Filters: `kinds[]` (the notification kinds of PRD 0032; default all except `digest`), `accounts[]` (stable ids; default all), `min_severity` (`info` | `warning` | `critical`; exhaustion and floor are critical, thresholds warning, resets and recovery info), `include_names: bool` (default follows PRD 0032's private setting).
- **Acceptance criterion:** validation table tests (every rejection with its message), vault round trip with the URL never in the preferences file or any log, filters serialization.

### RF2 — Payloads (high priority)

- **RF2.1** One **core-rendered** notification (title, body, severity, kind, `occurred_at`, provider, account id and optional scrubbed name, metric label and window, values, `resets_at`, a deep link `cuotai://account/<id>`) feeds four renderers:
  - **Webhook:** `POST` JSON `{ "schema": 1, "kind", "severity", "title", "body", "occurred_at", "provider", "account": { "id", "name"? }, "metric"?, "current"?, "resets_at"?, "link" }` with `Content-Type: application/json`, `User-Agent: cuotai/<version>`, optional `X-Cuotai-Signature: sha256=<HMAC>` when the person set a signing secret (vault).
  - **ntfy:** `POST` with headers `Title`, `Priority` (1–5 from severity), `Tags` (an emoji short-code per kind), `Click` (the deep link), and the body text.
  - **Slack:** `{ "text": "<title>\n<body>" }` plus a minimal `blocks` section (header + section); no attachments.
  - **Discord:** `{ "content": "**<title>**\n<body>" }`, with a 1 900-character cap.
- **RF2.2** Texts come from PRD 0032's renderer in the person's language, with `include_names` deciding whether the account name appears; never a credential variable name, URL, e-mail or raw provider text (the export's redaction).
- **RF2.3** Bodies are capped (4 KiB webhook, 4 KiB ntfy, 3 KiB Slack, 1 900 chars Discord) with an ellipsis.
- **Acceptance criterion:** golden payloads for each kind × a threshold, an exhaustion, a digest; the redaction test suite of PRD 0032 run over every payload; the HMAC computed over the exact body.

### RF3 — Delivery (high priority)

- **RF3.1** `channels::deliver(notification, channels)` runs after PRD 0032's decision (quiet hours hold channel deliveries exactly as system notices; exhaustion breaks through) and after the shared-ledger claim (one delivery per crossing across processes). Deliveries run in the background, at most 2 in flight, 10 s timeout each, 3 attempts with 5 s / 30 s backoff on network errors and `5xx`, no retry on `4xx` (the channel is marked **failing** with the status code).
- **RF3.2** Rate limit per channel: at most 20 deliveries per hour; beyond that, the rest of the hour's notifications become one "N more notifications were not sent (rate limit)" message at the hour's end. Never a burst at startup: the first minute after start delivers only critical kinds.
- **RF3.3** A local **delivery log** (`<OS local data dir>/cuotai/channels/log.jsonl`, 7 days, 0600): time, channel id, kind, outcome, status code, attempt; never the body or URL. Shown in Settings and `cuotai --channel-log`.
- **RF3.4** A channel failing for 24 hours gets one system notification "Channel <label> is failing (403)" and a card-free banner in Settings; it is never disabled automatically.
- **Acceptance criterion:** `wiremock` tests for the retry matrix, the rate limit with the summary message, the quiet-hours hold and break-through, the two-process ledger (desktop + `--notify`), the log redaction.

### RF4 — Consent and test (high priority)

- **RF4.1** Adding or editing a channel shows a dialog with: the kind, the host, the filters in words ("Thresholds, exhaustion and floors of 3 accounts, names hidden"), an **example payload** rendered from a sample notification, and the sentence "Cuotai will send these texts to this address when a notification is due. Nothing else leaves this computer." **Save** is the consent; the channel is created enabled.
- **RF4.2** **Send test** sends a sample marked `"sample": true` (webhook) or titled "Cuotai test" and reports the status; the TUI equivalent is `cuotai --channel-test <id>`.
- **RF4.3** A channel whose URL changed (edit) asks consent again; disabling needs no dialog.
- **Acceptance criterion:** dialog state tests, the sample payload flag, the re-consent on URL change.

### RF5 — Editors (medium priority)

- **RF5.1** Settings › Alerts › **Channels** (DS-CMP-LIST): list with kind icon, label, host, filters summary, last delivery and outcome, **Send test**, **Edit**, **Remove** (confirmation; the vault entry is deleted). **Add channel** picks the kind, pastes the URL (masked after validation), sets the label and filters.
- **RF5.2** CLI: `cuotai --channel-add <kind> --label L [--filter ...]` reads the URL from stdin or a prompt (never from an argument, so it does not enter shell history), prints the consent text and asks for confirmation on a terminal; `--channel-list`, `--channel-remove <id>`, `--channel-test <id>`, `--channel-log`.
- **RF5.3** `cuotai --notify` delivers to channels when run on a terminal or as a service (headless), sharing the ledger with the desktop.
- **Acceptance criterion:** editor tests, the stdin URL path, the masked display, the headless `--notify` smoke in CI with `wiremock`.

### RF6 — Digest and hooks interplay (medium priority)

- **RF6.1** The PRD 0043 digest is a kind (`digest`, severity info, Markdown body for webhook/Slack/Discord, plain for ntfy), off by default per channel.
- **RF6.2** Hooks keep receiving events as today; a notification delivered to a channel is also visible to hooks as `notification_sent` (additive, with kind and channel id only), so a person can audit both paths.
- **Acceptance criterion:** the digest payload goldens, the `notification_sent` event.

### RF7 — Documentation (low priority)

- `docs/notifications.md` (new, or a section of `gui-features.md`): kinds, filters, the four payloads with examples, self-hosted ntfy guidance, the delivery log, what never leaves the computer; `docs/advanced.md` CLI; `docs/events.md` (`notification_sent`); `SECURITY.md` note on the vault storage of URLs.

## 6. Non-functional requirements

- **Privacy:** a channel is created only by the person; payloads carry PRD 0032's private texts by default; the URL and signing secret live in the vault; the log never holds bodies or URLs; no provider reading is sent in bulk.
- **Safety:** HTTPS enforced (loopback/RFC 1918 exception for ntfy with a warning), host allow-lists for Slack and Discord, no redirects followed, 10 s timeout, bounded retries and rate.
- **Reliability:** once per crossing across processes (shared ledger); failures never block or delay a system notification or a refresh.
- **Localization:** texts in the person's language from the core catalogs.

## 7. Data and contract changes

- **Preferences (PRD 0032 file):** `notify.channels[]` (`id`, `kind`, `label`, `host`, `filters`, `enabled`, `created_at`); no URL.
- **Vault:** `cuotai/channel/<id>` (URL), `cuotai/channel/<id>/secret` (HMAC key, optional).
- **Files:** `channels/log.jsonl`.
- **Events (additive):** `notification_sent`.
- **CLI:** `--channel-add|list|remove|test|log`.

## 8. Surfaces

| Surface | Change |
|---|---|
| `cuotai-core` | `channels` module, renderers, delivery, log, validation, vault refs |
| Desktop | Settings › Alerts › Channels, consent dialog, failing banner |
| TUI/CLI | `--channel-*`, `--notify` delivery |
| Docs | notifications guide, advanced, events, SECURITY |

## 9. Dependencies and sequencing

After PRD 0032 (it reuses the decision, texts and ledger). RF1–RF4 together (storage, payloads, delivery, consent), then RF5, RF6, RF7. PRD 0043's digest plugs in when it ships. Lane: `channels` and one Settings page.

## 10. Risks

| Risk | Mitigation |
|---|---|
| A pasted URL goes to the wrong place | Host shown in the consent dialog and the list; Slack/Discord allow-lists; **Send test** |
| Secret leakage through logs or preferences | URL in the vault only, host-only display, log without bodies, tests over every log line |
| Notification storms | Per-channel hourly cap with a summary, startup grace, PRD 0032 grouping |
| Self-hosted ntfy over HTTP | Allowed only for loopback/RFC 1918 with a warning; documented |
| Perceived as "Cuotai sends my data" | Opt-in per destination, plain consent text, nothing else sent, docs section "what never leaves" |

## 11. Order of execution and verification

1. **RF1**, **RF2** (model, vault, payloads).
2. **RF3**, **RF4** (delivery, consent), CLI of **RF5**.
3. Desktop of **RF5**, **RF6**, **RF7**.

At each step: `cargo fmt`, `clippy -D warnings`, workspace and desktop tests; a smoke with a self-hosted ntfy on loopback and a Discord test webhook: a 90% crossing produces one message with the desktop and `cuotai --notify` both running, the message hides the account name by default, and `grep` of the preferences and logs for the URL finds nothing.

## 12. Open questions and decisions to take

| Question | Recommendation |
|---|---|
| Telegram bot or Pushover as fifth and sixth kinds? | Later; both need tokens beyond a URL. Generic webhook covers them through relays. |
| Should channels be allowed without a vault (CLI-only, headless Linux server)? | Yes with `--dev-credentials-sqlite` in debug builds only, as OAuth today; release builds need Secret Service or a file-based keyring decision (§12 of PRD 0012 history). |
| HMAC signing default | Off; one switch per webhook channel, secret generated by Cuotai and shown once. |
| Deep link scheme `cuotai://` | Register it in the desktop (Tauri) with PRD 0025; until then the link is omitted. |
