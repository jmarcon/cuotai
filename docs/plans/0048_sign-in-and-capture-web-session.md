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
| RF1–RF8, RNF1–RNF5 | 0.81.0 | Delivered on `feat/webview-session-capture`: core capture metadata and allow-list, the desktop `session_capture` module (window, private store, polling, reduction, state machine), the account dialog button and the texts in the 14 desktop catalogs. Enabled for the Xiaomi MiMo platform session only. A live sign-in was not run by the implementing agent (no real account may be used); the owner's manual test is pending. |
| RF1.1, RF3.3, RF4.3 (Qwen Cloud) | 0.81.0 | The Qwen Cloud Token Plan session (`alibaba_token_plan`, `session_env`) declares a capture: it opens the Token Plan usage page, which is also the descriptor's open link; allow-list from RF3.3; completion on `login_aliyunid_pk` (RF4.3). Live sign-in pending the owner's manual test. |
| RF1.1, RF3.4, RF3.5, RF4.3 (Alibaba Coding Plan) | 0.81.0 | The Alibaba Coding Plan session (`alibaba_coding_plan`, `session_env`) declares a capture per region (`SessionDescriptor::regions`): the international Model Studio console by default and the mainland Bailian console for `region = "cn"`, each with its own start page, cookie address, exact sign-in hosts (RF3.4) and open link; completion on `login_aliyunid_pk`. RF3.5 records why the Token Plan keeps its Qwen Cloud capture. Live sign-in pending the owner's manual test. |

## 1. Owner request (2026-10-27)

In the desktop app the user should no longer open the developer tools and copy cookies. Next to a web-session paste box, **Sign in and capture session** ("Entrar e capturar sessão") opens a Cuotai-owned window at the provider's page; the user signs in there (password, 2FA and captcha typed by the user); Cuotai reads the session cookies from that window's own cookie store, keeps only the descriptor's cookies, stages them through the existing paste path, runs the existing test and closes the window. Start with the Xiaomi MiMo platform session (`https://platform.xiaomimimo.com/console/plan-manage`, confirmed by the owner), designed per descriptor.

## 2. Problem

PRD 0016 RF2.1 asks the user to sign in in a browser, open the developer tools, find a request and copy its `Cookie` header. It works, but it is the hardest step of adding a Xiaomi MiMo platform account, it exposes the whole cookie jar to the clipboard, and many users stop there.

## 3. Requirements (GUI user)

- **RF1 — Consent by click.** The window opens only when the user clicks **Sign in and capture session** next to a paste box whose descriptor declares a capture (`form_schema` `sessions[].capture`). A hint under the button says what happens: a private Cuotai window, the user types the credentials, only the session's cookies are kept, the window's data is erased. Nothing opens at dialog start, on provider change or after a failed test.
  - RF1.1: the capture is declared per descriptor by the core (`SessionDescriptor::capture: Option<&CaptureSpec>` with `start_url`, `cookie_url`, `login_hosts`, `login_suffixes`). Today `XIAOMI_PLATFORM`, `ALIBABA_TOKEN_PLAN` (Qwen Cloud) and `ALIBABA_CODING_PLAN` declare one. A descriptor may also declare `regions` (`SessionRegion { value, capture, open_url }`): the account's `params.region`, sent by the dialog from its `region` choice, selects that region's capture and "Open {site}" page; any other value keeps the default (RF3.4).
  - RF1.2: one capture at a time; a second click while a window is open focuses it and says so.
- **RF2 — Isolation.** The window has a dedicated, non-persistent cookie store that the main app webview never shares.
  - RF2.1: `incognito(true)`: macOS uses a non-persistent `WKWebsiteDataStore`; Windows uses an InPrivate WebView2 profile; WebKitGTK an ephemeral context.
  - RF2.2: on Windows and Linux the window also gets a fresh data directory under the system temporary folder (`cuotai-session-capture-<pid>-<nanos>-<n>`), so it is apart from the main window even where `incognito` does nothing (WebView2 older than 101.0.1210.39).
  - RF2.3: when the capture ends (success, cancel, time-out or failure) the store is cleared (`clear_all_browsing_data`), the window is destroyed and its directory removed; leftovers (a crash, a file still held) are removed at the next capture.
