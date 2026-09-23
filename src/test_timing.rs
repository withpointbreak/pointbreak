//! Shared timing for library test waits.

use std::panic::{self, AssertUnwindSafe};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

/// Upper bound for a test's wait on progress it expects to happen. Only a real
/// hang should reach it, so a loaded host passes and a deadlock still fails.
pub(crate) const HANG_GUARD: Duration = Duration::from_secs(60);

/// A test operation running on its own thread, awaited within a bound.
pub(crate) struct BoundedThread<T> {
    outcome: mpsc::Receiver<std::thread::Result<T>>,
    handle: JoinHandle<()>,
}

/// Run `f` on a new thread, catching its panic.
pub(crate) fn spawn_bounded<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> BoundedThread<T> {
    let (sender, outcome) = mpsc::channel();
    let handle = std::thread::spawn(move || {
        let _ = sender.send(panic::catch_unwind(AssertUnwindSafe(f)));
    });
    BoundedThread { outcome, handle }
}

impl<T> BoundedThread<T> {
    /// The result, or the thread's panic re-raised; panics "{what} did not finish within {bound:?}" on timeout.
    pub(crate) fn wait_within(self, bound: Duration, what: &str) -> T {
        // On timeout the thread stays detached; the failing test ends the process.
        let outcome = match self.outcome.recv_timeout(bound) {
            Ok(outcome) => outcome,
            Err(RecvTimeoutError::Timeout) => panic!("{what} did not finish within {bound:?}"),
            Err(RecvTimeoutError::Disconnected) => panic!("{what} ended without a result"),
        };
        let _ = self.handle.join();
        outcome.unwrap_or_else(|payload| panic::resume_unwind(payload))
    }

    /// `wait_within(HANG_GUARD, what)`.
    pub(crate) fn wait(self, what: &str) -> T {
        self.wait_within(HANG_GUARD, what)
    }

    pub(crate) fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{self, AssertUnwindSafe};
    use std::sync::mpsc;
    use std::time::Duration;

    use super::spawn_bounded;

    #[test]
    fn spawn_bounded_returns_the_result() {
        assert_eq!(spawn_bounded(|| 7).wait("seven"), 7);
    }

    #[test]
    fn spawn_bounded_reraises_the_thread_panic() {
        let payload = panic::catch_unwind(AssertUnwindSafe(|| {
            spawn_bounded::<()>(|| panic!("inner")).wait("x")
        }))
        .expect_err("the thread's panic is re-raised on the waiting thread");
        assert_eq!(payload.downcast_ref::<&str>(), Some(&"inner"));
    }

    #[test]
    fn spawn_bounded_times_out_with_the_operation_name() {
        let (_park, parked) = mpsc::channel::<()>();
        let parked_op = spawn_bounded(move || parked.recv());
        let payload = panic::catch_unwind(AssertUnwindSafe(|| {
            parked_op.wait_within(Duration::from_millis(20), "parked op")
        }))
        .expect_err("a parked operation times out");
        let message = payload
            .downcast_ref::<String>()
            .expect("the timeout message is formatted");
        assert!(
            message.contains("parked op did not finish within"),
            "unexpected timeout message: {message}"
        );
    }
}
