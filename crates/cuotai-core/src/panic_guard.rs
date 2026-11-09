//! Per-account panic isolation (PRD 0003 re-validation S-01/R-03).
//!
//! A bug in one adapter (an arithmetic overflow on a provider value, for
//! example) must fail that account's reading, not lose every report of the
//! refresh or take a frontend down. [`isolate`] runs a future under
//! `catch_unwind` and marks the polling thread while it does, so a frontend's
//! panic hook can tell an isolated panic (the refresh goes on) from a fatal
//! one with [`panic_isolated`]: panic hooks run before unwinding, so a hook
//! that restores the terminal would otherwise tear the TUI down even though
//! the panic is caught.

use std::cell::Cell;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

use futures_util::FutureExt;

thread_local! {
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Whether the current thread is polling a future under [`isolate`]. A panic
/// hook that sees `true` must not tear down the process or the terminal: the
/// panic becomes that account's failed report.
pub fn panic_isolated() -> bool {
    DEPTH.with(|depth| depth.get() > 0)
}

/// A panic caught by [`isolate`]; its payload was dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panicked;

/// Run `future`, turning a panic while it is polled into [`Panicked`]. The
/// payload is dropped: its text never reaches a report.
pub async fn isolate<F: Future>(future: F) -> Result<F::Output, Panicked> {
    std::panic::AssertUnwindSafe(Marked {
        inner: Box::pin(future),
    })
    .catch_unwind()
    .await
    .map_err(|_| Panicked)
}

/// Sets the thread mark for the duration of each poll; the guard clears it
/// while unwinding, after the panic hook has run.
struct Marked<F> {
    inner: Pin<Box<F>>,
}

impl<F: Future> Future for Marked<F> {
    type Output = F::Output;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let _mark = Mark::enter();
        self.inner.as_mut().poll(cx)
    }
}

struct Mark;

impl Mark {
    fn enter() -> Self {
        DEPTH.with(|depth| depth.set(depth.get() + 1));
        Mark
    }
}

impl Drop for Mark {
    fn drop(&mut self) {
        DEPTH.with(|depth| depth.set(depth.get().saturating_sub(1)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_panic_becomes_an_error_and_clears_the_mark() {
        assert!(!panic_isolated());
        let seen = isolate(async { panic_isolated() }).await;
        assert_eq!(seen, Ok(true));
        let failed = isolate(async {
            tokio::task::yield_now().await;
            panic!("isolated test panic");
        })
        .await;
        assert_eq!(failed, Err(Panicked));
        assert!(!panic_isolated());
    }
}
