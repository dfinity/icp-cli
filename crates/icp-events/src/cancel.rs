use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};

use futures::future::{Either, select};

/// A cooperative cancellation signal shared by everything a [`Reporter`](crate::Reporter)
/// hands out.
///
/// Deliberately runtime-agnostic: it is a flag plus a waker list, so operations can
/// wait on cancellation without pulling in an async runtime.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    inner: Arc<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    cancelled: AtomicBool,
    wakers: Mutex<Vec<Waker>>,
}

impl CancelToken {
    /// A fresh, uncancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Trip the token and wake everything waiting on it. Idempotent.
    pub fn cancel(&self) {
        if self.inner.cancelled.swap(true, Ordering::SeqCst) {
            return;
        }

        let wakers = std::mem::take(&mut *self.inner.wakers.lock().expect("cancel token poisoned"));
        for waker in wakers {
            waker.wake();
        }
    }

    /// Whether [`cancel`](CancelToken::cancel) has been called.
    pub fn is_cancelled(&self) -> bool {
        self.inner.cancelled.load(Ordering::SeqCst)
    }

    /// A future that resolves once the token is cancelled.
    pub fn cancelled(&self) -> Cancelled {
        Cancelled {
            inner: self.inner.clone(),
            registered: false,
        }
    }

    /// Run `future` unless cancellation wins the race.
    ///
    /// Returns `None` if the token tripped first, in which case `future` is dropped
    /// at its next suspension point.
    pub async fn run_until<F: Future>(&self, future: F) -> Option<F::Output> {
        let cancelled = self.cancelled();
        futures::pin_mut!(cancelled);
        futures::pin_mut!(future);

        match select(cancelled, future).await {
            Either::Left(((), _)) => None,
            Either::Right((output, _)) => Some(output),
        }
    }
}

/// The future returned by [`CancelToken::cancelled`].
#[derive(Debug)]
pub struct Cancelled {
    inner: Arc<Inner>,
    registered: bool,
}

impl Future for Cancelled {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();

        if this.inner.cancelled.load(Ordering::SeqCst) {
            return Poll::Ready(());
        }

        // Register once, then re-check: `cancel` may have run between the load above
        // and taking the lock, in which case our waker would never be woken.
        if !this.registered {
            let mut wakers = this.inner.wakers.lock().expect("cancel token poisoned");
            if this.inner.cancelled.load(Ordering::SeqCst) {
                return Poll::Ready(());
            }
            wakers.push(cx.waker().clone());
            this.registered = true;
        }

        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;

    #[test]
    fn starts_uncancelled_and_trips_once() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());

        token.cancel();
        assert!(token.is_cancelled());

        // Idempotent.
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn clones_share_one_signal() {
        let token = CancelToken::new();
        let clone = token.clone();

        clone.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn cancelled_resolves_when_already_cancelled() {
        let token = CancelToken::new();
        token.cancel();

        block_on(token.cancelled());
    }

    #[test]
    fn cancelled_wakes_a_pending_waiter() {
        let token = CancelToken::new();
        let waiter = token.cancelled();

        let cancel_from_another_thread = {
            let token = token.clone();
            std::thread::spawn(move || token.cancel())
        };

        block_on(waiter);
        cancel_from_another_thread.join().unwrap();
    }

    #[test]
    fn run_until_returns_the_output_when_not_cancelled() {
        let token = CancelToken::new();
        assert_eq!(block_on(token.run_until(async { 42 })), Some(42));
    }

    #[test]
    fn run_until_returns_none_when_already_cancelled() {
        let token = CancelToken::new();
        token.cancel();

        assert_eq!(
            block_on(token.run_until(futures::future::pending::<u8>())),
            None
        );
    }
}
