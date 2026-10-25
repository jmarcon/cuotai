//! PRD 0028 RF4.1 — the opt-in capture of the structural fingerprint of the
//! responses a query decoded. Off unless a [`scoped`] future is running:
//! only the local canary turns it on, per account query.
//!
//! The fingerprint holds keys and JSON types only (see
//! [`crate::diagnostic::ResponseShape`]); the value is dropped here.

use crate::diagnostic::ResponseShape;
use serde_json::Value;
use std::cell::RefCell;
use std::future::Future;

tokio::task_local! {
    static CAPTURE: RefCell<Option<ResponseShape>>;
}

/// Note the shape of a response that parsed as JSON. A no-op outside
/// [`scoped`].
pub(crate) fn record(value: &Value) {
    let _ = CAPTURE.try_with(|slot| {
        let shape = ResponseShape::of(value);
        let mut slot = slot.borrow_mut();
        match slot.as_mut() {
            Some(existing) => existing.merge(&shape),
            None => *slot = Some(shape),
        }
    });
}

/// Run `future` capturing the shapes its responses had.
pub(crate) async fn scoped<F: Future>(future: F) -> (F::Output, Option<ResponseShape>) {
    CAPTURE
        .scope(RefCell::new(None), async {
            let output = future.await;
            let shape = CAPTURE.with(|slot| slot.borrow_mut().take());
            (output, shape)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn only_a_scope_captures_and_responses_are_merged() {
        record(&json!({"ignored": 1}));
        let (_, shape) = scoped(async {
            record(&json!({"a": "SECRET", "b": 1}));
            record(&json!({"c": [true]}));
        })
        .await;
        let shape = shape.expect("captured");
        let paths: Vec<&str> = shape.fingerprint.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(paths, [".", "a", "b", "c", "c[]"]);
        assert!(!format!("{shape:?}").contains("SECRET"));
        let (_, none) = scoped(async {}).await;
        assert!(none.is_none());
    }
}
