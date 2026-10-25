use crate::{
    FleetOperation, FleetRequest, FleetSnapshot, fleet_decision::envelope,
    fleet_rules::OperationPlan,
};
use chrono::{DateTime, Utc};
use message_board::*;

fn adr<'snapshot>(
    snapshot: &'snapshot FleetSnapshot,
    request: &FleetRequest,
    id: &AdrId,
) -> Result<&'snapshot Adr, Refusal> {
    match snapshot.record(&id.clone().into()) {
        Some(Record::Adr(row)) if row.workspace == request.workspace => Ok(row),
        _ => Err(Refusal::NotFound(id.clone().into())),
    }
}

pub(crate) fn adr_plan(
    request: &FleetRequest,
    snapshot: &FleetSnapshot,
    now: DateTime<Utc>,
    origin: &ActorOrigin,
) -> Result<OperationPlan, Refusal> {
    match &request.operation {
        FleetOperation::AdrCreate {
            adr: id,
            title,
            text,
            supersedes,
            made_in,
            repository_files,
        } => {
            if snapshot.record(&id.clone().into()).is_some() {
                return Err(Refusal::AlreadyExists(id.clone().into()));
            }
            if let Some(previous) = supersedes {
                adr(snapshot, request, previous)?;
            }
            Ok(OperationPlan::changed(Record::Adr(Adr {
                envelope: envelope(
                    request,
                    id.clone().into(),
                    now,
                    FleetPosition::ZERO,
                    None,
                    origin,
                ),
                adr_id: id.clone(),
                workspace: request.workspace.clone(),
                title: title.clone(),
                text: text.clone(),
                status: AdrStatus::Proposed,
                supersedes: supersedes.clone(),
                made_in: made_in.clone(),
                repository_files: repository_files.clone(),
                revision: SubjectRevision::INITIAL,
            })))
        }
        FleetOperation::AdrTransition {
            adr: id,
            transition,
            expected_revision,
        } => {
            let previous = adr(snapshot, request, id)?;
            if previous.revision != *expected_revision {
                return Err(Refusal::StaleRevision {
                    current: previous.revision,
                });
            }
            let target = match transition {
                AdrTransition::Accept {}
                    if previous.status == AdrStatus::Proposed
                        || previous.status == AdrStatus::Accepted =>
                {
                    AdrStatus::Accepted
                }
                AdrTransition::Withdraw {}
                    if previous.status == AdrStatus::Proposed
                        || previous.status == AdrStatus::Withdrawn =>
                {
                    AdrStatus::Withdrawn
                }
                AdrTransition::Supersede { by }
                    if previous.status == AdrStatus::Accepted
                        || previous.status == AdrStatus::Superseded =>
                {
                    let replacement = adr(snapshot, request, by)?;
                    if replacement.adr_id == previous.adr_id
                        || replacement.status != AdrStatus::Accepted
                        || replacement.supersedes.as_ref() != Some(&previous.adr_id)
                    {
                        return Err(Refusal::InvalidShape {
                            field: "supersedes".into(),
                        });
                    }
                    AdrStatus::Superseded
                }
                _ => {
                    return Err(Refusal::Closed {
                        subject: id.clone().into(),
                    });
                }
            };
            if target == previous.status {
                return Ok(OperationPlan::unchanged(Record::Adr(previous.clone())));
            }
            let mut updated = previous.clone();
            updated.status = target;
            updated.revision =
                previous
                    .revision
                    .successor()
                    .map_err(|_| Refusal::InvalidShape {
                        field: "revision".into(),
                    })?;
            Ok(OperationPlan::changed(Record::Adr(updated)))
        }
        _ => Err(Refusal::InvalidShape {
            field: "operation".into(),
        }),
    }
}
