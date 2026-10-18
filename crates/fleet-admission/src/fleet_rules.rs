//! Decide against a supplied transactional read set, never against IO or a clock.
use crate::{
    FleetAdmissionError, FleetDecision, FleetFeedEntry, FleetIds, FleetOperation, FleetRequest,
    FleetSnapshot, FleetWrite, canonical_hash, fleet_decision::envelope,
};
use chrono::{DateTime, Utc};
use message_board::*;
use sha2::{Digest, Sha256};

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
    ids: &FleetIds,
) -> Result<FleetDecision, FleetAdmissionError> {
    let hash = canonical_hash(request)?;
    if let Some(ledger) = &snapshot.ledger {
        if ledger.request_id != request.request_id {
            return Err(FleetAdmissionError::InvalidSnapshot);
        }
        if ledger.canonical_hash != hash {
            return Err(FleetAdmissionError::ConflictingRequest);
        }
        return Ok(FleetDecision {
            decision: ledger.decision.clone(),
            writes: Vec::new(),
            feed: Vec::new(),
        });
    }
    let plan = match &request.operation {
        FleetOperation::WorkspaceCreate { .. } | FleetOperation::WorkspaceArchive {} => {
            crate::fleet_metadata_rules::workspace_plan(request, snapshot, accepted_at)
        }
        _ => match workspace(snapshot, &request.workspace) {
            Err(reason) => Err(reason),
            Ok(parent) if parent.state != WorkspaceState::Active => Err(Refusal::Archived),
            Ok(_) => match &request.operation {
                FleetOperation::AdrCreate { .. } | FleetOperation::AdrTransition { .. } => {
                    crate::fleet_adr_rules::adr_plan(request, snapshot, accepted_at)
                }
                FleetOperation::DefinitionRegister { definition } => {
                    definition_plan(request, snapshot, definition, accepted_at, ids)
                }
                _ => crate::fleet_metadata_rules::metadata_plan(request, snapshot, accepted_at),
            },
        },
    };
    finish(request, snapshot, accepted_at, hash, plan)
}

fn finish(
    request: &FleetRequest,
    snapshot: &FleetSnapshot,
    accepted_at: DateTime<Utc>,
    hash: CanonicalHash,
    plan: Result<OperationPlan, Refusal>,
) -> Result<FleetDecision, FleetAdmissionError> {
    let mut position = snapshot.high_water;
    let mut writes = Vec::new();
    let mut feed = Vec::new();
    let decision = match plan {
        Err(reason) => RequestDecision::Rejected {
            reasons: vec![reason],
        },
        Ok(mut plan) => {
            if plan.change == ChangeOutcome::Changed {
                for record in &mut plan.records {
                    position = position.successor()?;
                    let envelope = record.envelope_mut();
                    envelope.position = RecordPosition::Fleet(position);
                    envelope.request = Some(request.request_id.clone());
                    envelope.actor = request.actor.clone();
                    envelope.router_time = accepted_at;
                    writes.push(FleetWrite::Put(Box::new(record.clone())));
                    feed.push(FleetFeedEntry {
                        position,
                        record: record.clone(),
                    });
                }
                writes.extend(plan.extra);
            }
            RequestDecision::Accepted {
                records: plan.records.iter().map(Record::reference).collect(),
                positions: plan
                    .records
                    .iter()
                    .map(|record| record.envelope().position)
                    .collect(),
                change: plan.change,
            }
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
        ),
        request_id: request.request_id.clone(),
        canonical_hash: hash,
        observed_epoch: None,
        accepted_in: None,
        decision: decision.clone(),
        resubmits: request.resubmits.clone(),
    };
    writes.push(FleetWrite::Ledger(Box::new(ledger.clone())));
    feed.push(FleetFeedEntry {
        position,
        record: Record::Request(ledger),
    });
    Ok(FleetDecision {
        decision,
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

fn definition_plan(
    request: &FleetRequest,
    snapshot: &FleetSnapshot,
    definition: &crate::ProcessDefinitionInput,
    accepted_at: DateTime<Utc>,
    ids: &FleetIds,
) -> Result<OperationPlan, Refusal> {
    if format!("{:x}", Sha256::digest(definition.source.as_bytes()))
        != definition.reference.source_hash.as_str()
    {
        return Err(Refusal::InvalidShape {
            field: "sourceHash".into(),
        });
    }
    let parsed =
        ProcessDefinitionSource::parse(&definition.source).map_err(|_| Refusal::InvalidShape {
            field: "source".into(),
        })?;
    if parsed
        .edges
        .iter()
        .any(|edge| edge.required_role != AuthorityRole::Lead)
    {
        return Err(Refusal::InvalidShape {
            field: "requiredRole".into(),
        });
    }
    if let Some(existing) = snapshot.definitions.iter().find(|row| {
        row.reference.name == definition.reference.name
            && row.reference.version == definition.reference.version
    }) {
        if existing.reference.source_hash != definition.reference.source_hash {
            return Err(Refusal::DefinitionConflict);
        }
        return Ok(OperationPlan::unchanged(Record::ProcessDefinition(
            existing.clone(),
        )));
    }
    Ok(OperationPlan::changed(Record::ProcessDefinition(
        ProcessDefinition {
            envelope: envelope(
                request,
                ids.generated_record.clone(),
                accepted_at,
                FleetPosition::ZERO,
                None,
            ),
            reference: definition.reference.clone(),
            source: definition.source.clone(),
            initial_status: parsed.initial_status,
            edges: parsed.edges,
        },
    )))
}
