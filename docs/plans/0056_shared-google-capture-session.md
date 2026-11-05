# PRD 0056 — Shared Google sign-in for session captures

| Field | Value |
|---|---|
| ID | 0056 |
| Status | Awaiting owner verification |
| Owner | `cuotai-core` (`user_session::CaptureSpec::shared`, `SharedSignIn`), desktop backend (`session_capture` shared window, `forget_shared_capture` command, `form_schema` `sessions[].capture_shared`), desktop UI (`account-dialog.js`, `lib.js`, desktop catalogs) |
| Consumers | Desktop users adding or editing an account whose console sign-in offers "Sign in with Google" |
| Depends on | PRD 0048 (sign in and capture a web session) |
| Related | PRD 0049 RF7 (dev.meta.ai capture), PRD 0050 (webview trust) |
| Date | 2026-11-04 |

> **Note on terminology:** "RF" (from the Portuguese *requisito funcional*) means functional requirement; "RNF" a non-functional one. All requirements address the **GUI user**; the TUI is unaffected (RNF5).

## Implementation status (2026-11-04)

| Item | Version | Delivery |
|---|---|---|
| RF1–RF9, RNF1–RNF5 | 0.83.0 | Delivered on `feat/shared-google-capture-session`: the core's `SharedSignIn` declaration on the four captures that offer Google, the desktop's shared window (state machine, parking with cookie reduction, idle limit, exit, forget), in-place Google popups, the dialog hints and "Forget the Google sign-in" in the 14 desktop catalogs, and the docs. The lifecycle was verified with unit tests and Tauri's mock runtime only (no real webview, no real account, no real cookie store); the owner's live check on macOS (§9) is pending. |

## 1. Owner request (2026-11-04)

The PRD 0048 capture opens a fresh private window for every capture, so a user with several consoles behind one Google account types the Google password and second factor again for each. The owner asked for one shared, hidden, kept-alive capture window for the providers whose console login offers "Sign in with Google", after a spike on the owner's Mac showed that this works.

## 2. Evidence

Spike `spike/google-session-sharing` (`spikes/google-session-sharing/`, local branch, not pushed; `RESULTS.md` and `README.md` there). Owner runs on macOS 27.2, 2026-11-04, real Google account, a capture-like window (`incognito`, popups denied, no IPC capability, Tauri 2.12.5 / wry 0.57.0):

