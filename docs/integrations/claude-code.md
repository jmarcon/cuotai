# Claude Code and Codex CLI: agent-fed readings

Cuotai can read your Claude plan windows from **Claude Code itself** instead of querying Anthropic with a token, and your Codex windows from **Codex CLI's own session logs**. Both are local: no network request, no token reuse, nothing sent anywhere (PRD 0040).

## Claude Code status line

Claude Code runs a command you choose as its [status line](https://code.claude.com/docs/en/statusline) and hands it one JSON object on standard input several times a minute. That object carries the 5-hour and weekly windows (`rate_limits`), the gateway spend limit when you have one, the context window share and the session's estimated cost. `cuotai --feed claude-statusline` stores the numbers; the Claude account set to **Fed by Claude Code** reads them.

### Set it up

1. Install the **terminal version** of Cuotai (`brew install jmarcon/cuotai/cuotai`, `scoop install cuotai/cuotai`, or the Linux archive or package; see [`../installing.md`](../installing.md)). The desktop app does not include it, and Claude Code runs it, not the app.

   Add this to `~/.claude/settings.json` (Cuotai never edits it for you; `cuotai --feed claude-statusline --setup` prints the same text, and the desktop has it with **Copy** under Settings › Dashboard › More display options › Claude Code, using the terminal version's full path when it finds one):

   ```json
   {
     "statusLine": {
       "type": "command",
       "command": "cuotai --feed claude-statusline --print"
     }
   }
   ```

   With `--print` the status line shows Claude's windows, the tightest other account and the context share. Use `cuotai --statusline-claude` instead for the same line with more options (below), or drop `--print` to store the reading and print nothing.

2. To keep your own status-line script, chain it: Cuotai reads a copy of the input and your script prints as before (`~/.claude/statusline.sh` stands for your script):

   ```json
   {
     "statusLine": {
       "type": "command",
       "command": "sh -c 'input=$(cat); printf %s \"$input\" | cuotai --feed claude-statusline; printf %s \"$input\" | ~/.claude/statusline.sh'"
     }
   }
   ```

3. Add (or edit) a Claude account and pick **Source › Fed by Claude Code**. In the TOML this is:

   ```toml
   [[accounts]]
   provider = "claude"
   params = { query_mode = "claude_code" }
   ```

   No credential is needed. When Claude Code is detected on the computer, the Add account dialog preselects this mode.

4. Send one prompt in Claude Code. `rate_limits` appears only for Pro, Max and Ultra subscribers (or behind a Claude apps gateway with a spend limit) and only after the first API response of a session.

### Modes

| `query_mode` | What answers | Network | Credential |
|---|---|---|---|
| *(absent)* | The internal interface the official clients use | yes | Claude OAuth token |
| `claude_code` | Claude Code's status line | no | none |
| `claude_code_or_api` | The status line while it is younger than the account's refresh interval, otherwise the query | only for the fallback | Claude OAuth token |

A query-mode account can still show Claude Code's live session with `params = { feed = "claude_code" }`. Claude Code's JSON carries no account identity, so only **one** account may read it (its feed mode or its `feed` binding); two are a configuration error. If you have a work and a personal Claude account, bind the feed to the one Claude Code is signed in with.

**Two accounts in different Claude Code windows.** The card merges the windows of every recent session (below). If two Claude Code windows are signed in to different Claude accounts, Cuotai cannot tell their readings apart and may show one account's 5-hour window beside the other's weekly window. Bind the feed to one account and use Claude Code with that account only.

### What the card shows

- The 5-hour and weekly windows as the query mode shows them, with the reading's age counted from when Claude Code last spoke ("read 2 h ago"), on the card, in the details, in `--once`, in the menu bar, in the widget and in `--status-line`. When Claude Code is closed the reading ages and turns **Aging**, then **Stale** (and `--status-line` exits 2); it never turns into 0 and never reads "Up to date".
- A window Claude Code dropped after its reset stays **unknown** until the next prompt brings the new one.
- Each window comes from the newest session that reported it with its reset still ahead. A new Claude Code window that has not reported its limits yet (before its first API response) does not blank the card, and its status line shows the account's last known windows. The card's age counts from the oldest reading it uses.
- `claude_code_or_api` uses the feed only while a reading **with** limits is younger than the refresh interval; a fresh session without limits never decides it, so the query answers instead.
- **Live session** while a session sent a reading in the last 10 minutes: model, "ctx 8%" (the context window share; information only, never an alert) and "≈ $0.42 this session" (Claude Code's own estimate at list price, not a charge). Settings › Dashboard › Show live agent session turns it off by writing `live_sessions = false` to the accounts file (the same key the terminal reads): then nothing computes or exports the sessions, neither the cards, the details, `--once`, the tray chip, `--json-v1` nor `/v1/*` of the local servers. Claude Code still writes its readings, and the status line keeps its own `ctx N%`. Hooks, the widget and notifications never carry the sessions. The terminal shows `ctx 8% · ≈$0.42` on the card and `--once` prints a `Live session:` line.

### `cuotai --statusline-claude`

```sh
cuotai --statusline-claude [--account ID]... [--from-server [URL]] [--max-width N] [--color]
```

Reads the status-line JSON on standard input (like `--feed`), stores it and prints one line: `5h 24% · Wk 41% ↺ 19:30 · Codex 64% left · ctx 8%`.

- The other accounts come from a running `cuotai --serve` or the desktop's local server with `--from-server` (loopback only, `localhost` read as `127.0.0.1`, the token from `--token-file` or the server's default file, never on a command line and never through a proxy, and sent only after the server proved it holds it; see [Who receives the token](../local-server.md#who-receives-the-token)), or else from the last reading recorded in the history (within 100 ms of the command's start, then the history answers). It **never queries a provider**: Claude Code calls it often.
- A stale reading, the other account's or Claude's own windows written longer ago than the account's refresh interval, ends with its age in parentheses, in your language: `Wk 41% (2 h)`, `Codex 64% left (3 min)` (`(stale)` when its time is unknown). The menu bar and `--status-line` mark a stale figure the same way. `--max-width` never drops the age from a part it keeps.
- An exhausted other account says when it returns: `Codex 0% until 14:30`.
- The spend limit and the session cost use your language's money format (`$314.12/$500.00`, `314,12 $/500,00 $` in German).
- `--account ID` limits the other accounts. Like `--status-line`, it takes an account's id, its key or a name no other account has; a value that names no single account is reported on stderr (with the ids) and the command exits `1`. Claude accounts are left out (their windows come from the feed).
- `--max-width N` drops the context share first, then the other account, then cuts Claude's windows with "…".
- Colour only with `--color`, on a terminal, with `NO_COLOR` unset.

Both commands read standard input up to the end of the first JSON object, and wait for it at most a quarter of a second: a writer that sends nothing, or never closes its end, cannot hold Claude Code's prompt.

Exit codes: `0` for a valid object (absent windows are normal), `1` for malformed input or input over 64 KiB, `3` when no input arrived within 250 ms (nothing on stdout, one line on stderr).

Feed files that fail validation (truncated, edited by hand, out of contract) are ignored, never read as 0: `cuotai --doctor` and the fed account's details say how many. A temporary file left by an interrupted write is removed a minute later.

`cuotai --feed claude-statusline --forget` erases the stored Claude Code readings and exits (`0`, or `1` when they could not be erased).

### Privacy

From the status-line JSON Cuotai keeps only: the windows' percentages, resets, spend dollars and period; the context share and size; the session cost estimate; the model id and name (when they are plain text of up to 64 characters); the Claude Code version; and a **digest** of the session id (SHA-256, 16 hex characters). The working directory, `workspace`, `transcript_path`, `output_style`, the session name, prompts and every other field are never copied into a structure, written or logged.

The readings live in `<OS local data>/cuotai/feed/claude_code/` (on macOS `~/Library/Application Support/cuotai/feed`), one fixed folder per user that `CUOTAI_DATA_DIR` and the desktop's data-folder setting do not move, so the status line and the app always agree. A `feed` folder that older builds left in a moved data folder is deleted at the next refresh. One `0600` file per session in a `0700` directory, deleted after 24 hours and capped at 64 files. While an account reads them (its feed mode or `feed` binding), the newest one is kept so a quiet agent ages instead of disappearing; without such an account it is deleted after 24 hours too. `docs/privacy.md` lists the directory; **Erase all data** removes it.

Installing the status line is your consent: Cuotai reads the readings only for the account bound to Claude Code, and the banner only checks whether a recent one exists. To stop, remove the `statusLine` entry from `~/.claude/settings.json` (Claude Code then stops writing) and erase what is stored with `cuotai --feed claude-statusline --forget` or the desktop's Settings › Dashboard › More display options › Claude Code › **Forget Claude Code readings**.

The status line is a documented extension point of Claude Code and Cuotai reuses no token through it. That is documented, not "approved": Anthropic's terms still govern how you use Claude Code.

## Codex CLI

Codex CLI writes its rate-limit windows into its own session logs (`token_count` events under `CODEX_HOME` or `~/.codex/sessions`). A ChatGPT / Codex account with **Source › Fed by Codex CLI** reads the recent ones:

```toml
[[accounts]]
provider = "chat_gpt"
params = { query_mode = "codex_cli" }   # or "codex_cli_or_api"
```

- Reading the logs needs the **Codex CLI** local source turned on (Local cost: `cuotai --cost-enable codex_cli`, or the desktop's Local cost page), whose consent lists exactly the folders read and says that the rate-limit windows are kept for up to a day. Without it the account says so instead of showing numbers, a `codex_cli_or_api` account says why the query answered, and nothing shows or exports the stored Codex readings (live session, `--once`, `--json-v1`, the local servers). Turning the source off erases them.
- Only the logs modified in the last 24 hours are read, newest first and at most 16, until both windows are found: each window comes from the newest log that reported it, so a session that has not reported its limits yet hides nothing. Of each log only the first line (the session id, digested at once) and the last 256 KiB are read; symbolic links are not followed. Each line is parsed into a structure that keeps only the time, the model, the session id and the rate-limit numbers: messages, instructions, working directories and tool output are skipped, never deserialized.
- The format is not a published API: it was checked against the Codex CLI source (`RateLimitSnapshot` in `codex-rs/protocol/src/protocol.rs`, 2026-11-05). A shape Cuotai does not recognise is reported as a changed interface, never as 0.
- Cuotai never starts Codex CLI or calls its local RPC.

The windows use the same labels as the query mode, so the history continues when you switch.