- **RF3 — Navigation allow-list.** The window loads only HTTPS pages on the default port, without user info, to the descriptor's `hosts`, the capture's `login_hosts` (exact) and `login_suffixes` (a strict subdomain), plus `about:blank`/`about:srcdoc` frames (`SessionDescriptor::capture_allows`). Everything else is refused; `window.open` never opens a webview (an HTTPS target opens in the user's browser); downloads are refused.
  - RF3.1 (evidence, Xiaomi, 2026-10-27, unauthenticated requests only): the console bundle sends a signed-out user to `/api/v1/genLoginUrl?currentPath=…`; that endpoint answers `302` to `https://account.xiaomi.com/pass/serviceLogin?callback=https://platform.xiaomimimo.com/sts?…&sid=api-platform`; that page moves to `account.xiaomi.com/fe/service/login` and loads its security check from `static-verify.sec.xiaomi.com`. The capture therefore allows `platform.xiaomimimo.com`, `account.xiaomi.com`, `*.account.xiaomi.com` (regional sign-in hosts) and `*.sec.xiaomi.com` (security check frames). `crates/cuotai-core/src/oauth/reference_xiaomi.rs` only documents the platform's `/authorize` key flow on `platform.xiaomimimo.com` and names no identity provider host.
  - RF3.3 (evidence, Qwen Cloud, 2026-10-27, unauthenticated requests only): the usage page `https://home.qwencloud.com/analytics/token-plan/individual?region=ap-southeast-1` answers `200` signed out; its console bundle (`q.alcasset.com/code/qwen-cloud/console-home/1.1.60/assets/index.js`) builds the sign-in link `https://account.qwencloud.com/sso/OIDCAuth?oauth_callback=…`, which answers `302` to `https://account.alibabacloud.com/sso/login.htm?response_type=code&client_id=qwencloud&redirect_uri=https://account.qwencloud.com/sso/ssoLogin…`. That page (`g.alicdn.com/aliyunfe-account/alicloud-sso-login/0.0.30`) offers e-mail sign-in on the same host plus Google and GitHub buttons (`google_qwen`, `github_qwen`), which go to `account.alibabacloud.com/login/third_party_bind_login.htm`; it script-redirects to `https://passport.alibabacloud.com/sns_oauth.htm?…`, which answers `302` to `https://accounts.google.com/o/oauth2/v2/auth?…` or `https://github.com/login/oauth/authorize?…`, both with a `redirect_uri` on `account.qwencloud.com/sso/`. The security check (`AWSC`/`baxia`) is a script from `g.alicdn.com` that names `passport.alibabacloud.com`. The capture therefore allows, exactly, `account.qwencloud.com`, `account.alibabacloud.com`, `passport.alibabacloud.com`, `accounts.google.com` and `github.com`, with no suffix (no regional sign-in host was seen). `signin.alibabacloud.com` and the mainland `account.qianwenai.com` were not part of this flow and stay blocked. The same bundle shows the page's own script reading `login_aliyunid`, `login_aliyunid_pk` and `login_current_pk` from its cookies; `tool/user/info.json` answers `ConsoleNeedLogin` without them.
  - RF3.4 (evidence, Alibaba Coding Plan, 2026-10-27, unauthenticated requests only): both consoles serve the same bundle (`g.alicdn.com/bailian-fe/bailian-site-fe/0.0.169`); its `fetcher.js` handles a signed-out user (`tool/user/info.json` answers `ConsoleNeedLogin` on both) by sending an `alibabacloud` host to `https://account.alibabacloud.com/login/login.htm?oauth_callback=…&clearRedirectCookie=1` and an `aliyun.com` host to `https://account.aliyun.com/login/login.htm?oauth_callback=…` (or to an in-page dialog built by the script `g.alicdn.com/dawn/assets-loader/scripts/fast-login.js`). Since the adapter fixes the region per account (`params.region`, default `intl`), the capture is chosen the same way:
    - **International** (default): starts at the adapter's dashboard `https://modelstudio.console.alibabacloud.com/ap-southeast-1/?tab=coding-plan#/efm/coding_plan` (now also the default open link) and reads cookies for `modelstudio.console.alibabacloud.com`. The sign-in page embeds `passport.alibabacloud.com/mini_login.htm?appName=intl-aliyun`; its Google and GitHub buttons (`login.js` of `g.alicdn.com/aliyun/account/1.8.23`) go to `account.alibabacloud.com/login/third_party_bind_login.htm?type=google|github`, which links `passport.alibabacloud.com/sns_oauth.htm?…`, answering `302` to `accounts.google.com/o/oauth2/v2/auth` or `github.com/login/oauth/authorize`. Allowed exactly: `account.alibabacloud.com`, `passport.alibabacloud.com`, `accounts.google.com`, `github.com`. `signin.alibabacloud.com` (RAM sign-in) answers `200` but neither the console bundle nor the sign-in page links it, so it stays blocked.
    - **Mainland** (`region = "cn"`): starts at `https://bailian.console.aliyun.com/cn-beijing/?tab=model#/efm/coding_plan` (the former open link, kept for `cn`) and reads cookies for `bailian.console.aliyun.com`. The sign-in page (`g.alicdn.com/dawn/fe-account-login/1.0.88`) embeds `passport.aliyun.com/mini_login.htm` and links the RAM sign-in `signin.aliyun.com/login.htm`; its Alipay, Taobao and DingTalk buttons go through `account.aliyun.com/login/third_party_bind_login.htm?type=…`: Alipay to `auth.alipay.com/login/index.htm` (whose embedder frames `passport.alipay.com/littleLogin/littleLogin.htm`), Taobao to `passport.aliyun.com/sns_oauth.htm`, answering `302` to `havanalogin.taobao.com/unify_oauth.htm`, DingTalk to `oapi.dingtalk.com/connect/oauth2/sns_authorize`, answering `302` to `h5.dingtalk.com/liveeasylogin/`. Allowed exactly: `account.aliyun.com`, `passport.aliyun.com`, `signin.aliyun.com`, `auth.alipay.com`, `passport.alipay.com`, `havanalogin.taobao.com`, `oapi.dingtalk.com`, `h5.dingtalk.com`.
    - Neither region allows the other's hosts, and no suffix is allowed. Inferred, not observed (no account was used): the pages each social provider shows after its first step (for example an Alipay QR or captcha page on another `alipay.com` host, or a Taobao page beyond `havanalogin.taobao.com`) may need further hosts; then the window refuses that page and the paste box remains the path. Captchas are scripts and frames from `g.alicdn.com` (AWSC/baxia), not navigations.
  - RF3.5 (Token Plan on the Model Studio console, owner question, 2026-10-27): the international console's own Token Plan page (`/ap-southeast-1/subscription/token-plan/personal`, `200` signed out) is a micro-frontend (`app-tokenplan`) whose shared module `tokenPlanApiKeyStatus.js` names the same API as the adapter (`zeldaHttp.apikeyMgr./tokenplan/personal/api/v2`, commodity `sfm_tokenplansolo_public_intl`, action `IntlBroadScopeAspnGateway`), so the subscription is the same one Qwen Cloud shows. The adapter, however, calls it through the Qwen Cloud gateway (`cs-data.qwencloud.com`, `consoleSite = QWENCLOUD`, `feURL` on `home.qwencloud.com`), and the Model Studio request shape (gateway host, `consoleSite`, `feURL`) for this call could not be confirmed without a signed-in session. The Token Plan descriptor therefore keeps its Qwen Cloud capture and open link and does not add Model Studio hosts (they would receive the session without the adapter using them). Qwen Cloud signs in through the same `account.alibabacloud.com` SSO (RF3.3, `client_id=qwencloud`), so the same international account works there. Reading the Token Plan through the Model Studio session needs an adapter change verified on a real session; it is left for a later change.
  - RF3.2: static assets (scripts, images, fonts from CDNs such as `cdn.web-global.fds.api.mi-img.com`) are sub-resources, not navigations, and are not affected.
- **RF4 — Read and reduce in Rust.** Rust polls the window's store every second (`Webview::cookies`, on an async task, never the main thread) and keeps, for the host of `cookie_url`, only the descriptor's required and optional cookie names that domain-match it (RFC 6265: the host or a parent domain). A name present several times keeps the most specific cookie (longest domain, then path). The capture completes when every required cookie has a non-empty value.
  - RF4.1: the whole private store is read and matched by Cuotai because WKWebView's `cookies_for_url` keeps only cookies whose domain equals the host exactly and would drop a parent-domain cookie such as `.xiaomimimo.com`.
  - RF4.3: a descriptor whose cookies are all optional (the Qwen Cloud session) would otherwise complete on the first anonymous cookie (`cna` exists before sign-in). Its capture declares `done_when` cookies (`CaptureSpec::done_when`) that must also be non-empty: for Qwen Cloud, `login_aliyunid_pk`, the account id the Alibaba sign-in sets with `login_aliyunid_ticket` (which the adapter sends to `home.qwencloud.com/tool/user/info.json` to obtain `sec_token`). Cookies are read for `home.qwencloud.com`; parent-domain `.qwencloud.com` cookies match it and are the ones the adapter also sends to `cs-data.qwencloud.com`; `.alibabacloud.com` cookies never match.
  - RF4.4: the Coding Plan capture also completes on `login_aliyunid_pk` (both regions). Cookies are read for the region's console host, so the international capture keeps `.alibabacloud.com` cookies and the mainland one `.aliyun.com` cookies; the other domain's cookies never match. The adapter then obtains `sec_token` from the dashboard with that cookie list.
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
- **RF9 — The session as an alternative (owner, 2026-10-27).** A provider with a session slot never shows "there is no credential to enter" (`dialog.credNone`) beside the session box; when the provider also has a sign-in mode or a credential, the box opens with one line presenting the web session as the alternative (`dialog.session.orIntro`, pt-BR "Ou use a sessão web de {provider}:"). The `bl` mode of the Token Plan reads as a CLI profile in every core catalog (en "Bailian CLI profile (bl)", pt-BR "Perfil do CLI bl (Bailian)").
- **RF10 — Missing CLI (owner, 2026-10-27).** When the CLI a sign-in mode runs is not found on this device's `PATH` (`bl` for the Token Plan), the core's `LoginMode.install` names the official install page (Alibaba Cloud Model Studio, "Installation & Authentication") with a localized text (`login.install.bl`, pt-BR "Instalar o CLI bl"); the desktop links it beside the option through the exact-match `open_external` allow-list, the TUI prints the address as text.

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
| 2026-10-27 | Owner: add the capture to the Qwen Cloud Token Plan session, starting at the Token Plan usage page, which also replaces the billing page as its open link. Agent: exact sign-in hosts from RF3.3 (no wildcard), and completion on `login_aliyunid_pk` (RF4.3), since the session has no required cookie and a missing ticket is then reported by the adapter's own test. |
| 2026-10-27 | Owner: add the capture to the Alibaba Coding Plan session. Agent: one capture per region selected by the account's `region` choice (the adapter's own rule; the dialog sends it and the core picks), exact sign-in hosts per region from RF3.4 with no wildcard, completion on `login_aliyunid_pk`; the default open link becomes the international dashboard (the adapter's default region) and `cn` keeps the mainland one. The Token Plan keeps its Qwen Cloud capture (RF3.5). |
