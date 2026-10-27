# PRD 0048 — Sign in and capture a web session in the desktop

| Field | Value |
|---|---|
| ID | 0048 |
| Status | Awaiting owner verification |
| Owner | `cuotai-core` (`user_session::CaptureSpec`, `SessionDescriptor::capture`/`capture_allows`), desktop backend (`session_capture` module, `capture_session`/`cancel_session_capture` commands, `form_schema` `sessions[].capture`), desktop UI (`account-dialog.js`, desktop catalogs) |
| Consumers | Desktop users adding or editing an account whose provider reads a web session |
| Depends on | PRD 0016 (paste box, `preview_session`, `test_session`, SES-02 test before save) |
| Related | PRD 0047 (typed secrets in the desktop) |
| Date | 2026-10-27 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; "RNF" a non-functional one. All requirements here address the **GUI user**; the TUI is unaffected (RNF5).

## Implementation status (2026-10-27)

| Item | Version | Delivery |
|---|---|---|
| RF1–RF8, RNF1–RNF5 | vNEXT | Delivered on `feat/webview-session-capture`: core capture metadata and allow-list, the desktop `session_capture` module (window, private store, polling, reduction, state machine), the account dialog button and the texts in the 14 desktop catalogs. Enabled for the Xiaomi MiMo platform session only. A live sign-in was not run by the implementing agent (no real account may be used); the owner's manual test is pending. |

## 1. Owner request (2026-10-27)

In the desktop app the user should no longer open the developer tools and copy cookies. Next to a web-session paste box, **Sign in and capture session** ("Entrar e capturar sessão") opens a Cuotai-owned window at the provider's page; the user signs in there (password, 2FA and captcha typed by the user); Cuotai reads the session cookies from that window's own cookie store, keeps only the descriptor's cookies, stages them through the existing paste path, runs the existing test and closes the window. Start with the Xiaomi MiMo platform session (`https://platform.xiaomimimo.com/console/plan-manage`, confirmed by the owner), designed per descriptor.

## 2. Problem

PRD 0016 RF2.1 asks the user to sign in in a browser, open the developer tools, find a request and copy its `Cookie` header. It works, but it is the hardest step of adding a Xiaomi MiMo platform account, it exposes the whole cookie jar to the clipboard, and many users stop there.

## 3. Requirements (GUI user)

- **RF1 — Consent by click.** The window opens only when the user clicks **Sign in and capture session** next to a paste box whose descriptor declares a capture (`form_schema` `sessions[].capture`). A hint under the button says what happens: a private Cuotai window, the user types the credentials, only the session's cookies are kept, the window's data is erased. Nothing opens at dialog start, on provider change or after a failed test.
  - RF1.1: the capture is declared per descriptor by the core (`SessionDescriptor::capture: Option<&CaptureSpec>` with `start_url`, `cookie_url`, `login_hosts`, `login_suffixes`). Today only `XIAOMI_PLATFORM` declares one.
  - RF1.2: one capture at a time; a second click while a window is open focuses it and says so.
- **RF2 — Isolation.** The window has a dedicated, non-persistent cookie store that the main app webview never shares.
  - RF2.1: `incognito(true)`: macOS uses a non-persistent `WKWebsiteDataStore`; Windows uses an InPrivate WebView2 profile; WebKitGTK an ephemeral context.
  - RF2.2: on Windows and Linux the window also gets a fresh data directory under the system temporary folder (`cuotai-session-capture-<pid>-<nanos>-<n>`), so it is apart from the main window even where `incognito` does nothing (WebView2 older than 101.0.1210.39).
  - RF2.3: when the capture ends (success, cancel, time-out or failure) the store is cleared (`clear_all_browsing_data`), the window is destroyed and its directory removed; leftovers (a crash, a file still held) are removed at the next capture.
