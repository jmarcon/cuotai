//! PRD 0050 RF7.4: one table over the security boundaries. Each row feeds a
//! sentinel URL, body, token and path into a failure of transport, OAuth,
//! credential storage, private files or the local server, and proves none
//! of them reaches the error's `Display`/`Debug`, its `Localized` id,
//! parameters and rendered texts, the app log, or a serialized answer.

use std::sync::Arc;
use std::time::Duration;

use wiremock::matchers::any;
use wiremock::{Mock, MockServer, ResponseTemplate};

use crate::diagnostics::logging::{self, AppKind, Field, Level};
use crate::network::{client, ClientPurpose};
use crate::text::Localized;

const URL_HOST: &str = "sentinel-host.example.com";
const URL_PATH: &str = "sentinel-path-91c2";
const QUERY: &str = "sentinel-query-91c2";
const BODY: &str = "sentinel-body-91c2";
const TOKEN: &str = "sk-sentinel0token0value0000091c2";
const FILE: &str = "sentinel-file-91c2";

const SENTINELS: [&str; 6] = [URL_HOST, URL_PATH, QUERY, BODY, TOKEN, FILE];

/// What one boundary exposes after a failure.
struct Exposed {
    boundary: &'static str,
    texts: Vec<String>,
    notes: Vec<Localized>,
}

