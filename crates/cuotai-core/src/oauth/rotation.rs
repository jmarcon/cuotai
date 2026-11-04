//! Salvage of a refresh token that a token endpoint rotated in a response the
//! refresh then rejected.
//!
//! A provider that rotates refresh tokens invalidates the old one as soon as
//! it answers. When the rest of that answer fails validation (a non-bearer
//! `token_type`, an `expires_in` sent as a string, a missing access token), the
//! refresh fails and, without this module, the only valid refresh token is
//! dropped with the response: the next refresh sends the invalidated one and
//! the account ends logged out.
//!
//! [`OAuthSession`](super::OAuthSession) runs every refresh inside
//! [`capture`]. The shared body readers of the provider modules call
//! [`observe`] on each successful JSON answer; outside a capture (logins,
//! quota requests) it does nothing. The captured value only ever reaches the
//! vault, never an error, a log or `Debug` output.

use serde_json::Value;
use std::cell::RefCell;
use std::future::Future;

tokio::task_local! {
    static OBSERVED: RefCell<Option<String>>;
}

/// Longest refresh token accepted for salvage; longer values are not tokens
/// this client would send back.
const MAX_REFRESH_TOKEN_LEN: usize = 16 * 1024;

/// Run `future` and return its output with the first refresh token a token
/// endpoint answered during it, whether or not the answer was then accepted.
pub(super) async fn capture<F: Future>(future: F) -> (F::Output, Option<String>) {
    OBSERVED
        .scope(RefCell::new(None), async move {
            let output = future.await;
            let observed = OBSERVED.with(|cell| cell.borrow_mut().take());
            (output, observed)
        })
        .await
}

/// Record the refresh token a successful JSON answer carries, at the top level
/// or in a `data` envelope, in snake or camel case. The first one wins: the
/// token endpoint answers before any follow-up request of a refresh.
pub(super) fn observe(status: reqwest::StatusCode, body: &Value) {
    if !status.is_success() {
        return;
    }
    let Some(found) = refresh_token_in(body) else {
        return;
    };
    // Outside a capture there is nothing to record.
    let _ = OBSERVED.try_with(|cell| {
        let mut cell = cell.borrow_mut();
        if cell.is_none() {
            *cell = Some(found);
        }
    });
}

fn refresh_token_in(body: &Value) -> Option<String> {
    [Some(body), body.get("data")]
        .into_iter()
        .flatten()
        .flat_map(|level| ["refresh_token", "refreshToken"].map(|name| level.get(name)))
        .flatten()
        .find_map(Value::as_str)
        .map(str::trim)
        .filter(|token| {
            !token.is_empty()
                && token.len() <= MAX_REFRESH_TOKEN_LEN
                && token.chars().all(|c| c.is_ascii_graphic())
        })
        .map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;
    use serde_json::json;

    async fn observed(status: StatusCode, body: Value) -> Option<String> {
        capture(async move { observe(status, &body) }).await.1
    }

    #[tokio::test]
    async fn finds_the_refresh_token_in_every_supported_shape() {
        for body in [
            json!({"refresh_token": "fake-rt"}),
            json!({"refreshToken": "fake-rt"}),
            json!({"code": 0, "data": {"refreshToken": "fake-rt"}}),
            json!({"data": {"refresh_token": " fake-rt "}}),
        ] {
            assert_eq!(
                observed(StatusCode::OK, body).await.as_deref(),
                Some("fake-rt")
            );
        }
    }

    #[tokio::test]
    async fn ignores_failures_and_values_that_are_not_tokens() {
        assert_eq!(
            observed(StatusCode::BAD_REQUEST, json!({"refresh_token": "fake-rt"})).await,
            None
        );
        for body in [
            json!({}),
            json!({"refresh_token": ""}),
            json!({"refresh_token": 42}),
            json!({"refresh_token": "has space"}),
            json!({"refresh_token": "x".repeat(MAX_REFRESH_TOKEN_LEN + 1)}),
        ] {
            assert_eq!(observed(StatusCode::OK, body).await, None);
        }
    }

    #[tokio::test]
    async fn keeps_the_first_answer_and_ignores_calls_outside_a_capture() {
        observe(StatusCode::OK, &json!({"refresh_token": "fake-outside"}));
        let ((), seen) = capture(async {
            observe(StatusCode::OK, &json!({"refresh_token": "fake-first"}));
            observe(StatusCode::OK, &json!({"refresh_token": "fake-second"}));
        })
        .await;
        assert_eq!(seen.as_deref(), Some("fake-first"));
    }
}
