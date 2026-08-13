use std::{
    collections::HashMap,
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
    waiters: Mutex<Waiters>,
}

/// The waiters currently parked on a token.
///
/// Keyed rather than a bare `Vec` so that a [`Cancelled`] which is dropped before the
/// token ever trips can take its own registration back out again; otherwise a token
/// shared across an operation would accumulate a waker per completed wait.
#[derive(Debug, Default)]
struct Waiters {
    /// Never reused, so a stale id can never name someone else's slot.
    next_id: u64,
    wakers: HashMap<u64, Waker>,
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

        let waiters = std::mem::take(
            &mut self
                .inner
                .waiters
                .lock()
                .expect("cancel token poisoned")
                .wakers,
        );
        for waker in waiters.into_values() {
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
            registration: None,
        }
    }

    /// How many waiters are currently parked on this token.
    #[cfg(test)]
    fn parked_waiters(&self) -> usize {
        self.inner
            .waiters
            .lock()
            .expect("cancel token poisoned")
            .wakers
            .len()
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
    /// Which slot in [`Waiters`] holds this future's waker, once it has parked.
    registration: Option<u64>,
}

impl Future for Cancelled {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();

        if this.inner.cancelled.load(Ordering::SeqCst) {
            // `cancel` already drained every waker, so there is no slot left to free.
            this.registration = None;
            return Poll::Ready(());
        }

        let mut waiters = this.inner.waiters.lock().expect("cancel token poisoned");

        // Re-check under the lock: `cancel` may have run between the load above and
        // taking it, in which case the waker stored below would never be woken.
        if this.inner.cancelled.load(Ordering::SeqCst) {
            this.registration = None;
            return Poll::Ready(());
        }

        // The `Future` contract requires the *latest* waker to be the registered one:
        // a re-poll can arrive from a different task than the last one.
        match this.registration.and_then(|id| waiters.wakers.get_mut(&id)) {
            Some(stored) => {
                if !stored.will_wake(cx.waker()) {
                    *stored = cx.waker().clone();
                }
            }
            None => {
                let id = waiters.next_id;
                waiters.next_id += 1;
                waiters.wakers.insert(id, cx.waker().clone());
                this.registration = Some(id);
            }
        }

        Poll::Pending
    }
}

impl Drop for Cancelled {
    /// Give the slot back, so a token that outlives many waits does not accumulate
    /// wakers for futures that finished without it ever tripping.
    fn drop(&mut self) {
        let Some(id) = self.registration.take() else {
            return;
        };

        if let Ok(mut waiters) = self.inner.waiters.lock() {
            waiters.wakers.remove(&id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::executor::block_on;
    use std::{
        sync::atomic::AtomicUsize,
        task::{Wake, Waker},
    };

    /// A waker that only records whether it was woken.
    #[derive(Debug, Default)]
    struct CountingWaker(AtomicUsize);

    impl CountingWaker {
        fn wakes(&self) -> usize {
            self.0.load(Ordering::SeqCst)
        }
    }

    impl Wake for CountingWaker {
        fn wake(self: Arc<Self>) {
            self.wake_by_ref();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    /// Poll `waiter` once with `waker`, asserting it parks.
    fn poll_pending(waiter: &mut Cancelled, waker: &Waker) {
        assert!(
            Pin::new(waiter)
                .poll(&mut Context::from_waker(waker))
                .is_pending()
        );
    }

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

    /// A re-poll can come from a different task than the one that parked, so the
    /// waker registered last is the only one guaranteed to still be live.
    #[test]
    fn a_repoll_moves_the_wakeup_to_the_newest_waker() {
        let token = CancelToken::new();
        let mut waiter = token.cancelled();

        let stale = Arc::new(CountingWaker::default());
        let fresh = Arc::new(CountingWaker::default());

        poll_pending(&mut waiter, &Waker::from(stale.clone()));
        poll_pending(&mut waiter, &Waker::from(fresh.clone()));

        token.cancel();

        assert_eq!(stale.wakes(), 0, "the waker replaced on re-poll was woken");
        assert_eq!(fresh.wakes(), 1);
        assert!(
            Pin::new(&mut waiter)
                .poll(&mut Context::from_waker(&Waker::from(fresh)))
                .is_ready()
        );
    }

    #[test]
    fn repolling_with_the_same_waker_parks_only_once() {
        let token = CancelToken::new();
        let mut waiter = token.cancelled();
        let waker = Waker::from(Arc::new(CountingWaker::default()));

        poll_pending(&mut waiter, &waker);
        poll_pending(&mut waiter, &waker);
        poll_pending(&mut waiter, &waker);

        assert_eq!(token.parked_waiters(), 1);
    }

    #[test]
    fn a_dropped_waiter_takes_its_registration_with_it() {
        let token = CancelToken::new();
        let waker = Waker::from(Arc::new(CountingWaker::default()));

        for _ in 0..100 {
            let mut waiter = token.cancelled();
            poll_pending(&mut waiter, &waker);
            assert_eq!(token.parked_waiters(), 1);
        }

        assert_eq!(token.parked_waiters(), 0);
    }

    /// The same token is handed to every operation, so waits that end without
    /// cancellation must not leave anything behind.
    #[test]
    fn run_until_leaves_nothing_parked_when_the_future_wins() {
        let token = CancelToken::new();

        for _ in 0..100 {
            assert_eq!(block_on(token.run_until(async { 42 })), Some(42));
        }

        assert_eq!(token.parked_waiters(), 0);
    }
}
