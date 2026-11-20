use super::*;
use uuid::Uuid;

#[path = "session_owner/owner_history_test.rs"]
mod owner_history;
#[path = "session_owner/owner_property_test.rs"]
mod owner_property;

fn uuid(number: u64) -> Uuid {
    // Deterministic UUIDv7 inputs keep clock reads out of the property.
    Uuid::from_u128(0x0000_0000_0000_7000_8000_0000_0000_0000 | u128::from(number))
}

fn input(generation: SessionGeneration, number: u64) -> OwnerInput {
    let identity = uuid(number);
    OwnerInput {
        generation,
        input: InputId::new(identity.to_string()).expect("fixture input identity"),
        turn: TurnId(identity),
    }
}

fn apply(state: &mut SessionOwnerState, event: SessionOwnerEvent) -> Vec<SessionOwnerEffect> {
    match state.decide(event) {
        Ok(SessionOwnerCommit { next, effects }) => {
            *state = next;
            effects
        }
        Err(SessionOwnerRejection::Refused(reason)) => panic!("fixture event refused: {reason:?}"),
        Err(SessionOwnerRejection::IgnoredLate) => panic!("fixture event was late"),
    }
}

fn command(state: &mut SessionOwnerState, command: SessionOwnerCommand) -> Vec<SessionOwnerEffect> {
    apply(state, SessionOwnerEvent::Command(command))
}

fn start_restore(state: &mut SessionOwnerState, number: u64) -> RunAttempt<LoadAttempt> {
    let effects = command(
        state,
        SessionOwnerCommand::Restore {
            attempt: LoadAttempt(uuid(number)),
            via: RestoreVia::Load,
        },
    );
    effects
        .into_iter()
        .find_map(|effect| match effect {
            SessionOwnerEffect::Store(SessionStoreRequest::ReadRestoreSetup(run)) => Some(run),
            _ => None,
        })
        .expect("owner released setup read")
}

fn read_setup(
    state: &mut SessionOwnerState,
    run: RunAttempt<LoadAttempt>,
) -> Vec<SessionOwnerEffect> {
    apply(
        state,
        SessionOwnerEvent::Store(SessionStoreResult::RestoreSetup {
            run,
            outcome: EffectOutcome::Applied,
        }),
    )
}

fn restore_reply(run: RunAttempt<LoadAttempt>) -> SessionOwnerEvent {
    SessionOwnerEvent::Wire(SessionWireResult::Restore {
        run,
        outcome: WireOutcome::Confirmed(RestoredMode::Observed),
    })
}

fn restore_live(state: &mut SessionOwnerState, number: u64) -> RunAttempt<LoadAttempt> {
    let run = start_restore(state, number);
    read_setup(state, run);
    apply(state, restore_reply(run));
    apply(
        state,
        SessionOwnerEvent::Publication {
            run,
            outcome: EffectOutcome::Applied,
        },
    );
    run
}

fn start_close(
    state: &mut SessionOwnerState,
    generation: SessionGeneration,
    number: u64,
) -> RunAttempt<CloseAttempt> {
    let attempt = CloseAttempt(uuid(number));
    command(
        state,
        SessionOwnerCommand::Close {
            generation,
            attempt,
        },
    );
    RunAttempt {
        generation,
        attempt,
    }
}

fn mark_prompt(input: &OwnerInput) -> SessionOwnerEvent {
    SessionOwnerEvent::Store(SessionStoreResult::Dispatch {
        input: input.clone(),
        outcome: EffectOutcome::Applied,
    })
}

fn enqueue_prompt(state: &mut SessionOwnerState, input: &OwnerInput) -> Vec<SessionOwnerEffect> {
    command(
        state,
        SessionOwnerCommand::Prompt {
            input: input.clone(),
            admission: PromptAdmission::Auto {
                steering_supported: true,
            },
        },
    )
}

fn end_turn(state: &mut SessionOwnerState, input: &OwnerInput) -> Vec<SessionOwnerEffect> {
    apply(
        state,
        SessionOwnerEvent::Wire(SessionWireResult::Turn {
            input: input.clone(),
            outcome: WireOutcome::Confirmed(()),
        }),
    )
}

fn publication_for(generation: SessionGeneration, effects: &[SessionOwnerEffect]) -> bool {
    effects.iter().any(|effect| matches!(effect,
        SessionOwnerEffect::Publish(SessionPublication::Activated { run, .. }) if run.generation == generation
    ))
}

fn tool_forwarded(effects: &[SessionOwnerEffect], generation: SessionGeneration) -> bool {
    effects.contains(&SessionOwnerEffect::ForwardTool {
        generation,
        tool_id: "provider-tool-id".to_owned(),
    })
}

fn tool_update(generation: SessionGeneration, observation: ToolObservation) -> SessionOwnerEvent {
    SessionOwnerEvent::RoutedTool(RoutedSessionToolEvent::Observed {
        generation,
        tool_id: "provider-tool-id".to_owned(),
        observation,
    })
}

fn callback(generation: SessionGeneration) -> SessionOwnerEvent {
    SessionOwnerEvent::RoutedTool(RoutedSessionToolEvent::RoutedCallback {
        generation,
        tool_id: "provider-tool-id".to_owned(),
    })
}
