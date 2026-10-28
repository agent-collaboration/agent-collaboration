use message_board::*;
use serde_json::json;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn not_recorded_is_a_closed_interval_value_and_never_a_live_join_role()
-> Result<(), Box<dyn std::error::Error>> {
    let joined = FeedCursor {
        epoch: EpochNumber::INITIAL,
        position: FeedPosition::try_from(1)?,
    };
    let closed = FeedCursor {
        position: FeedPosition::try_from(2)?,
        ..joined
    };
    let props = ParticipantProps {
        identity: Identity::Human {
            human_id: HumanId::try_from("synthetic reader".to_owned())?,
        },
        role: RecordedParticipantRole::NotRecorded,
        note: None,
        joined_at_position: joined,
        last_seen_position: closed,
        closed_at_position: Some(closed),
        closed_reason: Some(ParticipantClosedReason::Left),
        replaced_by: None,
    };
    let interval = Participant::new(props.clone())?;
    let encoded = serde_json::to_value(&interval)?;
    assert_eq!(encoded["role"], json!({"kind":"notRecorded"}));
    assert_eq!(serde_json::from_value::<Participant>(encoded)?, interval);
    assert!(
        Participant::new(ParticipantProps {
            last_seen_position: joined,
            closed_at_position: None,
            closed_reason: None,
            ..props
        })
        .is_err()
    );

    // The live operation's role stays the existing enum, with no import alias.
    let operation = BoardOperation::ThreadJoin {
        thread_id: ThreadId::generate(),
        role: ParticipantRole::Reviewer,
        watch: false,
        mode: None,
        when_idle: None,
        replace: None,
        note: None,
    };
    let mut incoming = serde_json::to_value(operation)?;
    incoming["role"] = json!({"kind":"notRecorded"});
    assert!(serde_json::from_value::<BoardOperation>(incoming).is_err());
    assert!(serde_json::from_value::<ParticipantRole>(json!("notRecorded")).is_err());
    let request = ThreadJoinRequest {
        request_id: RequestId::generate(),
        protocol_version: ProtocolVersion::CURRENT,
        thread: ThreadId::generate().into(),
        actor: interval.identity,
        role: ParticipantRole::Reviewer,
        watch: false,
        mode: None,
        when_idle: None,
        replace: None,
        note: None,
    };
    let mut wire = serde_json::to_value(request)?;
    wire["role"] = json!({"kind":"notRecorded"});
    assert!(serde_json::from_value::<ThreadJoinRequest>(wire).is_err());
    Ok(())
}
