use super::*;

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
    let first = admit_create(&cells, SESSION, SessionGeneration::UNTRACKED)
        .expect("the create is admitted");
    admit_close(&cells, SESSION, close_attempt(1)).expect("the close is admitted");
    let closed = settle_close(&cells, SESSION, close_attempt(1), true);

    // Act
    let reloaded = admit_load(
        &cells,
        SESSION,
        SessionGeneration::UNTRACKED,
        RestoreVia::Load,
        load_attempt(2),
    );

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
    admit_load(
        &cells,
        SESSION,
        SessionGeneration::UNTRACKED,
        RestoreVia::Load,
        load_attempt(1),
    )
    .expect("the load is admitted");
    admit_close(&cells, SESSION, close_attempt(2)).expect("the close is admitted");
    let closed = settle_close(&cells, SESSION, close_attempt(2), true);

    // Act
    let late_is_current = is_current_load(&cells, SESSION, load_attempt(1));
    let fresh = admit_load(
        &cells,
        SESSION,
        SessionGeneration::UNTRACKED,
        RestoreVia::Resume,
        load_attempt(3),
    );

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
    admit_create(&cells, "live", SessionGeneration::UNTRACKED).expect("created");
    admit_load(
        &cells,
        "loading",
        SessionGeneration::UNTRACKED,
        RestoreVia::Load,
        load_attempt(1),
    )
    .expect("loading");
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

/// Oracle: D1 R14 (one run per generation). A session this connection hasn't
/// run starts after the generation the sink last published it under, so an
/// earlier connection's run can't share its number.
#[test]
fn a_session_new_to_this_connection_runs_after_the_sinks_last_generation() {
    // Arrange
    let cells = SessionCellTable::default();
    let earlier = SessionGeneration::from_number(7);

    // Act
    let created = admit_create(&cells, SESSION, earlier).expect("created");
    let duplicate = admit_create(&cells, SESSION, earlier);

    // Assert
    assert_eq!(created.number(), 8);
    assert!(
        matches!(
            &duplicate,
            Err(ExternalProviderRuntimeError::Operation(message))
                if message == "provider returned a duplicate conversation identity"
        ),
        "{duplicate:?}"
    );
    assert!(!is_closing(&cells, SESSION));
}

/// Oracle: design note §12.1 (Closing{EndingTurn} advances only on the
/// running turn's end) and the owner's close decision (a done close ends
/// NotLoaded and publishes Closed). A close that finds a prompt running
/// waits for that prompt's turn to end, then frees and publishes Closed.
#[tokio::test]
async fn a_close_during_a_prompt_frees_only_after_the_turn_ends() {
    // Arrange
    let cells = std::sync::Arc::new(SessionCellTable::default());
    let generation = admit_create(&cells, SESSION, SessionGeneration::UNTRACKED).expect("created");
    let turn = TurnId(uuid::Uuid::from_u128(5));
    assert!(
        start_direct_turn(&cells, SESSION, turn).is_ok(),
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
