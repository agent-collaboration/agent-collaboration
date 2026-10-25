//! Combined evidence maxima include every repeated repository and identity.
use super::{
    RECORD_BYTE_BUDGET, TestResult, envelope, maximum_actor, measure_native_row, project_state,
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
async fn maximum_receipt_rows_fit_the_adopted_record_budget() -> TestResult {
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
    receipt.validate_bounds()?;
    let size = measure_native_row("maximumReceipt", &state, receipt).await?;
    eprintln!(
        "BOARD_MAXIMUM_EVIDENCE_SIZES {}",
        serde_json::to_string(std::slice::from_ref(&size))?
    );
    assert!(
        size.conservative_numeric_row_bytes <= RECORD_BYTE_BUDGET,
        "{}: {} bytes exceeds {}",
        size.kind,
        size.conservative_numeric_row_bytes,
        RECORD_BYTE_BUDGET
    );
    Ok(())
}