impl Exposed {
    fn new(boundary: &'static str) -> Self {
        Self {
            boundary,
            texts: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn text(mut self, text: impl Into<String>) -> Self {
        self.texts.push(text.into());
        self
    }

    fn note(mut self, note: Localized) -> Self {
        self.notes.push(note);
        self
    }

    /// Every rendering of this row: raw texts, and per note its `Display`,
    /// its serialized form (id and every parameter) and two locales.
    fn renderings(&self) -> Vec<String> {
        let mut out = self.texts.clone();
        for note in &self.notes {
            out.push(note.to_string());
            out.push(serde_json::to_string(note).unwrap());
            out.push(note.resolve("en-US"));
            out.push(note.resolve("pt-BR"));
        }
        out
    }
}

fn assert_no_sentinel(row: &Exposed, sentinels: &[&str]) {
    for text in row.renderings() {
        for sentinel in sentinels {
            assert!(
                !text.contains(sentinel),
                "{}: {sentinel:?} leaked into {text:?}",
                row.boundary
            );
        }
    }
}

fn sentinel_url(scheme: &str, host: &str) -> String {
    format!("{scheme}://{host}/{URL_PATH}?token={QUERY}")
}

/// A mock that answers every request with `status` and the sentinel body.
async fn answering(status: u16) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(any())
        .respond_with(
            ResponseTemplate::new(status)
                .insert_header("Location", sentinel_url("https", URL_HOST).as_str())
                .set_body_string(format!(r#"{{"error":"{BODY}","token":"{TOKEN}"}}"#)),
        )
        .mount(&server)
        .await;
    server
}

fn seeded_request(purpose: ClientPurpose, url: &str) -> reqwest::RequestBuilder {
    client(purpose, Duration::from_secs(5))
        .unwrap()
        .get(url)
        .bearer_auth(TOKEN)
}

async fn transport_rows() -> Vec<Exposed> {
    let mut rows = Vec::new();
    // Plain HTTP to a non-loopback host: refused before the network.
    let error = crate::providers::json::<serde_json::Value>(seeded_request(
        ClientPurpose::Provider,
        &sentinel_url("http", URL_HOST),
    ))
    .await
    .unwrap_err();
    assert_eq!(&*error.note.id, "query.insecure_transport");
    rows.push(
        Exposed::new("transport: insecure URL")
            .text(format!("{error:?}"))
            .note(error.note),
    );
    // A provider answering an error with a body full of sentinels.
    for status in [400, 401, 429, 500, 302] {
        let server = answering(status).await;
        let url = format!("{}/{URL_PATH}?token={QUERY}", server.uri());
        let error = crate::providers::json::<serde_json::Value>(seeded_request(
            ClientPurpose::Provider,
            &url,
        ))
        .await
        .unwrap_err();
        rows.push(
            Exposed::new("transport: provider error answer")
                .text(format!("{error:?}"))
                .note(error.note),
        );
    }
    // A host that does not answer at all (closed loopback port).
    let error = crate::providers::json::<serde_json::Value>(seeded_request(
        ClientPurpose::Provider,
        &format!("http://127.0.0.1:1/{URL_PATH}?token={QUERY}"),
    ))
    .await
    .unwrap_err();
    rows.push(
        Exposed::new("transport: unreachable host")
            .text(format!("{error:?}"))
            .note(error.note),
    );
    rows
}

async fn oauth_rows() -> Vec<Exposed> {
    let mut rows = Vec::new();
    let error = crate::oauth::send_secure(seeded_request(
        ClientPurpose::OAuth,
        &sentinel_url("http", URL_HOST),
    ))
    .await
    .unwrap_err();
    rows.push(
        Exposed::new("oauth: insecure endpoint")
            .text(format!("{error} {error:?}"))
            .note(error.l10n())
            .note(error.l10n_for_purge()),
    );
    for status in [400, 401, 403, 429, 500, 302] {
        let server = answering(status).await;
        let response = crate::oauth::send_secure(seeded_request(
            ClientPurpose::OAuth,
            &format!("{}/{URL_PATH}?token={QUERY}", server.uri()),
        ))
        .await
        .unwrap();
        let error = crate::oauth::status_failure(response.status());
        rows.push(
            Exposed::new("oauth: token endpoint error answer")
                .text(format!("{error} {error:?}"))
                .note(error.l10n()),
        );
    }
    rows
}

fn storage_rows(dir: &std::path::Path) -> Vec<Exposed> {
    let mut rows = Vec::new();
    // The development store refuses a path it cannot trust (a symbolic link
    // on Unix, a path under a regular file elsewhere); a release build
    // refuses it outright. Either way the path is never named.
    let target = dir.join(format!("{FILE}-target.sqlite3"));
    std::fs::write(&target, TOKEN).unwrap();
    #[cfg(unix)]
    let refused = {
        let link = dir.join(format!("{FILE}.sqlite3"));
        std::os::unix::fs::symlink(&target, &link).unwrap();
        link
    };
    #[cfg(not(unix))]
    let refused = target.join(format!("{FILE}.sqlite3"));
    let Err(error) = crate::oauth::OAuthSession::development_sqlite(&refused) else {
        panic!("the development store must refuse {refused:?}");
    };
    rows.push(
        Exposed::new("storage: development store")
            .text(format!("{error} {error:?}"))
            .note(error.l10n())
            .note(error.l10n_for_purge()),
    );
    rows
}

fn private_fs_rows(dir: &std::path::Path) -> Vec<Exposed> {
    let mut rows = Vec::new();
    let file = dir.join(format!("{FILE}-regular"));
    std::fs::write(&file, BODY).unwrap();
    // A directory wanted where a file stands.
    let error = crate::private_fs::ensure_dir(&file).unwrap_err();
    rows.push(Exposed::new("private_fs: not a directory").text(format!("{error} {error:?}")));
    // A file wanted under a regular file.
    let error = crate::private_fs::write_file(&file.join(FILE), TOKEN.as_bytes()).unwrap_err();
    rows.push(Exposed::new("private_fs: write under a file").text(format!("{error} {error:?}")));
    // A locked update of a path that is not private.
    #[cfg(unix)]
    {
        let link = dir.join(format!("{FILE}-link.json"));
        std::os::unix::fs::symlink(&file, &link).unwrap();
        let error = crate::private_fs::update_locked::<std::convert::Infallible>(&link, |_| {
            Ok(TOKEN.as_bytes().to_vec())
        })
        .unwrap_err();
        assert!(matches!(error, crate::private_fs::UpdateError::NotPrivate));
        rows.push(Exposed::new("private_fs: symlinked update").text(format!("{error} {error:?}")));
    }
    rows
}

#[cfg(feature = "serve")]
fn server_rows(dir: &std::path::Path) -> Vec<Exposed> {
    use crate::serve::{read_token_file, route_template, ServeError, Token};
    let mut rows = Vec::new();
    // A token file holding something that is not one token.
    let path = dir.join(format!("{FILE}.token"));
    std::fs::write(&path, format!("{TOKEN} {BODY}\n{URL_PATH}")).unwrap();
    let error = read_token_file(&path).unwrap_err();
    assert!(matches!(
        error,
        ServeError::TokenFile | ServeError::InvalidToken
    ));
    rows.push(
        Exposed::new("server: token file")
            .text(format!("{error} {error:?}"))
            .note(error.l10n()),
    );
    let missing = read_token_file(&dir.join(FILE)).unwrap_err();
    rows.push(
        Exposed::new("server: missing token file")
            .text(format!("{missing} {missing:?}"))
            .note(missing.l10n()),
    );
    // A token with a forbidden character: the value is never echoed.
    let Err(error) = Token::parse(&format!("{TOKEN} {BODY}")) else {
        panic!("a token with a space must be refused");
    };
    rows.push(
        Exposed::new("server: invalid token")
            .text(format!("{error} {error:?}"))
            .note(error.l10n()),
    );
    // The access-log route of a request target that carries every sentinel.
    for target in [
        format!("/v1/accounts/{FILE}?token={QUERY}"),
        format!("/{URL_PATH}/{TOKEN}?q={QUERY}"),
        format!("/v1/snapshot?{BODY}"),
    ] {
        rows.push(Exposed::new("server: route template").text(route_template(&target)));
    }
    rows
}

/// Every row's failure text, plus the raw sentinels, written through the
/// log facade: the file holds none of them.
fn log_row(rows: &[Exposed], dir: &std::path::Path) -> Exposed {
    let path = dir.join("cuotai.log");
    let file = Arc::new(crate::private_fs::RotatingFile::new(
        path.clone(),
        256 * 1024,
    ));
    let url = sentinel_url("https", URL_HOST);
    let raw = format!("Authorization: Bearer {TOKEN}");
    let body = format!(r#"{{"error":"{BODY}","token":"{TOKEN}"}}"#);
    logging::with_test_file(AppKind::Cli, file, "debug", || {
        for row in rows {
            for text in row.renderings() {
                logging::event(
                    Level::Error,
                    "test.sentinel",
                    &[
                        ("boundary", Field::Text(row.boundary)),
                        ("detail", Field::Text(&text)),
                    ],
                );
            }
        }
        for level in [Level::Error, Level::Warn, Level::Info, Level::Debug] {
            logging::event(
                level,
                "test.sentinel",
                &[
                    ("url", Field::Text(&url)),
                    ("header", Field::Text(&raw)),
                    ("detail", Field::Text(&raw)),
                    ("response", Field::Text(&body)),
                    ("token", Field::Text(TOKEN)),
                    ("account", Field::Account(TOKEN)),
                ],
            );
        }
    });
    let mut written = std::fs::read_to_string(&path).unwrap_or_default();
    if let Ok(older) = std::fs::read_to_string(path.with_extension("log.1")) {
        written.push_str(&older);
    }
    assert!(written.contains("test.sentinel"), "the log was written");
    Exposed::new("log").text(written)
}

// security-invariant: SEC-ERRORS
#[tokio::test]
async fn security_boundary_failures_never_carry_sentinels() {
    let dir = tempfile::tempdir().unwrap();
    let mut rows = transport_rows().await;
    rows.extend(oauth_rows().await);
    rows.extend(storage_rows(dir.path()));
    rows.extend(private_fs_rows(dir.path()));
    #[cfg(feature = "serve")]
    rows.extend(server_rows(dir.path()));
    for row in &rows {
        assert_no_sentinel(row, &SENTINELS);
    }
    // The log keeps a URL's scheme and host by design (PRD 0052 RF1.3);
    // its path, query, every token and body are gone.
    let log = log_row(&rows, dir.path());
    assert_no_sentinel(&log, &SENTINELS[1..]);
    assert!(rows.len() >= 15, "{} rows", rows.len());
}
