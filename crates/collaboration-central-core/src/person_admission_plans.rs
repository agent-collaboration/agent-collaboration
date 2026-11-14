//! Prepare fresh-proof reservation; the caller appends its effect to the same batch.
use crate::{
    ObjectHost,
    authorization_floor_rows::{
        integer, organization_guard, person_guard, read_organization_floor, read_person_floor,
    },
    guarded_statements::{exact, text},
    person_request::{
        AdmissionRefusal, VerifiedPersonRequest,
        request_admission::{admission_before_person, decide_admission, request_time_refusal},
    },
    workspace_decode_error::corrupt,
    workspace_row_codec::string,
};
use central_storage::{GuardedStatement, SqlValue, Statement, StoreFailure};
use workspace_model::Timestamp;

pub(crate) struct PersonAdmissionPlan {
    /// Floor guards come first, the guarded proof reservation last. If any guard
    /// misses, re-read and re-admit; a concurrent proof reservation then refuses reuse.
    pub statements: Vec<GuardedStatement>,
}

/// `now` is the caller's sole applying sample, taken while it holds the org turn.
/// No request marker is written until the caller commits this plan with its effect.
pub(crate) async fn prepare_person_admission(
    host: &impl ObjectHost,
    request: &VerifiedPersonRequest,
    now: Timestamp,
) -> Result<Result<PersonAdmissionPlan, AdmissionRefusal>, StoreFailure> {
    let now_ms = now.timestamp_millis();
    if let Some(refusal) = request_time_refusal(request, now_ms) {
        return Ok(Err(refusal));
    }
    let organization_id = request.organization_id.as_str();
    let organization = read_organization_floor(host, organization_id).await?;
    if let Err(refusal) = admission_before_person(request, now_ms, organization.as_ref())? {
        return Ok(Err(refusal));
    }
    let person = read_person_floor(host, organization_id, &request.person_id).await?;
    let admitted = match decide_admission(
        request,
        now_ms,
        organization.as_ref(),
        person.as_ref(),
        false,
    )? {
        Ok(admitted) => admitted,
        Err(refusal) => return Ok(Err(refusal)),
    };
    let marker = host
        .read(Statement {
            sql: "SELECT id FROM organization_used_requests WHERE id=?",
            args: vec![text(request.request_id)],
        })
        .await?;
    if marker.rows.len() > 1 {
        return Err(corrupt("organization_used_requests.id"));
    }
    if let Some(row) = marker.rows.first() {
        if string(&marker, row, "id")? != request.request_id.to_string() {
            return Err(corrupt("organization_used_requests.id"));
        }
        return Ok(Err(AdmissionRefusal::RequestReused));
    }
    let organization =
        organization.ok_or_else(|| corrupt("organization_authorization_floors.id"))?;
    let person = person.ok_or_else(|| corrupt("person_organization_authorization_floors.id"))?;
    let now_ms = admitted.now_ms;
    let statements = vec![
        organization_guard(organization_id, &organization)?,
        person_guard(organization_id, &request.person_id, &person)?,
        exact(
            "INSERT INTO organization_used_requests(id,organization_id,request_time_ms,operation_id,actor_person_id,actor_session_id,intent_digest,proof_key_thumbprint,created_at,updated_at) SELECT ?1,?2,?3,?4,?5,?6,?7,?8,?9,?9 WHERE ?9<?10 AND ?3>?9-65000 AND ?3<=?9+5000 AND ?11>?9-65000 AND ?11<=?9+5000 AND (?13=0 OR (?12 IS NOT NULL AND ?12*1000<=?9 AND ?9-?12*1000<=900000)) AND ?3>(SELECT request_reject_through_ms FROM organization_authorization_floors WHERE id=?2) AND NOT EXISTS(SELECT 1 FROM organization_used_requests WHERE id=?1)",
            vec![
                text(request.request_id),
                text(organization_id),
                integer(
                    request.request_time_ms,
                    "organization_used_requests.request_time_ms",
                )?,
                request
                    .operation
                    .operation_id()
                    .map_or(SqlValue::Null, |id| text(id.as_str())),
                text(&request.person_id),
                text(&request.session_id),
                text(&request.intent_digest),
                text(&request.proof_key_thumbprint),
                integer(now_ms, "organization_used_requests.created_at")?,
                integer(
                    request.effective_expiry_ms,
                    "person_request.effective_expiry_ms",
                )?,
                integer(request.proof_time_ms, "person_request.proof_time_ms")?,
                request
                    .second_factor_at
                    .map(|at| integer(at, "person_request.second_factor_at"))
                    .transpose()?
                    .unwrap_or(SqlValue::Null),
                integer(
                    i64::from(request.operation.route().changes_state()),
                    "person_request.changes_state",
                )?,
            ],
        ),
    ];
    Ok(Ok(PersonAdmissionPlan { statements }))
}
