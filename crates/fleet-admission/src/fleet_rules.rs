//! Decide against a supplied transactional read set, never against IO or a clock.
use crate::{
    FleetAdmissionError, FleetDecision, FleetFeedEntry, FleetOperation, FleetRequest,
    FleetSnapshot, FleetWrite, canonical_hash, fleet_decision::envelope,
};
use chrono::{DateTime, Utc};
use message_board::*;

pub(crate) struct OperationPlan {
    pub records: Vec<Record>,
    pub extra: Vec<FleetWrite>,
    pub change: ChangeOutcome,
}
impl OperationPlan {
    pub(crate) fn changed(record: Record) -> Self {
        Self {
            records: vec![record],
            extra: Vec::new(),
            change: ChangeOutcome::Changed,
        }
    }
    pub(crate) fn unchanged(record: Record) -> Self {
        Self {
            records: vec![record],
            extra: Vec::new(),
            change: ChangeOutcome::AlreadySo,
        }
    }
}

pub fn decide(
    request: &FleetRequest,
    snapshot: &FleetSnapshot,
    accepted_at: DateTime<Utc>,
    origin: &ActorOrigin,
) -> Result<FleetDecision, FleetAdmissionError> {
    if matches!(&request.actor, RecordedActor::Router { installation, .. }
        if installation != &origin.installation)
    {
        return Err(FleetAdmissionError::InvalidActorOrigin);
    }
    let hash = canonical_hash(request)?;
    if let Some(ledger) = &snapshot.ledger {
        if ledger.request_id != request.request_id {
            return Err(FleetAdmissionError::InvalidSnapshot);
        }
        if ledger.canonical_hash != hash {
            return Err(FleetAdmissionError::ConflictingRequest);
        }
        return Ok(FleetDecision {
            decision: ledger
                .decision
                .clone()
                .into_reply(snapshot.decision_receipts.clone(), None)
                .map_err(|_| FleetAdmissionError::InvalidSnapshot)?,
            writes: Vec::new(),
            feed: Vec::new(),
        });
    }
    let already_decided_creation = match &request.operation {
        FleetOperation::ProjectDirectoryCreate { project, .. }
            if snapshot
                .creation_decisions
                .iter()
                .any(|binding| &binding.project == project) =>
        {
            Some(project)
        }
        _ => None,
    };
    let plan = if let Some(project) = already_decided_creation {
        Err(Refusal::AlreadyExists(project.clone().into()))
    } else {
        match &request.operation {
            FleetOperation::WorkspaceCreate { .. } | FleetOperation::WorkspaceArchive {} => {
                crate::fleet_metadata_rules::workspace_plan(request, snapshot, accepted_at, origin)
            }
            _ => match workspace(snapshot, &request.workspace) {
                Err(reason) => Err(reason),
                Ok(parent) if parent.state != WorkspaceState::Active => Err(Refusal::Archived),
                Ok(_) => match &request.operation {
                    FleetOperation::AdrCreate { .. } | FleetOperation::AdrTransition { .. } => {
                        crate::fleet_adr_rules::adr_plan(request, snapshot, accepted_at, origin)
                    }
                    _ => crate::fleet_metadata_rules::metadata_plan(
                        request,
                        snapshot,
                        accepted_at,
                        origin,
                    ),
                },
            },
        }
    };
    finish(request, snapshot, accepted_at, hash, plan, origin)
}

