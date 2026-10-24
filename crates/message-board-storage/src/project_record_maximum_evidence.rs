//! Combined evidence maxima include every repeated repository and identity.
use super::{
    RECORD_BYTE_BUDGET, TestResult, envelope, ledger, maximum_actor, measure_native_row,
    project_state,
};
use message_board::*;

fn repository() -> TestResult<NormalizedOrigin> {
    Ok(NormalizedOrigin::try_from(format!(
        "a.b/{}",
        "\u{0001}".repeat(4092)
    ))?)
}
fn artifact() -> TestResult<ArtifactRef> {
    Ok(ArtifactRef {
        repository: repository()?,
        path: "\0".repeat(1024),
        commit: Some(CommitSha::try_from("f".repeat(64))?),
    })
}
pub(super) fn evidence() -> TestResult<EvidenceRef> {
    Ok(EvidenceRef::Artifact {
        repository: repository()?,
        path: "\0".repeat(1024),
        commit: CommitSha::try_from("f".repeat(64))?,
    })
}

#[tokio::test]
async fn maximum_receipt_and_move_rows_fit_the_adopted_record_budget() -> TestResult {
    let mut state = project_state()?;
    if let RecordPlacement::Project(position) = &mut state.envelope.position {
        position.origin.machine = MachineLabel::try_from("\"".repeat(256))?;
    }
    let fact_id = FactId::generate();
    let actor = maximum_actor()?;
    let executor = match &actor {
        RecordedActor::Session { session } => session.clone(),
        _ => return Err("session fixture".into()),
    };
    let receipt = Record::Fact(Fact {
        envelope: envelope(&state, fact_id.clone().into()),
        fact_id,
        kind: FactKind::Receipt(Box::new(ReceiptFact {
            task: TaskId::generate(),
            obligation: "\0".repeat(4096),
            head: CommitSha::try_from("f".repeat(64))?,
            tree: Some(CommitSha::try_from("f".repeat(64))?),
            contract_revision: CanonicalHash::try_from("f".repeat(64))?,
            verifier: actor.clone(),
            scenario_run: "\0".repeat(256),
            observed: "\0".repeat(8192),
            outcome: ReceiptOutcome::NotVerified,
            commands: (0..16)
                .map(|_| CommandRun {
                    command: "\0".repeat(1024),
                    exit: i32::MIN,
                })
                .collect(),
            artifacts: (0..16)
                .map(|_| artifact())
                .collect::<TestResult<Vec<_>>>()?,
            commission: Some(MoveId::generate()),
            executor: Some(executor),
        })),
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
    let move_id = MoveId::generate();
    let moved = Record::Move(MoveRecord::new(MoveRecordProps {
        envelope: envelope(&state, move_id.clone().into()),
        move_id,
        project: state.project_id.clone(),
        move_name: MoveName::try_from("\"".repeat(256))?,
        endpoints: Some(MoveEndpoints {
            from: StatusName::try_from("\"".repeat(256))?,
            to: StatusName::try_from("\"".repeat(256))?,
        }),
        evidence: (0..16)
            .map(|_| evidence())
            .collect::<TestResult<Vec<_>>>()?,
        reason: "\0".repeat(16 * 1024),
        requester: actor.clone(),
        outcome: MoveOutcome::Accepted {},
    })?);
    let mut rejected: MoveRecordProps = match moved.clone() {
        Record::Move(row) => row.into(),
        _ => return Err("move fixture".into()),
    };
    let mut reasons = vec![
        RejectReason::Archived {},
        RejectReason::Closed {},
        RejectReason::StaleRevision {
            current: SubjectRevision::try_from(i64::MAX as u64)?,
        },
        RejectReason::MoveNotAllowed {
            edge: MoveName::try_from("\"".repeat(256))?,
            required_role: Some(AuthorityRole::Implementer),
        },
    ];
    let evaluator = actor.declared_identity().ok_or("session fixture")?;
    reasons.extend(
        (0..MAX_EDGE_GUARDS).map(|_| RejectReason::GuardUnavailable {
            guard: "\0".repeat(256),
            evaluator: evaluator.clone(),
        }),
    );
    rejected.outcome = MoveOutcome::Rejected {
        reasons: reasons.clone(),
    };
    let rejected_ledger = ledger(
        &state,
        StoredDecision::Rejected {
            reasons: reasons.into_iter().map(Refusal::Move).collect(),
        },
    )?;
    let mut sizes = Vec::new();
    for (kind, record) in [
        ("maximumReceipt", receipt),
        ("maximumMoveAccepted", moved),
        (
            "maximumMoveRejected",
            Record::Move(MoveRecord::new(rejected)?),
        ),
        ("maximumRejectedDecision", rejected_ledger),
    ] {
        record.validate_bounds()?;
        sizes.push(measure_native_row(kind, &state, record).await?);
    }
    eprintln!(
        "BOARD_MAXIMUM_EVIDENCE_SIZES {}",
        serde_json::to_string(&sizes)?
    );
    for size in sizes {
        assert!(
            size.conservative_numeric_row_bytes <= RECORD_BYTE_BUDGET,
            "{}: {} bytes exceeds {}",
            size.kind,
            size.conservative_numeric_row_bytes,
            RECORD_BYTE_BUDGET
        );
    }
    Ok(())
}
