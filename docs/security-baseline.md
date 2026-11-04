# Security baseline

The security properties Cuotai promises, each tied to the test that fails when the property regresses (PRD 0050 RF7). `cargo xtask security-baseline` reads this table: every row needs at least one test carrying its marker comment (`security-invariant: SEC-…`), every marker in the code needs a row here, every `file::test` owner must name a test that exists, and every row declares the platforms its tests run on. The same command checks the desktop Content Security Policy and HTML pages (RF1.4).

Platforms are the CI runners the owning test runs on. A Unix-only test (`#[cfg(unix)]`) lists Linux and macOS; Windows parity of file-permission checks waits for the owner decision in PRD 0050 §13.

## Invariants

| Invariant | Boundary | Fail-closed result | Owning test or command | Platforms |
|---|---|---|---|---|
| `SEC-WEBVIEW` | Desktop webview: the Content Security Policy in `tauri.conf.json` and the three HTML pages | No inline or remote script, no eval, no frame, object, form or base URL, connections only to the app bundle and Tauri IPC; a broken policy or an inline script fails the build checks | `apps/desktop/src-tauri/src/lib.rs::the_webviews_run_under_a_restrictive_content_security_policy`, `xtask/tests/security_baseline.rs::the_csp_fixtures_are_refused`, `cargo xtask security-baseline` | Linux, macOS, Windows |
| `SEC-TRANSPORT` | Credential-bearing URLs (`transport::is_secure`) | Plain HTTP is refused unless the host is loopback | `crates/cuotai-core/src/transport.rs::plain_http_is_secure_only_for_loopback` | Linux, macOS, Windows |
| `SEC-REDIRECT` | Provider and user-session HTTP clients | A 3xx answer is an error and is never followed, so credentials never reach a second host | `crates/cuotai-core/src/user_session_tests.rs::a_redirect_to_another_host_never_carries_the_session`, `crates/cuotai-core/src/providers/open_router.rs::a_redirect_is_rejected_and_never_followed` | Linux, macOS, Windows |
| `SEC-DEV-STORE` | Plaintext SQLite development credential store | A release build refuses it with a fixed error before any filesystem access | `crates/cuotai-core/src/oauth/store.rs::the_development_store_is_refused_in_release_builds` (the release branch runs under `cargo test -p cuotai-core --release --lib oauth::store`) | Linux, macOS, Windows |
| `SEC-DEV-FILE` | Development store file | A shared (hard-linked), symlinked or group/other-readable file is refused | `crates/cuotai-core/src/oauth/store/sqlite_dev.rs::rejects_shared_files_and_symlinks` | Linux, macOS |
| `SEC-PRIVATE-FS` | Private files and directories (`private_fs`) | Directories are `0700`, files `0600`, owned by the current user | `crates/cuotai-core/src/private_fs.rs::a_private_file_is_0600_in_a_0700_directory` | Linux, macOS |
| `SEC-ERRORS` | Local server answers and access log | Neither carries the token, credentials, credential variable names or query parameters | `crates/cuotai-core/src/serve/tests.rs::answers_and_access_log_carry_no_secrets` | Linux, macOS, Windows |
| `SEC-SERVE-ORIGIN` | Local server browser mode (`ServeConfig::allow_browsers`) | No token or no trusted origin refuses to start; only an exact allowed `Origin` is answered, echoed with `Vary: Origin`, never `*`; any other gets `403 origin_not_allowed` with no CORS header | `crates/cuotai-core/src/serve/tests.rs::browser_mode_echoes_only_an_exact_allowed_origin`, `crates/cuotai-core/src/serve/tests.rs::browser_mode_needs_a_token_and_trusted_origins` | Linux, macOS, Windows |

## Recorded exceptions

- **`withGlobalTauri: true` (PRD 0050 RF1.3).** The desktop UI is a set of unbundled ES modules with no package build, and they call `window.__TAURI__.core.invoke` and the event API directly. The global stays until every direct use moves to an official module bridge (not `__TAURI_INTERNALS__`). The CSP above is what keeps a foreign script from reaching that global.
- **`connect-src 'self'`.** The UI reads its bundled catalogs (`locales/*.json`) with `fetch` from the app's own origin. `'self'` is the application bundle, never a network host.
- **`style-src 'unsafe-inline'`.** Components write `style` attributes for bar positions and sizes, and user themes are inserted as style elements. CSP still blocks every remote, `data:` and `blob:` fetch a style could trigger (`img-src` and `font-src` are `'self'`).