fn finish(
    request: &FleetRequest,
    snapshot: &FleetSnapshot,
    accepted_at: DateTime<Utc>,
    hash: CanonicalHash,
    plan: Result<OperationPlan, Refusal>,
    origin: &ActorOrigin,
) -> Result<FleetDecision, FleetAdmissionError> {
    let mut position = snapshot.high_water;
    let plan = plan.and_then(|plan| {
        for record in &plan.records {
            record
                .validate_bounds()
                .map_err(|error| Refusal::InvalidShape {
                    field: error.field.to_owned(),
                })?;
        }
        Ok(plan)
    });
    let mut writes = Vec::new();
    let mut feed = Vec::new();
    let mut receipt_effects = Vec::new();
    let decision = match plan {
        Err(reason) => StoredDecision::Rejected {
            reasons: vec![reason],
        },
        Ok(mut plan) => {
            if plan.change == ChangeOutcome::Changed {
                for record in &mut plan.records {
                    position = position.successor()?;
                    let envelope = record.envelope_mut();
                    envelope.position = RecordPlacement::Fleet(message_board::FleetFeedEntry {
                        position,
                        origin: origin.clone(),
                    });
                    envelope.request = Some(request.request_id.clone());
                    envelope.actor = request.actor.clone();
                    envelope.time = RecordTime::Router(accepted_at);
                    writes.push(FleetWrite::Put(Box::new(record.clone())));
                    feed.push(FleetFeedEntry {
                        position,
                        record: record.clone(),
                    });
                }
                writes.extend(plan.extra);
            }
            receipt_effects = plan
                .records
                .iter()
                .map(|record| (record.reference(), record.envelope().position.clone()))
                .collect();
            StoredDecision::accepted(receipt_effects.clone(), plan.change)
        }
    };
    position = position.successor()?;
    let ledger = RequestLedgerEntry {
        envelope: envelope(
            request,
            request.request_id.clone().into(),
            accepted_at,
            position,
            None,
            origin,
        ),
        request_id: request.request_id.clone(),
        canonical_hash: hash,
        accepted_in: None,
        decision: decision.clone(),
    };
    writes.push(FleetWrite::Ledger(Box::new(ledger.clone())));
    if let FleetOperation::ProjectDirectoryCreate { project, .. } = &request.operation
        && !snapshot
            .creation_decisions
            .iter()
            .any(|binding| &binding.project == project)
    {
        writes.push(FleetWrite::BindCreation {
            project: project.clone(),
            creating_request: request.request_id.clone(),
            carrier_installation: origin.installation.clone(),
        });
    }
    for (ordinal, effect) in receipt_effects.iter().enumerate() {
        writes.push(FleetWrite::DecisionReceipt {
            request: request.request_id.clone(),
            ordinal: u64::try_from(ordinal).map_err(|_| FleetAdmissionError::InvalidSnapshot)?,
            effect: effect.clone(),
        });
    }
    feed.push(FleetFeedEntry {
        position,
        record: Record::Request(ledger),
    });
    Ok(FleetDecision {
        decision: decision
            .into_reply(receipt_effects, None)
            .map_err(|_| FleetAdmissionError::InvalidSnapshot)?,
        writes,
        feed,
    })
}

pub(crate) fn workspace<'snapshot>(
    snapshot: &'snapshot FleetSnapshot,
    id: &WorkspaceId,
) -> Result<&'snapshot Workspace, Refusal> {
    match snapshot.record(&id.clone().into()) {
        Some(Record::Workspace(row)) => Ok(row),
        _ => Err(Refusal::NotFound(id.clone().into())),
    }
}

pub(crate) fn board<'snapshot>(
    snapshot: &'snapshot FleetSnapshot,
    request: &FleetRequest,
    id: &BoardId,
) -> Result<&'snapshot Board, Refusal> {
    match snapshot.record(&id.clone().into()) {
        Some(Record::Board(row)) if row.workspace_id == request.workspace => Ok(row),
        _ => Err(Refusal::NotFound(id.clone().into())),
    }
}

pub(crate) fn name_conflict(
    snapshot: &FleetSnapshot,
    scope: &crate::FleetNameScope,
    name: &ResourceName,
    except: Option<&RecordRef>,
) -> Option<Refusal> {
    snapshot
        .names
        .iter()
        .find(|owner| owner.scope == *scope && owner.name == *name && Some(&owner.record) != except)
        .map(|owner| Refusal::NameConflict {
            name: name.clone(),
            existing: owner.record.clone(),
        })
}
