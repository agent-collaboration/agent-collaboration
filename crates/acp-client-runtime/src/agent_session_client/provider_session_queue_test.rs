use super::*;
use crate::session_lifecycle_machine::CloseAttempt;
use std::sync::Mutex;

/// Records every message the sink is told ended not sent.
#[derive(Default)]
struct RecordingSink(Mutex<Vec<(String, String, &'static str)>>);

impl QueueEntrySink for RecordingSink {
    fn entry_not_sent(&self, session_id: &str, operation_id: &str, reason: &'static str) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((session_id.to_owned(), operation_id.to_owned(), reason));
    }
}

impl RecordingSink {
    fn ended(&self) -> Vec<(String, &'static str)> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(_, operation, reason)| (operation.clone(), *reason))
            .collect()
    }
}

const SESSION: &str = "session-one";

fn live_table() -> (SessionCellTable, Arc<RecordingSink>) {
    let cells = SessionCellTable::default();
    let sink = Arc::new(RecordingSink::default());
    cells.set_sink(Arc::clone(&sink) as Arc<dyn QueueEntrySink>);
    cells
        .transition(
            SESSION,
            SessionCellEvent::CreateAdmitted,
            EntryEffectRoute::Sink,
        )
        .expect("the session is Live");
    (cells, sink)
}

fn close(cells: &SessionCellTable) {
    cells
        .transition(
            SESSION,
            SessionCellEvent::CloseRequested {
                attempt: CloseAttempt(uuid::Uuid::from_u128(9)),
            },
            EntryEffectRoute::Sink,
        )
        .expect("the close is admitted");
}

/// Oracle: design note §11.2's linearization: only the head commits, and
/// only one message is in flight until its turn ends.
#[test]
fn only_the_head_commits_and_one_message_is_in_flight() {
    // Arrange
    let (cells, _sink) = live_table();
    let door = SessionQueueDoor { cells: &cells };
    let first = door.queue_enqueue(SESSION, "a").expect("a joins");
    let second = door.queue_enqueue(SESSION, "b").expect("b joins");

    // Act
    let second_early = door.queue_claim(SESSION, second.sequence);
    door.queue_claim(SESSION, first.sequence)
        .expect("a is the head");
    door.queue_commit(SESSION, first.sequence)
        .expect("a commits");
    let step = door.queue_mark_written(SESSION, first.sequence, true);
    door.queue_claim(SESSION, second.sequence)
        .expect("b is the head now");
    let second_commit = door.queue_commit(SESSION, second.sequence);

    // Assert
    assert_eq!((first.sequence, second.sequence), (0, 1));
    assert_eq!(second_early, Err(QueueRefusal::NotYet));
    assert!(matches!(step, QueueMarkStep::Send { .. }), "{step:?}");
    assert_eq!(
        second_commit,
        Err(QueueRefusal::NotYet),
        "b waits while a's turn runs"
    );
}

/// Oracle: the accepted door rule and M8: a message for a session that isn't
/// loaded is refused with the state and its next steps, so a door that may
/// load loads it first.
#[test]
fn a_message_for_a_session_that_is_not_loaded_is_refused_with_its_next_steps() {
    // Arrange
    let cells = SessionCellTable::default();
    let door = SessionQueueDoor { cells: &cells };

    // Act
    let refused = door.queue_enqueue(SESSION, "a");

    // Assert
    assert_eq!(
        refused,
        Err(QueueRefusal::NotLoaded {
            answer: "session is not loaded; load it or resume it".to_owned()
        })
    );
}

/// Oracle: §12.2's settlement table: a close ends waiting, held and committed
/// messages not sent through the sink; a message whose mark is being written
/// is withdrawn, and its runner settles it after undoing the mark.
#[test]
fn a_close_settles_unsent_messages_once_each() {
    // Arrange
    let (cells, sink) = live_table();
    let door = SessionQueueDoor { cells: &cells };
    let marking = door.queue_enqueue(SESSION, "marking").expect("joins");
    door.queue_enqueue(SESSION, "waiting").expect("joins");
    door.queue_claim(SESSION, marking.sequence).expect("head");
    door.queue_commit(SESSION, marking.sequence)
        .expect("commits and its mark write starts");

    // Act
    close(&cells);
    let step = door.queue_mark_written(SESSION, marking.sequence, true);
    let unmarked = door.queue_unmark_done(SESSION, marking.sequence, true);
    let late = door.queue_enqueue(SESSION, "late");

    // Assert
    assert_eq!(sink.ended(), vec![("waiting".to_owned(), "session closed")]);
    assert_eq!(step, QueueMarkStep::Withdraw);
    assert_eq!(unmarked, Some("session closed"));
    assert!(
        matches!(&late, Err(QueueRefusal::Refused { answer }) if answer == "session is closing; wait for it"),
        "{late:?}"
    );
}

/// Oracle: §11.2's cancel rows: a waiting message ends "cancelled from the
/// queue"; a sent one is refused, "already sent; cancel the turn instead".
#[test]
fn cancel_ends_a_waiting_message_and_refuses_a_sent_one() {
    // Arrange
    let (cells, _sink) = live_table();
    let door = SessionQueueDoor { cells: &cells };
    let sent = door.queue_enqueue(SESSION, "sent").expect("joins");
    door.queue_enqueue(SESSION, "waiting").expect("joins");
    door.queue_claim(SESSION, sent.sequence).expect("head");
    door.queue_commit(SESSION, sent.sequence).expect("commits");
    let _step = door.queue_mark_written(SESSION, sent.sequence, true);

    // Act
    let waiting = door.queue_cancel(SESSION, "waiting");
    let already_sent = door.queue_cancel(SESSION, "sent");

    // Assert
    assert_eq!(
        waiting,
        Ok(QueueCancel::NotSent {
            reason: "cancelled from the queue"
        })
    );
    assert_eq!(
        already_sent,
        Err(QueueRefusal::Refused {
            answer: "already sent; cancel the turn instead".to_owned()
        })
    );
}

/// Oracle: §11.1's shutdown: Stop ends every line, and messages not yet sent
/// end "Router shutting down" through the sink.
#[test]
fn stop_ends_every_line_through_the_sink() {
    // Arrange
    let (cells, sink) = live_table();
    let door = SessionQueueDoor { cells: &cells };
    door.queue_enqueue(SESSION, "waiting").expect("joins");

    // Act
    door.queue_stop();

    // Assert
    assert_eq!(
        sink.ended(),
        vec![("waiting".to_owned(), "Router shutting down")]
    );
}

/// Oracle: M1 / inventory S1 (§11.9): after a close fails, a new send is
/// accepted; nothing stays half-closed or banned. The close ended the line;
/// its failure reopens it under the same run.
#[test]
fn a_send_after_a_failed_close_is_accepted() {
    // Arrange
    let (cells, _sink) = live_table();
    let door = SessionQueueDoor { cells: &cells };
    let attempt = CloseAttempt(uuid::Uuid::from_u128(9));
    close(&cells);
    let refused_while_closing = door.queue_enqueue(SESSION, "during-close");

    // Act
    cells
        .transition(
            SESSION,
            SessionCellEvent::CloseFailed { attempt },
            EntryEffectRoute::Sink,
        )
        .expect("the close fails");
    let after_failure = door.queue_enqueue(SESSION, "after-failure");

    // Assert
    assert!(
        matches!(refused_while_closing, Err(QueueRefusal::Refused { .. })),
        "{refused_while_closing:?}"
    );
    assert!(after_failure.is_ok(), "{after_failure:?}");
}
