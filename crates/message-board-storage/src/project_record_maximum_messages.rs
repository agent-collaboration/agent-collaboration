//! Message and subscription maxima retain every typed identity and optional field.
use super::{
    RECORD_BYTE_BUDGET, SCHEMA_BYTE_BUDGET, TestResult, envelope, maximum_actor,
    measure_native_row, project_state,
};
use message_board::*;

fn maximum_cursor() -> TestResult<FeedCursor> {
    Ok(FeedCursor {
        epoch: EpochNumber::try_from(i64::MAX as u64)?,
        position: FeedPosition::try_from(i64::MAX as u64)?,
    })
}

fn project_subject(character: char) -> TestResult<ThreadSubject> {
    Ok(ThreadSubject::Project {
        line: SubjectLine::try_from(character.to_string().repeat(120))?,
    })
}

fn maximum_thread(state: &ProjectState) -> TestResult<Record> {
    let id = ThreadId::generate();
    let orchestrator = maximum_actor()?
        .declared_identity()
        .ok_or("session fixture")?;
    let RecordedActor::Session { mut session } = maximum_actor()? else {
        return Err("session fixture".into());
    };
    session.session_id = SessionId::try_from("\u{0002}".repeat(4096))?;
    Ok(Record::Thread(Thread {
        envelope: envelope(state, id.clone().into()),
        thread_id: id,
        about: project_subject('\u{0001}')?,
        imported: false,
        state: ThreadState::Resolved,
        orchestrator: Some(OrchestratorHolder {
            identity: orchestrator,
            last_seen_position: maximum_cursor()?,
        }),
        implementer: Some(ImplementerHolder {
            identity: Identity::Session { session },
            last_seen_position: maximum_cursor()?,
        }),
    }))
}

fn maximum_refiling(state: &ProjectState) -> TestResult<Record> {
    Ok(Record::ThreadRefiled(ThreadRefiled {
        envelope: envelope(state, RecordId::generate()),
        thread: ThreadId::generate(),
        from: project_subject('\u{0001}')?,
        to: project_subject('\u{0002}')?,
    }))
}

fn maximum_intent(state: &ProjectState) -> TestResult<Record> {
    let id = NotifyIntentId::generate();
    Ok(Record::NotifyIntent(NotifyIntent::new(
        NotifyIntentProps {
            envelope: envelope(state, id.clone().into()),
            intent_id: id,
            cause: RecordRef {
                kind: RecordKind::StandDownAcknowledgement,
                id: RecordId::generate(),
            },
            recipient: IntentRecipient::Resolved(ResolvedRecipient {
                actor: maximum_actor()?,
                via_role: Some(RoleRecipient {
                    subject: RoleSubject::Workstream(WorkstreamId::generate()),
                    role: AuthorityRole::Implementer,
                    assignment_revision: AssignmentRevision::try_from(i64::MAX as u64)?,
                }),
            }),
            state: IntentState::Refused {
                reason: IntentRefusal::Delivery {
                    detail: "\0".repeat(16 * 1024),
                },
            },
        },
    )?))
}

fn maximum_membership(state: &ProjectState) -> TestResult<Record> {
    let reader = maximum_actor()?
        .declared_identity()
        .ok_or("session fixture")?;
    let RecordPlacement::Project(position) = &state.envelope.position else {
        return Err("project placement fixture".into());
    };
    let mut ended_at = maximum_cursor()?;
    // The membership constructor requires the ending epoch to match its entry.
    ended_at.epoch = position.cursor.epoch;
    Ok(Record::SubscriptionMembership(SubscriptionMembership::new(
        SubscriptionMembershipProps {
            envelope: envelope(state, RecordId::generate()),
            reader: reader.clone(),
            scope: SubscriptionScope::project(state.project_id.clone()),
            policy: SubscriptionPolicy::new(
                &reader,
                SubscriptionMode::Deliver,
                WhenIdle::Hold,
                BatchTiming::new(MAX_SUBSCRIPTION_QUIET_SECONDS, MAX_SUBSCRIPTION_CAP_SECONDS)?,
                SubscriptionLifetime::new(MAX_SUBSCRIPTION_LIFETIME_SECONDS)?,
            )?,
            renewed_at: chrono::DateTime::<chrono::Utc>::MAX_UTC - chrono::Duration::seconds(1),
            expires_at: chrono::DateTime::<chrono::Utc>::MAX_UTC,
            generation: SubscriptionGeneration::new(i64::MAX as u64)?,
            activated_at: maximum_cursor()?,
            ended: Some(MembershipEnd {
                reason: EndReason::Cancelled,
                position: ended_at,
                at: chrono::DateTime::<chrono::Utc>::MAX_UTC,
            }),
        },
    )?))
}

#[tokio::test]
async fn maximum_message_and_membership_rows_fit_native_storage_without_truncation() -> TestResult {
    // The native owner and real participation rows reconstruct the current
    // thread holders. Captured history must retain the same complete body.
    let state = project_state()?;
    let cases = [
        ("maximumThread", maximum_thread(&state)?),
        ("maximumThreadRefiled", maximum_refiling(&state)?),
        (
            "maximumTrailRedaction",
            Record::TrailRedaction(TrailRedaction {
                envelope: envelope(&state, RecordId::generate()),
                row: WorkTrailRowId::generate(),
                redacted: false,
            }),
        ),
        ("maximumNotifyIntent", maximum_intent(&state)?),
        ("maximumSubscriptionMembership", maximum_membership(&state)?),
    ];
    let mut measured = Vec::new();
    for (kind, record) in cases {
        record.validate_bounds()?;
        let size = measure_native_row(kind, &state, record).await?;
        assert!(size.schema_statements > 0);
        assert!(size.schema_bytes <= SCHEMA_BYTE_BUDGET);
        assert!(
            size.conservative_numeric_row_bytes <= RECORD_BYTE_BUDGET,
            "{kind}: {} bytes",
            size.conservative_numeric_row_bytes
        );
        measured.push(size);
    }
    eprintln!(
        "BOARD_MAXIMUM_MESSAGE_SIZES {}",
        serde_json::to_string(&measured)?
    );
    Ok(())
}
