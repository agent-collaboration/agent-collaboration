//! Finite non-tail fact variants share the complete native-row size oracle.
use super::{
    RECORD_BYTE_BUDGET, SCHEMA_BYTE_BUDGET, TestResult, envelope, maximum_actor,
    maximum_evidence::evidence, measure_native_row, project_state,
};
use message_board::*;

fn session() -> TestResult<SessionRef> {
    let RecordedActor::Session { session } = maximum_actor()? else {
        return Err("session fixture".into());
    };
    Ok(session)
}

fn maximum_project_created(state: &ProjectState) -> TestResult<FactKind> {
    let mut directory_envelope = envelope(state, state.project_id.clone().into());
    directory_envelope.scope.project = None;
    directory_envelope.position = RecordPlacement::Fleet(FleetFeedEntry {
        position: FleetPosition::try_from(i64::MAX as u64)?,
        origin: ActorOrigin {
            installation: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            machine: MachineLabel::try_from("\"".repeat(MAX_NAME_BYTES))?,
        },
    });
    Ok(FactKind::Lifecycle(LifecycleFact::ProjectCreated {
        entry: Box::new(ProjectDirectoryEntry {
            envelope: directory_envelope,
            id: state.project_id.clone(),
            board: state.envelope.scope.board.clone().ok_or("board fixture")?,
            title: ResourceName::try_from("\"".repeat(MAX_NAME_BYTES))?,
        }),
        creating_request: RequestId::generate(),
        acting_for: Some(ActingForIdentity::Human {
            human_id: HumanId::try_from("\u{0001}".repeat(4096))?,
        }),
    }))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn maximum_non_tail_fact_variants_fit_native_storage_without_truncation() -> TestResult {
    let state = project_state()?;
    let kinds = [
        (
            "maximumEvaluatorFact",
            FactKind::EvaluatorResult {
                guard: "\0".repeat(256),
                subject: RecordRef {
                    kind: RecordKind::StandDownAcknowledgement,
                    id: RecordId::generate(),
                },
                outcome: GuardOutcome::Unavailable,
                evidence: (0..16)
                    .map(|_| evidence())
                    .collect::<TestResult<Vec<_>>>()?,
                evaluator: EvaluatorRef {
                    actor: maximum_actor()?,
                    engine: Some("\0".repeat(256)),
                    calibration: Some(CanonicalHash::try_from("f".repeat(64))?),
                },
                executor: Some(session()?),
            },
        ),
        (
            "maximumPrFact",
            FactKind::PrFact {
                repository: NormalizedOrigin::try_from(format!(
                    "a.b/{}",
                    "\u{0001}".repeat(MAX_REPOSITORY_LOCATOR_BYTES - 4)
                ))?,
                number: u64::MAX,
                kind: PrFactKind::Merged {
                    merge_commit: CommitSha::try_from("f".repeat(64))?,
                },
                recorded_by: maximum_actor()?,
            },
        ),
        (
            "maximumHookFact",
            FactKind::HookFact {
                hook: HookKind::SessionStart,
                session: session()?,
                targets: vec!["\0".repeat(1024); 64],
            },
        ),
        (
            "maximumScopeAcknowledgement",
            FactKind::ScopeAcknowledgement {
                session: session()?,
                subject: TrailSubject::Workstream(WorkstreamId::generate()),
                paths: vec!["\0".repeat(1024); 64],
                reason: "\0".repeat(16 * 1024),
            },
        ),
        (
            "maximumProjectCreatedFact",
            maximum_project_created(&state)?,
        ),
        (
            "maximumThreadResolvedFact",
            FactKind::Lifecycle(LifecycleFact::ThreadResolved {
                thread: ThreadId::generate(),
                reason: ThreadResolutionReason::try_from(
                    "\u{0001}".repeat(MAX_MESSAGE_TEXT_BYTES),
                )?,
            }),
        ),
        (
            "maximumClosedFact",
            FactKind::Lifecycle(LifecycleFact::Closed {
                position: FeedCursor {
                    epoch: EpochNumber::try_from(i64::MAX as u64)?,
                    position: FeedPosition::try_from(i64::MAX as u64)?,
                },
                reason: "\0".repeat(MAX_MESSAGE_TEXT_BYTES),
            }),
        ),
        (
            "maximumDroppedFact",
            FactKind::Lifecycle(LifecycleFact::Dropped {
                task: TaskId::generate(),
                reason: "\0".repeat(MAX_MESSAGE_TEXT_BYTES),
            }),
        ),
        (
            "maximumPhaseCompletedFact",
            FactKind::Lifecycle(LifecycleFact::PhaseCompleted {
                phase: "\0".repeat(256),
            }),
        ),
        (
            "maximumWorkstreamClosedFact",
            FactKind::Lifecycle(LifecycleFact::WorkstreamClosed {
                workstream: WorkstreamId::generate(),
                reason: CloseReason::Dropped,
            }),
        ),
        (
            "maximumArchiveSeenFact",
            FactKind::Lifecycle(LifecycleFact::ArchiveSeen {
                board: BoardId::generate(),
            }),
        ),
    ];
    let mut measured = Vec::new();
    for (label, kind) in kinds {
        let id = FactId::generate();
        let record = Record::Fact(Fact {
            envelope: envelope(&state, id.clone().into()),
            fact_id: id,
            kind,
            links: (0..64)
                .map(|_| RecordRef {
                    kind: RecordKind::StandDownAcknowledgement,
                    id: RecordId::generate(),
                })
                .collect(),
            producer_time: chrono::DateTime::<chrono::Utc>::MAX_UTC,
            anchor: Some(FeedCursor {
                epoch: EpochNumber::INITIAL,
                position: FeedPosition::try_from(1)?,
            }),
            late: false,
        });
        record.validate_bounds()?;
        let size = measure_native_row(label, &state, record).await?;
        assert!(
            size.conservative_numeric_row_bytes <= RECORD_BYTE_BUDGET,
            "{label}: {} bytes",
            size.conservative_numeric_row_bytes
        );
        assert!(size.schema_statements > 0);
        assert!(size.schema_bytes <= SCHEMA_BYTE_BUDGET);
        measured.push(size);
    }
    eprintln!(
        "BOARD_MAXIMUM_NON_TAIL_FACT_SIZES {}",
        serde_json::to_string(&measured)?
    );
    Ok(())
}