- **RF3 — Navigation allow-list.** The window loads only HTTPS pages on the default port, without user info, to the descriptor's `hosts`, the capture's `login_hosts` (exact) and `login_suffixes` (a strict subdomain), plus `about:blank`/`about:srcdoc` frames (`SessionDescriptor::capture_allows`). Everything else is refused; `window.open` never opens a webview (an HTTPS target opens in the user's browser); downloads are refused.
  - RF3.1 (evidence, Xiaomi, 2026-10-27, unauthenticated requests only): the console bundle sends a signed-out user to `/api/v1/genLoginUrl?currentPath=…`; that endpoint answers `302` to `https://account.xiaomi.com/pass/serviceLogin?callback=https://platform.xiaomimimo.com/sts?…&sid=api-platform`; that page moves to `account.xiaomi.com/fe/service/login` and loads its security check from `static-verify.sec.xiaomi.com`. The capture therefore allows `platform.xiaomimimo.com`, `account.xiaomi.com`, `*.account.xiaomi.com` (regional sign-in hosts) and `*.sec.xiaomi.com` (security check frames). `crates/cuotai-core/src/oauth/reference_xiaomi.rs` only documents the platform's `/authorize` key flow on `platform.xiaomimimo.com` and names no identity provider host.
  - RF3.2: static assets (scripts, images, fonts from CDNs such as `cdn.web-global.fds.api.mi-img.com`) are sub-resources, not navigations, and are not affected.
- **RF4 — Read and reduce in Rust.** Rust polls the window's store every second (`Webview::cookies`, on an async task, never the main thread) and keeps, for the host of `cookie_url`, only the descriptor's required and optional cookie names that domain-match it (RFC 6265: the host or a parent domain). A name present several times keeps the most specific cookie (longest domain, then path). The capture completes when every required cookie has a non-empty value.
  - RF4.1: the whole private store is read and matched by Cuotai because WKWebView's `cookies_for_url` keeps only cookies whose domain equals the host exactly and would drop a parent-domain cookie such as `.xiaomimimo.com`.
  - RF4.2: the reduced `name=value; …` list goes back to the account dialog only, into the slot's paste box, which runs the existing preview (`preview_session`) and test (`test_session`); saving keeps SES-02 (test before save) and the vault path of PRD 0016.
- **RF5 — No script, no IPC for the remote page.** Nothing is injected into provider pages (no initialisation script, no `eval`). The capture window's label (`session-capture`) is in no capability and no capability has `remote` URLs, so Tauri rejects every IPC call from its pages (custom commands included: Tauri 2.12 checks the ACL for any non-local origin). A unit test fails if a capability lists the label, a wildcard window, `webviews` or `remote`.
- **RF6 — Time-out and cancel.**
  - RF6.1: without a finished sign-in after 10 minutes the window closes and the dialog says so; the paste box stays.
  - RF6.2: closing the window, closing the dialog or changing the provider cancels; the dialog says the window was closed before the session was captured.
  - RF6.3: a sign-in finished on the last poll is still captured; a cancel wins over a ready store.
- **RF7 — Platforms and fallback.**
  - RF7.1: macOS (WKWebView), Windows (WebView2) and Linux (WebKitGTK) are supported through Tauri 2.12.5 / wry 0.57.0.
  - RF7.2: when the platform cannot read its store (`cookies` returns an error) or the window cannot be created, the capture ends as *unavailable* and the dialog points to the paste box below. The browser demo always answers *unavailable*.
- **RF8 — Texts.** Every new text exists in the 14 desktop catalogs (`dialog.session.capture*`); pt-BR reads "Entrar e capturar sessão".

## 4. Non-functional requirements

- **RNF1 — Secrets.** Cookie values are never logged, never written to disk by Cuotai, never in an error, a report or the TOML; the backend keeps them only until the outcome is returned. Names outside the descriptor are dropped before leaving Rust.
- **RNF2 — No automation of the sign-in.** Cuotai never types, fills or submits anything in the provider's pages; password, 2FA and captcha are the user's.
- **RNF3 — Read-only.** The capture sends no request of its own to the provider; only the user's own browsing in the window and the existing read-only test query do.
- **RNF4 — Tests.** Rust unit tests cover the reduction, the domain match, the allow-list, the state machine (success, cancel, time-out, unavailable), single capture and the capability guard; `scripts/test-session-capture-ui.mjs` covers the catalogs, the demo fallback and the dialog wiring. A live sign-in cannot be tested automatically.
- **RNF5 — TUI unaffected.** The core only gains declarative metadata; the TUI and CLI keep their paste flow unchanged.

## 5. Platform notes (HttpOnly and store access)

Research on wry 0.57.0 (the version Tauri 2.12.5 pins here) and Tauri's documentation ("returns all cookies … including HTTP-only and secure cookies"):

| Platform | Store read | HttpOnly | Caveats |
|---|---|---|---|
| macOS | `WKHTTPCookieStore.getAllCookies` of the webview's non-persistent data store | Included | `cookies_for_url` matches the domain exactly (RF4.1), hence `cookies()` plus Cuotai's own match. `data_directory` does not exist; `incognito` gives the non-persistent store. |
| Windows | `ICoreWebView2CookieManager.GetCookies` | Included | Deadlocks if called from a synchronous command or the main thread (wry issue 583); Cuotai calls it from an async task. `incognito` needs WebView2 ≥ 101.0.1210.39; the fresh data directory isolates older runtimes. `clear_all_browsing_data` needs a runtime with `ICoreWebView2Profile2`; its failure is ignored because the directory is removed anyway. |
| Linux | `WebKitCookieManager.get_all_cookies` of the webview's data manager | Included | wry spins the GTK main loop until the answer arrives; if the webview had no data manager the read would not return (not observed with WebKitGTK 4.1). `clear_all_browsing_data` clears the whole context of the data directory, which is the capture's own. |

None of the three platforms hides HttpOnly cookies from the native store API, so `api-platform_serviceToken` (likely HttpOnly) is readable. Android/iOS are out of scope (the desktop app ships no mobile build).

## 6. Non-goals

- Capturing sessions for other descriptors (Alibaba consoles, the MiMo Desktop account session, 9router SSO): each needs its own evidence of the sign-in hosts first. The MiMo Desktop session is issued to the desktop app, not to a web page, and cannot be captured this way.
- Refreshing a session automatically when it expires; the user captures again.
- Any capture in the TUI.

## 7. UI contract

- `form_schema(provider).sessions[]` gains `capture: bool`.
- `capture_session({ provider, slot })` resolves with `{ "status": "captured", "paste": "name=value; …" }`, `{ "status": "cancelled" }`, `{ "status": "timed_out" }` or `{ "status": "unavailable" }`; it rejects with `not_capturable`, `busy` or `unavailable`.
- `cancel_session_capture()` cancels a waiting capture (no-op otherwise).

## 8. Acceptance criteria

1. In dev mode, adding or editing a Xiaomi MiMo account shows **Sign in and capture session** only next to the platform session box.
2. The click opens a window at the plan page; a signed-out user lands on `account.xiaomi.com` and can sign in, including 2FA and captcha.
3. After sign-in the window closes by itself, the paste box shows the kept names (`api-platform_serviceToken, userId`) and the test runs.
4. Closing the window or the dialog early reports the cancel; waiting 10 minutes reports the time-out.
5. The main window stays signed out of the platform (no shared cookies), and a second capture asks to sign in again.

## 9. Decision log

| Date | Decision |
|---|---|
| 2026-10-27 | Owner: capture in a Cuotai-owned window, user signs in; start with the Xiaomi platform session at `/console/plan-manage`; generic per descriptor. |
| 2026-10-27 | Agent: the captured list is returned to the dialog's paste box (the existing PRD 0016 path) rather than staged in Rust, so preview, test, save and wipe-on-close stay one code path. |
| 2026-10-27 | Agent: the allow-list includes `*.account.xiaomi.com` and `*.sec.xiaomi.com` on the evidence of RF3.1; off-list top-level pages are refused rather than opened externally, because the same callback also sees frame navigations on macOS and Linux. |
| 2026-10-27 | Owner: the Xiaomi sign-in page offers Facebook and Google. The allow-list adds the exact hosts `www.facebook.com`, `m.facebook.com` and `accounts.google.com` (no wildcard). Google may refuse sign-in inside an embedded window; then the paste box remains the path. |
