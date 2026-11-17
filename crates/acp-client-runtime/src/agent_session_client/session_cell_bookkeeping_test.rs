use super::*;
use provider_session_machines::SessionGeneration;

const SESSION: &str = "session-one";

fn load_attempt(id: u128) -> LoadAttempt {
    LoadAttempt(uuid::Uuid::from_u128(id))
}

fn close_attempt(id: u128) -> CloseAttempt {
    CloseAttempt(uuid::Uuid::from_u128(id))
}

/// Oracle: the owner's close decision (ACP close: cancel, then free; it ends
/// NotLoaded and can be loaded again) and M4 (a later load starts g+1).
#[test]
fn a_closed_session_loads_again_under_a_newer_generation() {
    // Arrange
    let cells = SessionCellTable::default();
    let first = admit_create(&cells, SESSION).expect("the create is admitted");
    admit_close(&cells, SESSION, close_attempt(1)).expect("the close is admitted");
    let closed = settle_close(&cells, SESSION, close_attempt(1), true);

    // Act
    let reloaded = admit_load(&cells, SESSION, RestoreVia::Load, load_attempt(2));

    // Assert
    assert_eq!(closed, Some(first));
    assert_eq!(
        reloaded.expect("a closed session loads again").number(),
        first.number() + 1
    );
}

/// Oracle: the owner's close decision and M8. Close on a session that isn't
/// loaded is refused "nothing to close" with its valid next steps.
#[test]
fn closing_a_session_that_is_not_loaded_answers_nothing_to_close() {
    // Arrange
    let cells = SessionCellTable::default();

    // Act
    let refused = admit_close(&cells, SESSION, close_attempt(1));

    // Assert
    assert!(
        matches!(
            &refused,
            Err(ExternalProviderRuntimeError::SessionStateRefused { answer })
                if answer == "session is not loaded; nothing to close; load it or resume it"
        ),
        "{refused:?}"
    );
}

/// Oracle: G3 at 87b6b9d8c, now as the load attempt (M4). A load that lands
/// after a close registers nothing, and a fresh load is admitted.
#[test]
fn a_load_landing_after_a_close_is_stale_and_a_fresh_load_is_admitted() {
    // Arrange
    let cells = SessionCellTable::default();
    admit_load(&cells, SESSION, RestoreVia::Load, load_attempt(1)).expect("the load is admitted");
    admit_close(&cells, SESSION, close_attempt(2)).expect("the close is admitted");
    let closed = settle_close(&cells, SESSION, close_attempt(2), true);

    // Act
    let late_is_current = is_current_load(&cells, SESSION, load_attempt(1));
    let fresh = admit_load(&cells, SESSION, RestoreVia::Resume, load_attempt(3));

    // Assert
    assert!(closed.is_some(), "the close published Closed");
    assert!(!late_is_current, "the late load still counted");
    assert!(fresh.is_ok(), "the fresh load was refused: {fresh:?}");
    assert!(is_current_load(&cells, SESSION, load_attempt(3)));
}

/// Oracle: §12.1's close-failure rule. A failed close goes back to Live from
/// Live and to NotLoaded from Loading.
#[test]
fn a_failed_close_returns_to_where_it_started() {
    // Arrange
    let cells = SessionCellTable::default();
    admit_create(&cells, "live").expect("created");
    admit_load(&cells, "loading", RestoreVia::Load, load_attempt(1)).expect("loading");
    admit_close(&cells, "live", close_attempt(2)).expect("closing live");
    admit_close(&cells, "loading", close_attempt(3)).expect("closing loading");

    // Act
    let live_closed = settle_close(&cells, "live", close_attempt(2), false);
    let loading_closed = settle_close(&cells, "loading", close_attempt(3), false);

    // Assert
    assert_eq!((live_closed, loading_closed), (None, None));
    assert!(matches!(
        cells.read("live", |cell| cell.lifecycle),
        SessionLifecycle::Live { .. }
    ));
    assert!(matches!(
        cells.read("loading", |cell| cell.lifecycle),
        SessionLifecycle::NotLoaded { .. }
    ));
}

