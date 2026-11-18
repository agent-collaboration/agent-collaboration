use super::*;
use crate::provider_session_actor::SettingReply;
use crate::session_cell_bookkeeping::{
    abandon_run, admit_create, end_run_and_load_next_for_test, settings_held,
};

const SESSION: &str = "session-one";

/// Oracle: Lead V4, admission exactness ("the loop admits the command, so
/// the loop claims its Lost"; the Verifier's case): a change sent while run
/// A was live reaches the connection only after A ended and B loaded, so B
/// takes it. B drops the reply unanswered. B's setting reads Lost (it holds
/// B's prompts) until B's run ends.
#[test]
fn a_change_admitted_after_a_reload_records_its_lost_on_the_run_that_took_it() {
    // Arrange: the caller sent the change while the first run was live.
    let cells = Arc::new(SessionCellTable::default());
    let first = admit_create(&cells, SESSION).expect("created");
    let (reply, answer) = tokio::sync::oneshot::channel();
    let second = end_run_and_load_next_for_test(&cells, SESSION, first);

    // Act: the loop admits it to the newer run, whose actor drops it.
    let admitted = AdmittedSettingReply::admit(reply, &cells, SESSION, ProviderSettingKind::Effort);
    drop(admitted);
    let held_by_admitting_run = settings_held(&cells, SESSION);
    // The admitting run ends as an actor exit ends it (the ActorExited move).
    abandon_run(&cells, SESSION, second);
    let held_after_its_run_ended = settings_held(&cells, SESSION);

    // Assert
    assert!(
        held_by_admitting_run,
        "the run that took the change and dropped its reply holds its setting"
    );
    assert!(
        !held_after_its_run_ended,
        "the Lost lasts only as long as the run that took the change"
    );
    assert!(
        answer.blocking_recv().is_err(),
        "the caller sees no answer and reports the outcome unknown"
    );
}

/// Oracle: Lead V4: only an unanswered reply speaks for its run. An
/// answered change, even a refused one, leaves the run's settings alone.
#[test]
fn an_answered_change_records_nothing_on_its_run() {
    // Arrange
    let cells = Arc::new(SessionCellTable::default());
    let _run = admit_create(&cells, SESSION).expect("created");
    let (reply, answer) = tokio::sync::oneshot::channel();
    let admitted = AdmittedSettingReply::admit(reply, &cells, SESSION, ProviderSettingKind::Effort);

    // Act
    admitted.send(Err(ExternalProviderRuntimeError::LocalNotFound));

    // Assert
    assert!(
        !settings_held(&cells, SESSION),
        "an answered change held the run's setting"
    );
    assert!(
        matches!(
            answer.blocking_recv(),
            Ok(Err(ExternalProviderRuntimeError::LocalNotFound))
        ),
        "the caller gets the actor's answer"
    );
}
