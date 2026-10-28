//! Owned test data for the same checked receipt rule on each real store.

use crate::ConformanceResult;
use message_board::{
    ActorOrigin, BoardId, CanonicalHash, ChangeOutcome, DecisionEffect, FleetFeedEntry,
    FleetPosition, HumanId, MachineLabel, RecordEnvelope, RecordId, RecordPlacement, RecordScope,
    RecordTime, RecordedActor, RequestId, RequestLedgerEntry, ServiceId, StoredDecision,
    WorkspaceId,
};

pub fn receipt_fixture(request_id: RequestId) -> ConformanceResult<RequestLedgerEntry> {
    Ok(RequestLedgerEntry {
        envelope: RecordEnvelope {
            id: RecordId::try_from(request_id.as_str().to_owned())?,
            scope: RecordScope {
                workspace: WorkspaceId::try_from(
                    "019f0000-0000-7000-8000-000000000003".to_owned(),
                )?,
                board: None,
                project: None,
            },
            actor: RecordedActor::Human {
                human_id: HumanId::try_from("fixture-owner".to_owned())?,
            },
            request: Some(request_id.clone()),
            position: RecordPlacement::Fleet(FleetFeedEntry {
                position: FleetPosition::try_from(1)?,
                origin: ActorOrigin {
                    installation: ServiceId::try_from(
                        "019f0000-0000-7000-8000-000000000004".to_owned(),
                    )?,
                    machine: MachineLabel::try_from("fixture".to_owned())?,
                },
            }),
            time: RecordTime::Router("2026-10-18T00:00:00Z".parse()?),
        },
        request_id,
        canonical_hash: CanonicalHash::try_from("a".repeat(64))?,
        accepted_in: None,
        decision: StoredDecision::accepted(Vec::new(), ChangeOutcome::Changed),
    })
}

pub fn receipt_with_effects_fixture(
    request_id: RequestId,
) -> ConformanceResult<(RequestLedgerEntry, Vec<DecisionEffect>)> {
    let mut entry = receipt_fixture(request_id)?;
    let RecordPlacement::Fleet(placement) = &entry.envelope.position else {
        return Err("receipt fixture needs Fleet placement".into());
    };
    let origin = placement.origin.clone();
    let effects = vec![
        (
            WorkspaceId::try_from("019f0000-0000-7000-8000-000000000003".to_owned())?.into(),
            RecordPlacement::Fleet(FleetFeedEntry {
                position: FleetPosition::try_from(1)?,
                origin: origin.clone(),
            }),
        ),
        (
            BoardId::try_from("019f0000-0000-7000-8000-000000000005".to_owned())?.into(),
            RecordPlacement::Fleet(FleetFeedEntry {
                position: FleetPosition::try_from(2)?,
                origin: origin.clone(),
            }),
        ),
    ];
    entry.envelope.position = RecordPlacement::Fleet(FleetFeedEntry {
        position: FleetPosition::try_from(3)?,
        origin,
    });
    entry.decision = StoredDecision::accepted(effects.clone(), ChangeOutcome::Changed);
    Ok((entry, effects))
}