/// Oracle: design note §12.1 (Closing{EndingTurn} advances only on the
/// running turn's end) and the owner's close decision (a done close ends
/// NotLoaded and publishes Closed). A close that finds a prompt running
/// waits for that prompt's turn to end, then frees and publishes Closed.
#[tokio::test]
async fn a_close_during_a_prompt_frees_only_after_the_turn_ends() {
    // Arrange
    let cells = std::sync::Arc::new(SessionCellTable::default());
    let generation = admit_create(&cells, SESSION).expect("created");
    let turn = TurnId(uuid::Uuid::from_u128(5));
    assert!(
        start_direct_turn(&cells, SESSION, turn, false).is_ok(),
        "the prompt's turn opens"
    );
    let mut guard = CellTurnGuard::new(&cells, SESSION, turn);
    admit_close(&cells, SESSION, close_attempt(1)).expect("the close is admitted");

    // Act
    let early = settle_close(&cells, SESSION, close_attempt(1), true);
    let waiting = wait_for_close_turn(&cells, SESSION);
    tokio::pin!(waiting);
    let pending = futures_util::poll!(waiting.as_mut()).is_pending();
    guard.applied();
    drop(guard);
    waiting.await;
    let closed = settle_close(&cells, SESSION, close_attempt(1), true);

    // Assert
    assert_eq!(early, None, "nothing frees while the turn runs");
    assert!(pending, "the close waits for the turn's end");
    assert_eq!(
        closed,
        Some(generation),
        "then it frees and publishes Closed"
    );
}

/// Records what the hub is told, with the run it was told in.
#[derive(Default)]
struct HubRecorder(std::sync::Mutex<Vec<(SessionGeneration, session_event_model::SessionEvent)>>);

impl crate::SessionEventSink for HubRecorder {
    fn publish_skill_view(
        &self,
        _session_id: &str,
        _view: session_event_model::SkillView,
    ) -> Result<(), crate::EventSinkClosed> {
        Ok(())
    }

    fn begin_history_replay(&self, _session_id: &str) -> crate::HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }

    fn publish(
        &self,
        _session_id: &str,
        event: session_event_model::SessionEvent,
    ) -> Result<(), crate::EventSinkClosed> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((SessionGeneration::UNTRACKED, event));
        Ok(())
    }

    fn publish_in(
        &self,
        _session_id: &str,
        generation: SessionGeneration,
        event: session_event_model::SessionEvent,
    ) -> Result<crate::GenerationPublication, crate::EventSinkClosed> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((generation, event));
        Ok(crate::GenerationPublication::Published)
    }
}

/// Records the queued messages the queue sink is told ended.
#[derive(Default)]
struct EndedMessages(std::sync::Mutex<Vec<(String, &'static str)>>);

impl crate::QueueEntrySink for EndedMessages {
    fn entry_not_sent(&self, _session_id: &str, operation_id: &str, reason: &'static str) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((operation_id.to_owned(), reason));
    }
}

/// Oracle: design note §11.2 and inventory S2: an actor whose loop ends on
/// its own ends its run: the session goes from Live to NotLoaded, the hub
/// reads Unloaded in that run, its waiting messages end not sent "the agent
/// stopped", and the session can be loaded again under a newer run.
#[test]
fn an_actor_exit_ends_the_run_and_the_session_loads_again() {
    // Arrange
    let cells = SessionCellTable::default();
    let ended = std::sync::Arc::new(EndedMessages::default());
    cells.set_sink(std::sync::Arc::clone(&ended) as std::sync::Arc<dyn crate::QueueEntrySink>);
    let generation = admit_create(&cells, SESSION).expect("created");
    cells
        .transition(
            SESSION,
            SessionCellEvent::Enqueue {
                operation: provider_session_machines::session_cell_machine::QueuedOperation(
                    "waiting".to_owned(),
                ),
            },
            crate::session_cell_table::EntryEffectRoute::Caller,
        )
        .expect("a message waits");
    let hub = HubRecorder::default();

    // Act
    report_actor_exit(&cells, &hub, SESSION, generation);
    let stale_exit = HubRecorder::default();
    report_actor_exit(&cells, &stale_exit, SESSION, generation);
    let reloaded = admit_load(&cells, SESSION, RestoreVia::Load, load_attempt(9));

    // Assert
    let published = hub
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert_eq!(
        published,
        vec![(
            generation,
            session_event_model::SessionEvent::StateChanged {
                state: session_event_model::SessionState::Unloaded,
            }
        )]
    );
    assert_eq!(
        ended
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
        vec![("waiting".to_owned(), "the agent stopped")]
    );
    assert!(
        stale_exit
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty(),
        "a second report for the same run publishes nothing"
    );
    assert_eq!(
        reloaded.map(|next| next.number()).ok(),
        Some(generation.number() + 1)
    );
}
