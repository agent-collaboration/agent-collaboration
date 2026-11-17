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
    let mut cells = SessionCells::new();
    let first = admit_create(&mut cells, SESSION, SessionGeneration::UNTRACKED)
        .expect("the create is admitted");
    admit_close(&mut cells, SESSION, close_attempt(1)).expect("the close is admitted");
    let closed = settle_close(&mut cells, SESSION, close_attempt(1), true);

    // Act
    let reloaded = admit_load(
        &mut cells,
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
    let mut cells = SessionCells::new();

    // Act
    let refused = admit_close(&mut cells, SESSION, close_attempt(1));

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
    let mut cells = SessionCells::new();
    admit_load(
        &mut cells,
        SESSION,
        SessionGeneration::UNTRACKED,
        RestoreVia::Load,
        load_attempt(1),
    )
    .expect("the load is admitted");
    admit_close(&mut cells, SESSION, close_attempt(2)).expect("the close is admitted");
    let closed = settle_close(&mut cells, SESSION, close_attempt(2), true);

    // Act
    let late_is_current = is_current_load(&cells, SESSION, load_attempt(1));
    let fresh = admit_load(
        &mut cells,
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
    let mut cells = SessionCells::new();
    admit_create(&mut cells, "live", SessionGeneration::UNTRACKED).expect("created");
    admit_load(
        &mut cells,
        "loading",
        SessionGeneration::UNTRACKED,
        RestoreVia::Load,
        load_attempt(1),
    )
    .expect("loading");
    admit_close(&mut cells, "live", close_attempt(2)).expect("closing live");
    admit_close(&mut cells, "loading", close_attempt(3)).expect("closing loading");

    // Act
    let live_closed = settle_close(&mut cells, "live", close_attempt(2), false);
    let loading_closed = settle_close(&mut cells, "loading", close_attempt(3), false);

    // Assert
    assert_eq!((live_closed, loading_closed), (None, None));
    assert!(matches!(
        cells["live"].lifecycle,
        SessionLifecycle::Live { .. }
    ));
    assert!(matches!(
        cells["loading"].lifecycle,
        SessionLifecycle::NotLoaded { .. }
    ));
}

/// Oracle: D1 R14 (one run per generation). A session this connection hasn't
/// run starts after the generation the sink last published it under, so an
/// earlier connection's run can't share its number.
#[test]
fn a_session_new_to_this_connection_runs_after_the_sinks_last_generation() {
    // Arrange
    let mut cells = SessionCells::new();
    let earlier = SessionGeneration::from_number(7);

    // Act
    let created = admit_create(&mut cells, SESSION, earlier).expect("created");
    let duplicate = admit_create(&mut cells, SESSION, earlier);

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
