# Claude Code and Codex CLI: agent-fed readings

Cuotai can read your Claude plan windows from **Claude Code itself** instead of querying Anthropic with a token, and your Codex windows from **Codex CLI's own session logs**. Both are local: no network request, no token reuse, nothing sent anywhere (PRD 0040).

## Claude Code status line

Claude Code runs a command you choose as its [status line](https://code.claude.com/docs/en/statusline) and hands it one JSON object on standard input several times a minute. That object carries the 5-hour and weekly windows (`rate_limits`), the gateway spend limit when you have one, the context window share and the session's estimated cost. `cuotai --feed claude-statusline` stores the numbers; the Claude account set to **Fed by Claude Code** reads them.

### Set it up

1. Add this to `~/.claude/settings.json` (Cuotai never edits it for you; `cuotai --feed claude-statusline --setup` prints the same text, and the desktop has it with **Copy** under Settings › Dashboard › More display options › Claude Code):

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

### What the card shows

- The 5-hour and weekly windows as the query mode shows them, with the reading's age counted from when Claude Code last spoke. When Claude Code is closed the reading ages; it never turns into 0.
- A window Claude Code dropped after its reset stays **unknown** until the next prompt brings the new one.
- **Live session** while a session sent a reading in the last 10 minutes: model, "ctx 8%" (the context window share; information only, never an alert) and "≈ $0.42 this session" (Claude Code's own estimate at list price, not a charge). Settings › Dashboard › Show live agent session turns it off. The terminal shows `ctx 8% · ≈$0.42` on the card and `--once` prints a `Live session:` line.

### `cuotai --statusline-claude`

```sh
cuotai --statusline-claude [--account ID]... [--from-server [URL]] [--max-width N] [--color]
```

Reads the status-line JSON on standard input (like `--feed`), stores it and prints one line: `5h 24% · Wk 41% ↺ 19:30 · Codex 64% left · ctx 8%`.

- The other accounts come from a running `cuotai --serve` or the desktop's local server with `--from-server` (loopback only, the token from `--token-file` or the server's default file), or else from the last reading recorded in the history, marked with its age (`(3m)`). It **never queries a provider**: Claude Code calls it often.
- `--account ID` limits the other accounts; Claude accounts are left out (their windows come from the feed).
- `--max-width N` drops the context share first, then the other account, then cuts Claude's windows with "…".
- Colour only with `--color`, on a terminal, with `NO_COLOR` unset.

Exit codes: `0` for a valid object (absent windows are normal), `1` for malformed input or input over 64 KiB.

### Privacy

From the status-line JSON Cuotai keeps only: the windows' percentages, resets, spend dollars and period; the context share and size; the session cost estimate; the model id and name (when they are plain text of up to 64 characters); the Claude Code version; and a **digest** of the session id (SHA-256, 16 hex characters). The working directory, `workspace`, `transcript_path`, `output_style`, the session name, prompts and every other field are never copied into a structure, written or logged.

The readings live in `<data dir>/feed/claude_code/`, one `0600` file per session in a `0700` directory, deleted after 24 hours (the newest one is kept so a quiet agent ages instead of disappearing) and capped at 64 files. `docs/privacy.md` lists the directory; **Erase all data** removes it.

The status line is a documented extension point of Claude Code and Cuotai reuses no token through it. That is documented, not "approved": Anthropic's terms still govern how you use Claude Code.

## Codex CLI

Codex CLI writes its rate-limit windows into its own session logs (`token_count` events under `CODEX_HOME` or `~/.codex/sessions`). A ChatGPT / Codex account with **Source › Fed by Codex CLI** reads the newest of them:

```toml
[[accounts]]
provider = "chat_gpt"
params = { query_mode = "codex_cli" }   # or "codex_cli_or_api"
```

- Reading the logs needs the **Codex CLI** local source turned on (Local cost: `cuotai --cost-enable codex_cli`, or the desktop's Local cost page), whose consent lists exactly the folders read. Without it the account says so instead of showing numbers.
- Only the first line of the newest log (the session id, digested at once) and its last 256 KiB are read; symbolic links are not followed. Messages, instructions, working directories and tool output are never deserialized.
- The format is not a published API: it was checked against the Codex CLI source (`RateLimitSnapshot` in `codex-rs/protocol/src/protocol.rs`, 2026-11-05). A shape Cuotai does not recognise is reported as a changed interface, never as 0.
- Cuotai never starts Codex CLI or calls its local RPC.

The windows use the same labels as the query mode, so the history continues when you switch.