- One incognito window kept alive and hidden, signed in to Google once (identifier and a challenge were asked; Google did **not** reject the embedded browser).
- AI Studio, opened next in the same window, was signed in with nothing asked.
- Qwen Cloud's "Sign in with Google", in the same window, showed only Google's account chooser (no password, no code).
- After 10 minutes hidden, the page was kept (no reload) and both sessions survived.
- Known limits: a named persistent WebKit store crashed WebKit (SIGSEGV in `removeDataStoreWithIdentifierImpl`) in a process without a webview on macOS 27 with Tauri 2.12.5; the Google Account intro page's sign-in button opens a new window, which the capture denies (the spike then started on the `accounts.google.com` sign-in page instead); passkeys are not available to an embedded WKWebView for `google.com` (§8).
- Passkey probe (`cargo run -- passkey-probe`, added for this PRD; run by the implementing agent on the owner's Mac, macOS 27.2, local loopback page, no account): `isSecureContext` true, `PublicKeyCredential` defined, `isUserVerifyingPlatformAuthenticatorAvailable()` **false**, `navigator.credentials.get` with a dummy local challenge rejected at once with `NotAllowedError`. Booleans and error names only were recorded.

Not tested by the spike: relaunch, the persistent store beyond the crash, idle beyond 10 minutes, Windows, Linux, the Xiaomi, Alibaba international and Meta targets.

## 3. Scope: which captures share the window

The captures that list Google's sign-in host (`accounts.google.com`) in their `login_hosts` (PRD 0048 RF3 evidence) share it:

| Provider | Session slot | Capture | Google sign-in evidence |
|---|---|---|---|
| Xiaomi MiMo | `platform_session_env` | `platform.xiaomimimo.com` console | PRD 0048 decision log (owner, 2026-10-27): the Xiaomi sign-in page offers Google and Facebook |
| Alibaba Coding Plan | `session_env`, default (international) region | Model Studio console | PRD 0048 RF3.4: `passport.alibabacloud.com/sns_oauth.htm` → `accounts.google.com` |
| Alibaba Token Plan (Qwen Cloud) | `session_env` | `home.qwencloud.com` | PRD 0048 RF3.3 and the spike (account chooser only) |
| Meta | `session_env` | `dev.meta.ai` console | PRD 0049 RF7 (owner, 2026-10-27): `auth.meta.com/login/oidc/google/` → `accounts.google.com` |

The mainland Coding Plan capture (`region = "cn"`) has no Google sign-in (Alipay, Taobao, DingTalk) and keeps its private window. No other descriptor in `user_session.rs` declares a capture. GitHub is also offered by three of them, but GitHub was not tested and is not shared (§10).

## 4. Requirements (GUI user)

- **RF1 — Opt-in declared by the core.**
  - RF1.1: `CaptureSpec::shared: Option<SharedSignIn>` declares the sign-in a capture shares; `SharedSignIn::Google` is the only value. A core test fails when a capture lists a sign-in's hosts (`SharedSignIn::sign_in_hosts`, `accounts.google.com`) without declaring it, or the reverse, and pins the four captures of §3.
  - RF1.2: the desktop routes a capture to the shared window when its chosen capture (region included, PRD 0048 RF3.4) declares `shared`, otherwise to the PRD 0048 private window (`session_capture::route`). A provider asks for it only through the core; the desktop has no provider list.
  - RF1.3: `form_schema(provider).sessions[]` gains `capture_shared` (`"google"` or `null`) for the default capture, and each `region_open_urls[]` entry gains the same field for its region's capture, so the dialog's hint follows the `region` choice.
- **RF2 — One window, app session only.**
  - RF2.1: the first shared capture creates the window `session-capture-google` (non-persistent store: `incognito(true)` on every platform, plus a fresh data directory under the system temporary folder on Windows and Linux, as PRD 0048 RF2.2). The next shared capture reuses it: the window is shown again, retitled and sent to that capture's start page.
  - RF2.2: after a capture that ends captured, timed out, or cancelled from the dialog (closing it, changing provider, Cancel), the window is **parked**: sent to `about:blank`, hidden, and reduced to the shared sign-in's cookies (RF3). Closing the window itself, or a store that cannot be read (PRD 0048 RF7.2), destroys it: closing the window means "forget".
  - RF2.3: nothing is persisted. The window is destroyed (store cleared, data directory removed) after **30 minutes** parked without a capture (`SHARED_IDLE`), when the user forgets it (RF2.4) or when the app quits (its non-persistent store ends with the process; the directory is removed on exit, and any leftover by the next capture's sweep). A named persistent store is not used (§2: WebKit crash, and a relaunch-surviving Google session was not asked for).
  - RF2.4: next to a shared capture, **Forget the Google sign-in** destroys a parked window and says whether there was one; while a capture runs in it, it answers that a sign-in window is open.
  - RF2.5: still one capture at a time across both windows (PRD 0048 RF1.2); a second click focuses the window in use.
- **RF3 — Between captures, only Google's cookies.** Parking deletes every cookie whose domain is not `google.com` or one of its subdomains (`SharedSignIn::keeps_cookie`), after the page left for `about:blank` and a short settle, then reads the store again. If a cookie cannot be deleted, the store cannot be read, or a non-Google cookie is still there after two passes, the window is destroyed instead (fail closed). So a provider's session never waits in the hidden window, and capturing a second account of the same provider does not end at once with the first account's cookies; Google's account chooser still lets the user pick or add an account.
- **RF4 — Allow-list of the capture in progress.** The shared window's navigation hook reads the capture in progress at each call: it loads only what that capture's `capture_allows_with` allows (PRD 0048 RF3); a parked window loads only `about:blank`. Downloads are refused, as before.
- **RF5 — Popups.** A `window.open` to an HTTPS page on `accounts.google.com` (default port, no user info) that the capture in progress allows opens **in the same window** (the new window is denied and the shared window navigates there, through the same allow-list). Every other HTTPS popup opens in the user's browser and anything else is dropped, as in PRD 0048. A flow that needs its opener (a popup that posts its result back with `postMessage`) still cannot finish; the four providers use redirects (§3 evidence), and the paste box remains the fallback.
- **RF6 — What the user sees.** Under **Sign in and capture session**, a shared capture shows, instead of the PRD 0048 hint: that the window is shared by the sites that offer "Sign in with Google", that the user types password, 2FA and captcha there, that once the session is captured the window hides and keeps only the Google sign-in in memory so the next site asks only for the account, that it is erased on quit, after 30 minutes unused or with **Forget the Google sign-in**, and that each site's own cookies are erased when its capture ends. A second line states the passkey limit (RF9.3). The window's title is the provider's site. The texts exist in the 14 desktop catalogs (`dialog.session.captureSharedHint`, `capturePasskeyHint`, `captureForget`, `captureForgotten`, `captureNothingToForget`).
- **RF7 — Memory cost.** One WebKit web-content process (macOS) or WebView2/WebKitGTK renderer stays alive while parked, on `about:blank`. Not measured; expected order of magnitude: a few tens of MB parked, 100–300 MB while a console page is shown (the PRD 0048 window already costs the latter during a capture). The 30-minute idle limit and **Forget** bound it; nothing is kept across launches.
- **RF8 — Platforms.** Same code path on macOS (WKWebView non-persistent data store), Windows (WebView2 InPrivate profile plus a fresh data directory) and Linux (WebKitGTK ephemeral context plus a fresh data directory). Only macOS was exercised by the spike; whether Google accepts the embedded sign-in on WebView2 and WebKitGTK, and whether a hidden window keeps its page there, is untested (§9).
- **RF9 — Passkeys.**
  - RF9.1 (macOS, verified): the capture window cannot use passkeys. The probe of §2 shows the WebAuthn API present but no user-verifying platform authenticator and an immediate `NotAllowedError`; Apple offers passkeys in a `WKWebView` only to apps with the web-browser entitlement or an associated domain for the relying party, and Cuotai cannot have `google.com` as an associated domain. Without a user gesture the `get` rejection alone is not decisive; the authenticator answer and Apple's rule are.
  - RF9.2 (the split): OAuth flows that open the **system browser** already support passkeys, since the user's own browser does the Google sign-in: Gemini API and Vertex AI (`oauth/flows.rs`), Gemini CLI (`oauth/reference_browser.rs`) and Antigravity (`oauth/reference_antigravity.rs`) all send the user to `accounts.google.com` through `oauth::login::open_browser`. `ASWebAuthenticationSession` would add an in-app sheet with Safari's cookies and passkeys, but only for flows that end on a callback URL; it never exposes cookies, so it cannot serve the console captures, and for the OAuth flows the system browser already offers passkeys. It is not adopted. Console captures (this PRD and PRD 0048) read cookies from Cuotai's own webview and cannot use passkeys.
  - RF9.3: the dialog tells the user, under a shared capture, that passkeys do not work in this window and to choose Google's "Try another way" (password, code or phone). An account with only a passkey uses the paste box (PRD 0016).
  - RF9.4 (other platforms, untested): WebView2 may offer WebAuthn through Windows Hello; WebKitGTK has no platform authenticator. The hint is shown on every platform.

## 5. Non-functional requirements

- **RNF1 — Secrets.** As PRD 0048 RNF1: cookie values are never logged, written to disk by Cuotai, put in an error, a report or the TOML; only the reduced paste of the capture in progress leaves Rust, to the dialog. The parked window holds Google's cookies in the webview's own non-persistent store only; Cuotai reads them only to decide what to delete and keeps no copy. Debug builds still log cookie names and domains, never values.
- **RNF2 — No automation.** Cuotai never types, fills, submits or clicks in Google's or the provider's pages; reusing the window only navigates to the capture's start page.
- **RNF3 — Read-only.** No request of Cuotai's own reaches Google or the provider; parking only deletes cookies in the local store.
- **RNF4 — Tests.** Core: the opt-in rule and list, the kept-cookie rule, the in-place popup rule. Desktop: the routing, the navigation of the capture in progress, the popup decision, the cookies forgotten on parking, the state machine (create, park, reuse, idle, forget, close, unavailable) and a mock-runtime run (cancel parks, the next capture reuses, forget destroys, exit clears). UI: `scripts/test-session-capture-ui.mjs` covers the 14 catalogs, the region-dependent hint, the demo answer and the dialog wiring. Every test uses fake stores or Tauri's mock runtime; no live sign-in runs automatically.
- **RNF5 — TUI unaffected.** The core only gains declarative metadata.

## 6. Non-goals

- Keeping the Google sign-in across app launches (a named persistent store; see RF2.3).
- Sharing a GitHub, Facebook or Alibaba sign-in between captures.
- A "Sign in to Google first" button, or starting the window on Google's own sign-in page.
- Passkeys in the capture window (RF9).
- Any capture in the TUI.

## 7. UI contract

- `form_schema(provider).sessions[]` gains `capture_shared: "google" | null`; `region_open_urls[]` entries gain the same field.
- `capture_session` is unchanged (PRD 0048 §7).
- `forget_shared_capture()` resolves `true` when a parked shared window was destroyed, `false` without one, and rejects with `busy` while a capture runs in it.

## 8. Acceptance criteria

1. In dev mode, the account dialog of Qwen Cloud (Token Plan), the Xiaomi MiMo platform session, the international Alibaba Coding Plan and Meta shows the shared hint, the passkey line and **Forget the Google sign-in**; the mainland Coding Plan (`region = cn`) shows the PRD 0048 hint.
2. Capturing Qwen Cloud with "Sign in with Google" asks Google's identifier and challenge once; the window hides after the capture.
3. Capturing Meta (or the Xiaomi platform, or the international Coding Plan) within 30 minutes reopens the same window; "Sign in with Google" shows only the account chooser.
4. Capturing Qwen Cloud again starts signed out of Qwen Cloud (its cookies were erased) and Google offers the account chooser.
5. **Forget the Google sign-in**, closing the window, 30 minutes unused or quitting Cuotai each make the next capture ask Google's identifier again.
6. The main window stays signed out of every provider and of Google.

## 9. Owner verification

On the packaged or dev macOS app, with the owner's own accounts: acceptance criteria 1–6, plus Activity Monitor's memory for the parked window. Windows and Linux when available: whether Google accepts the embedded sign-in, whether the parked window keeps the sign-in, and whether WebView2 offers Windows Hello passkeys (RF9.4). The passkey probe can be rerun with `cargo run -- passkey-probe` in `spikes/google-session-sharing` (spike branch).

## 10. Decision log

| Date | Decision |
|---|---|
| 2026-11-04 | Owner: one shared, hidden, kept-alive incognito capture window for the providers whose console sign-in offers Google, after the spike of §2. |
| 2026-11-04 | Agent: the opt-in is the core's `CaptureSpec::shared`, derived from the evidence already behind each capture's `login_hosts` and pinned by a test; the desktop holds no provider list. |
| 2026-11-04 | Agent: between captures the window keeps only Google's cookies (RF3), so a provider session never waits hidden and a second account of the same provider is not captured from the first one's cookies; a failed deletion destroys the window. |
| 2026-11-04 | Agent: idle limit 30 minutes (the spike showed 10 minutes hidden without loss; longer was not tested), destroyed on quit, no persistent store (the WebKit crash of §2 and no request to survive a relaunch). |
| 2026-11-04 | Agent: closing the window forgets the sign-in; a dialog cancel parks it. **Forget the Google sign-in** gives the user a way to end a hidden window before the idle limit. |
| 2026-11-04 | Agent: Google sign-in popups open in place instead of being denied, limited to HTTPS `accounts.google.com` pages the capture allows; GitHub is not shared because it was not tested. |
| 2026-11-04 | Agent: `ASWebAuthenticationSession` is not adopted: it exposes no cookies (console captures) and the system browser already gives the OAuth flows passkeys (RF9.2). |
