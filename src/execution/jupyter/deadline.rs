//! Monotonic phase limits shared by execution and supervised cleanup.

use std::future::{Future, poll_fn};
use std::pin::pin;
use std::task::Poll;
use std::time::Duration;

use tokio::time::Instant;

pub(super) async fn within<T>(
    duration: Duration,
    operation: impl Future<Output = T>,
) -> Result<T, ()> {
    until(Instant::now() + duration, operation).await
}

pub(super) async fn until<T>(
    deadline: Instant,
    operation: impl Future<Output = T>,
) -> Result<T, ()> {
    // Tokio's timeout polls ready work first. Check the clock as well as the
    // timer so expired work cannot win after a delayed or non-yielding poll.
    let mut operation = pin!(operation);
    let mut timer = pin!(tokio::time::sleep_until(deadline));
    poll_fn(|context| {
        if Instant::now() >= deadline || timer.as_mut().poll(context).is_ready() {
            return Poll::Ready(Err(()));
        }
        let result = operation.as_mut().poll(context);
        if Instant::now() >= deadline {
            Poll::Ready(Err(()))
        } else {
            result.map(Ok)
        }
    })
    .await
}

pub(super) fn cell_wait(
    cell: Instant,
    terminal: Option<Instant>,
) -> (Instant, crate::execution::ExecutionPhase) {
    use crate::execution::ExecutionPhase;
    match terminal {
        Some(terminal) if terminal < cell => (terminal, ExecutionPhase::TerminalSync),
        _ => (cell, ExecutionPhase::Cell),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[tokio::test]
    async fn expiry_precedes_ready_work_without_polling_it() {
        let polled = Cell::new(false);
        let result = until(Instant::now() - Duration::from_secs(1), async {
            polled.set(true);
        })
        .await;
        assert!(result.is_err());
        assert!(!polled.get(), "Expired work must not start or advance.");
    }

    #[tokio::test]
    async fn completion_after_a_non_yielding_poll_is_rejected() {
        let result = within(Duration::from_millis(1), async {
            std::thread::sleep(Duration::from_millis(10));
            42
        })
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn completion_before_the_deadline_succeeds() {
        assert_eq!(within(Duration::from_secs(60), async { 42 }).await, Ok(42));
    }

    #[tokio::test]
    async fn expiry_after_a_pending_poll_does_not_advance_work_again() {
        let polls = Cell::new(0);
        let operation = poll_fn(|context| {
            polls.set(polls.get() + 1);
            if polls.get() > 1 {
                return Poll::Ready(());
            }
            context.waker().wake_by_ref();
            std::thread::sleep(Duration::from_millis(10));
            Poll::Pending
        });
        assert!(within(Duration::from_millis(1), operation).await.is_err());
        assert!(
            polls.get() <= 1,
            "Expired pending work must not advance again."
        );
    }

    #[test]
    fn the_first_deadline_determines_the_phase_even_after_both_expire() {
        use crate::execution::ExecutionPhase;
        let earlier = Instant::now() - Duration::from_secs(2);
        let later = earlier + Duration::from_secs(1);
        assert_eq!(cell_wait(later, None), (later, ExecutionPhase::Cell));
        assert_eq!(
            cell_wait(earlier, Some(later)),
            (earlier, ExecutionPhase::Cell)
        );
        assert_eq!(
            cell_wait(later, Some(earlier)),
            (earlier, ExecutionPhase::TerminalSync)
        );
        assert_eq!(
            cell_wait(earlier, Some(earlier)),
            (earlier, ExecutionPhase::Cell)
        );
    }
}
