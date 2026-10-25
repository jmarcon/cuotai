# Local HTTP server (`serve::LocalServer`)

`cuotai-core` can publish the [`export::v1`](export-v1.md) contract over a read-only HTTP server on loopback, so status bars and scripts read the last known state without making Cuotai query a provider on every read (PRD 0012 RF2 and RF3). It lives behind the core's `serve` feature, which is **off by default**; the front ends that offer it turn it on (`cuotai --serve` since PRD 0012 RF4, the desktop's Settings › Advanced › Integrations since RF5), and the server itself stays off until the user starts it.

## From the command line (`cuotai --serve`)

```sh
cuotai --serve                               # http://127.0.0.1:6738 exactly, token required
cuotai --serve --port 7000                   # exactly this port; taken is an error
cuotai --serve --token-file ~/.cuotai-token  # your own token file instead of the generated one
cuotai --serve --no-token                    # no token: any local user can read (warned on stderr)
curl -s -H "Authorization: Bearer $(cat "<token file announced on stderr>")" http://127.0.0.1:6738/v1/limits
```

- It runs in the foreground without opening the TUI and without needing a terminal (a service manager, `nohup` or a status bar can start it). It loads the configuration like `--once` (`--config`, `CUOTAI_CONFIG`, `./cuotai.toml`, the OS default), queries every account once at start and then each one on its own interval with the core's backoff, exactly as the dashboard would; requests only read that state. `--status`/`CUOTAI_STATUS` add the providers' public status (`incident`), and the readings enter the local history as with the TUI.
- Until the first sweep answers, the data routes give `503` (an account restored from the history serves its last reading, `stale`).
- **Port (exact since PRD 0031 RF9):** it binds exactly `--port N` or, without it, 6738; there is no fallback to another port, so the address your integrations know never moves. A taken port is a local error (exit `1`) that names the port, says when the program holding it is another Cuotai server (the desktop app's, or another `cuotai --serve`; `serve::is_cuotai_server`) and suggests `--port`. `--port 0` lets the system choose.
- **Token (required by default since 0.78.0):** every request, `/health` included, needs `Authorization: Bearer <token>`; without it the answer is `401`. Without options, the first start generates a random `cuotai_…` token in `<OS local data dir>/cuotai/serve/token` (for example `~/Library/Application Support/cuotai/serve/token` on macOS or `~/.local/share/cuotai/serve/token` on Linux), a `0600` file in a `0700` folder, prints that path (never the token) on stderr and reuses the file on every later start; a file there that is a symbolic link, belongs to another user or holds no valid token is a local error, and a wider mode is narrowed back to `0600`. `--token-file FILE` reads the token from a file you control instead (trimmed; 16 to 512 characters of `A–Z a–z 0–9 - . _ ~ + /`, for example `printf 'cuotai_%s\n' "$(openssl rand -hex 24)" > ~/.cuotai-token && chmod 600 ~/.cuotai-token`) and warns on stderr when other users can read it. `--no-token` (not combinable with `--token-file`) serves without a token and warns that **any user or program on this computer** can then read your usage; web pages and other computers are still refused.
- **Migration from 0.77.x and earlier:** a script or status bar that read `cuotai --serve` without a token now gets `401`. Either send the generated token (`curl -H "Authorization: Bearer $(cat <token file>)" …`), point `--token-file` at a token you already distribute, or keep the old behavior explicitly with `--no-token`.
- **Output:** the address alone on stdout (`http://127.0.0.1:6738`), so a script that started it can read it; on stderr one line per request (time, peer, method, path without query, status, duration), never a header or the token.
- **Shutdown:** `SIGINT` (Ctrl+C) or `SIGTERM` (on Windows, Ctrl+C or Ctrl+Break) closes the port, cancels queries in flight and lets queued history readings land.
- **Exit codes (PRD 0031 RF10):** `0` when it is stopped by `SIGINT`/`SIGTERM`, whatever the accounts' state; `1` for a local error (a taken port, the configuration, the token file or an invalid argument). `--strict-exit` restores the earlier behavior for supervisors that want it: `2` when no account had a healthy (`ready`) reading until the shutdown. Earlier releases exited `2` in that case; a supervisor that relied on it adds `--strict-exit`.
- **With `--watch`** (PRD 0013 RF2): `cuotai --serve --watch` serves and streams events from one refresh cycle, so a reading is queried once for both. stdout then belongs to the events, so the address goes to stderr (`cuotai: servidor local em http://…`), `SIGHUP` also stops it, and, with `--strict-exit`, the exit code is `2` when either rule applies (no healthy account, or an account whose latest reading failed); without it a signal stop exits `0`. See [`events.md`](events.md).

`cuotai --json-v1` prints the same `Export` once, after one query, without a server (exit codes as `--json`).

## In the desktop app (Settings › Advanced › Integrations)

- **Off by default.** The switch "Serve my usage on a local address" starts the server inside the app; it is remembered in `desktop.json` (`serve`) and starts again with the app until you turn it off. Turning it off closes the port.
- **Same refresh cycle.** The server serves the app's own readings: every time the window updates, the app hands the server a copy of the state it shows, and each request only builds the export from it. No second refresh loop runs and no request queries a provider.
- **Port:** 6738 by default, any port from 1024 to 65535. Exactly that port is used (no fallback), so the address your integrations know never moves. When it is taken, the server stays off and the section (and a banner on the main window) says why; when the program on that port answers like a Cuotai server (`cuotai --serve`, or another copy of the app), it is named as such, so you can stop it or pick another port. The effective address appears with **Copy address** while it is on.
- **Token:** **Generate token** stores a new `cuotai_…` token in the system vault (service `cuotai.serve`, never the TOML) and requires it as `Authorization: Bearer <token>` from then on. **Copy token** copies it without showing it; **Generate new token** replaces it, so the old one gets `401` at once; **Revoke token** removes it from the vault and the server answers without a token again (loopback programs only, never web pages — but any local user or program among them, so keep a token unless you are the only user of the computer). If the vault later lacks the token or refuses access, the server stays off instead of serving without one.
- **Allow web browsers** asks for confirmation with `serve::allow_browsers_warning()` before turning on `allow_browsers`.
- The app keeps no access log.

```sh
curl -s http://127.0.0.1:6738/v1/limits
curl -s -H "Authorization: Bearer $(pbpaste)" http://127.0.0.1:6738/v1/snapshot   # after "Copy token" (macOS)
```

## Starting it (Rust)

```toml
cuotai-core = { path = "crates/cuotai-core", features = ["serve"] }
```

```rust
use std::sync::{Arc, Mutex};
use cuotai_core::export::v1::{Export, Options};
use cuotai_core::serve::{LocalServer, ServeConfig};

// `state` is what the front end already keeps: the configuration, its identity,
// the RefreshCycle its scheduler updates and the Monitor (for provider statuses).
let state = Arc::clone(&shared);
let server = LocalServer::start(ServeConfig::default(), move || {
    let state = state.lock().unwrap();
    Some(Export::from_cycle(
        &state.config,
        &state.identity,
        &state.cycle,
        &state.monitor.incident_statuses(),
        chrono::Utc::now(),
        Options::default(),
    ))
})
.await?;
println!("serving on {}", server.url()); // http://127.0.0.1:6738
// ...
server.shutdown().await; // returns once the port is closed
```

The source is called for every data request and must only read what the refresh cycle already holds: the server never triggers a query, and there is no `?refresh=1`. Return `None` while there is no state at all.

## Address and port

- Binds to `127.0.0.1` by default (`::1` is also accepted). Any other address is a configuration error (`ServeError::NonLoopback`) unless the explicit `allow_non_loopback` flag is set, and then a token is mandatory (`ServeError::TokenRequired`). Access over the local network is out of scope: use your own tunnel (for example `ssh -L 9000:localhost:6738 host`), which keeps a loopback `Host`.
- The default port is **6738**, unassigned in the IANA registry (6717–6766 are unassigned) and clear of OpenUsage's 6736. When it is taken, `start` fails with `ServeError::PortInUse`: `ServeConfig::port_fallback` is **off by default** since PRD 0031 RF9.1 (neither `cuotai --serve` nor the desktop app sets it). A library consumer that turns it on makes the server try the next nine ports; `LocalServer::addr()` and `fell_back()` report what was bound. Port `0` lets the system choose. `serve::is_cuotai_server(port)` tells a front end whether the program holding a port is a Cuotai server: it asks `/health` once without a token (at most `PROBE_TIMEOUT`, 1 second) and recognises the documented answer or the `401` with `WWW-Authenticate: Bearer realm="cuotai"`.

## Routes

Every route answers `GET` and `HEAD` (PRD 0031 RF9.2: `HEAD` has the status and headers of `GET`, for every route and also for `401` and `404`, and no body); any other method answers `405` with `Allow: GET, HEAD`, and an unknown path answers `404`. Every response is `Content-Type: application/json` with `Cache-Control: no-store` and `X-Content-Type-Options: nosniff`, and closes the connection.

**`ETag` and `304` (PRD 0031 RF6).** `cuotai --serve` and the desktop server build the export once per state change (a reading, an edit or a provider status) and reuse it for at most 10 seconds, so a burst of requests costs one build. `/v1/snapshot`, `/v1/limits` and `/v1/accounts/{key}` then carry `ETag: "<build>-<route>"` (the route is `snapshot`, `limits` or `account`); a request whose `If-None-Match` names it (or `*`) gets `304` with no body and `Cache-Control: no-store` still set. The tag changes with each build, so a client sees `200` again after a new reading and at least every 10 seconds, which keeps `age_seconds` and `freshness` honest. Embedders whose `Source` does not tell generations get no tag and build per request.

| Route | Answer |
|---|---|
| `/health` | `{"status":"ok","version":"<Cuotai version>","schema":1,"schemas":[1]}`, even before the first reading. `schema` is the current `export::v1` number; `schemas` lists every number the server answers (during the support window of a previous schema, both), so a client can discover a newer one. See the [contract policy](export-v1.md#stability-and-deprecation-policy). |
| `/v1/snapshot` | The full `Export` ([`export-v1.md`](export-v1.md)). |
| `/v1/accounts/{id_or_key}` | One `AccountExport`, found by its stable `id` first (`/v1/accounts/acc_0123456789ab`, PRD 0029) and then by its `<provider slug>:<name>` key, percent-encoded (`/v1/accounts/claude:Personal%20Max`). Unknown ones answer `404`. |
| `/v1/limits` | The summary `Limits` (`schema`, `generated_at` and, per account, `index`, `id`, `key`, `name`, `provider`, `class`, `binding`, `age_seconds`, `freshness`, `stale`, `incident`, plus a cross-account `summary`), declared as `$defs/Limits` in [`export-v1.schema.json`](export-v1.schema.json). |

Until some account has a reading (current or preserved), the data routes answer `503` with `Retry-After: 5`. Errors carry `{"error":"<code>"}` with `not_found`, `method_not_allowed`, `no_data`, `unauthorized`, `host_not_allowed`, `browser_not_allowed`, `peer_not_allowed`, `bad_request`, `uri_too_long` or `headers_too_large`.

## Limits

`ServeConfig::bounds` (`Bounds::default()`): at most 16 connections at once (any other is closed as soon as it is accepted), a request head of at most 8 KiB and 32 headers (`431`), a request target of at most 2 KiB (`414`), and 5 seconds for the whole connection (a slower client is disconnected).

## Security

- **DNS rebinding:** the `Host` header must be exactly one of `localhost`, `127.0.0.1` or `[::1]` (any port; or the address bound with `allow_non_loopback`). A page on a rebound domain sends its own name and gets `403`.
- **Web pages:** a request with an `Origin` header, or with `Sec-Fetch-Site` other than `none`, comes from a page and gets `403`; no `Access-Control-Allow-Origin` is sent, so browsers do not let other origins read answers. `ServeConfig::allow_browsers` turns this off explicitly: pages are accepted, every answer carries `Access-Control-Allow-Origin: *` and CORS preflights get `204`. That means **any page open in the browser can read your usage** (with a token, any page that knows it); front ends show `serve::allow_browsers_warning()` before enabling it.
- **Token:** optional. When `ServeConfig::token` is set, every route requires `Authorization: Bearer <token>` and answers `401` with `WWW-Authenticate: Bearer realm="cuotai"` otherwise (compared in constant time). `Token::generate()` creates `cuotai_` plus 32 random bytes; `ensure_token(&SystemTokenVault)` returns the token stored in the OS vault (Keychain, Credential Manager, Secret Service; service `cuotai.serve`) or generates and stores one, and `TokenVault::delete` revokes it. The token never goes into the TOML, and `Token` has no `Display` and a redacted `Debug`. `Token::parse` accepts a user-supplied token (16 to 512 RFC 6750 characters). For headless front ends, `serve::resolve_token(&TokenSource, default_file)` decides the token and the notices to show: `TokenSource::Default` uses `ensure_token_file(default_token_file())` (created exclusively as `0600` in a `0700` folder, reused, refused when it is a link or another user's), `TokenSource::File` uses `read_token_file` and warns when `token_file_exposed`, and `TokenSource::None` returns `no_token_warning()`. Without a token, only loopback peers are served — every local user's programs among them.
- **No secrets:** the routes serve only the redacted export (no credentials, credential variable names, authenticated URLs or e-mails unless the producer includes identity). The access log (`ServeConfig::access_log`, one `AccessEntry` per answered request: time, peer address, method, path without its query, status and duration) never records headers, so no `Authorization`, nor query parameters.

Loopback is not a user boundary: **without a token, any local user or program** that can open a TCP connection to `127.0.0.1` can read the data, which is why `cuotai --serve` requires one by default. With a token, any program that can read the token file (your own user's, since the file is `0600`) can read the data, exactly as it could run `cuotai --json`.
