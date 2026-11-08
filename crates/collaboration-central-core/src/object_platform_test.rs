use super::{ObjectCallFailure, ObjectIo};
use crate::ObjectAddress;
use durable_steps::CallFailure;
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use std::{sync::Arc, time::Duration};
use tokio::sync::Notify;

/// Calls answer at once or never; the event deadline passes only when the test says so.
struct ControlledCalls {
    answers: bool,
    deadline_passed: Arc<Notify>,
}
impl Clock for ControlledCalls {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(0)
    }
}
impl IdSource for ControlledCalls {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for ControlledCalls {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl ObjectIo for ControlledCalls {
    fn event_deadline(&self, _: Duration) -> impl Future<Output = ()> + Send {
        let deadline_passed = self.deadline_passed.clone();
        async move { deadline_passed.notified().await }
    }
    async fn object_call(&self, _: ObjectAddress, body: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        if self.answers {
            Ok(body)
        } else {
            std::future::pending().await
        }
    }
}

#[tokio::test]
async fn call_answered_before_its_deadline_returns_the_answer() {
    // Arrange: the deadline never passes.
    let io = ControlledCalls {
        answers: true,
        deadline_passed: Arc::new(Notify::new()),
    };

    // Act
    let answer = io
        .call(
            ObjectAddress::Fleet,
            b"echo".to_vec(),
            Duration::from_secs(5),
        )
        .await;

    // Assert
    assert_eq!(answer, Ok(b"echo".to_vec()));
}

#[tokio::test]
async fn silent_target_ends_when_the_event_deadline_passes() {
    // Arrange: the target never answers, and the deadline has already passed.
    let deadline_passed = Arc::new(Notify::new());
    let io = ControlledCalls {
        answers: false,
        deadline_passed: deadline_passed.clone(),
    };
    deadline_passed.notify_one();

    // Act
    let answer = io
        .call(ObjectAddress::Fleet, Vec::new(), Duration::from_secs(5))
        .await;

    // Assert
    assert_eq!(answer, Err(ObjectCallFailure::DeadlineExceeded));
}

#[test]
fn deadline_reaches_durable_steps_as_a_possibly_delivered_call() {
    // A timed-out call may have reached its target; NotSent would let the
    // engine treat a delivered effect as never attempted.
    assert_eq!(
        ObjectCallFailure::DeadlineExceeded.into_call_failure(),
        CallFailure::AnswerLost
    );
}
