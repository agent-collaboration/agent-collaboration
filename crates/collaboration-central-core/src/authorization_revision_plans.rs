//! Static authorization-floor batches prepared while the caller holds the org turn.
use crate::{
    ObjectHost,
    authorization_floor_rows::{
        integer, organization_guard, organization_state, person_guard, person_state,
        read_organization_floor, read_person_floor,
    },
    guarded_statements::{exact, text},
    identity_contract::AuthorizationRevision,
    person_request::floor_delivery::{FloorDelivery, FloorMutation, decide_floor_delivery},
};
use central_storage::{GuardedStatement, StoreFailure};
use workspace_model::Timestamp;

pub(crate) struct AuthorizationRevisionPlan {
    /// This is an acknowledgement only after every statement commits.
    pub delivery: FloorDelivery,
    /// A failed snapshot guard or unique insert means re-read and decide again.
    pub statements: Vec<GuardedStatement>,
}

pub(crate) async fn prepare_authorization_revision(
    host: &impl ObjectHost,
    incoming: &AuthorizationRevision,
    now: Timestamp,
) -> Result<AuthorizationRevisionPlan, StoreFailure> {
    let organization_id = incoming.organization_id().as_str();
    let organization = read_organization_floor(host, organization_id).await?;
    let person = match incoming {
        AuthorizationRevision::Person { person_id, .. } if organization.is_some() => {
            read_person_floor(host, organization_id, person_id.as_str()).await?
        }
        _ => None,
    };
    let (delivery, mutation) =
        decide_floor_delivery(incoming, organization.as_ref(), person.as_ref())?;
    let mut statements = vec![];
    if !matches!(delivery, FloorDelivery::Acknowledged(_)) {
        return Ok(AuthorizationRevisionPlan {
            delivery,
            statements,
        });
    }
    // Guard retained state even for older/equal pushes: the reply names the committed floor.
    if let Some(current) = &organization {
        statements.push(organization_guard(organization_id, current)?);
    }
    if let AuthorizationRevision::Person { person_id, .. } = incoming
        && let Some(current) = &person
    {
        statements.push(person_guard(organization_id, person_id.as_str(), current)?);
    }
    let FloorMutation::Store(revision) = mutation else {
        return Ok(AuthorizationRevisionPlan {
            delivery,
            statements,
        });
    };
    let now = integer(now.timestamp_millis(), "authorization_floors.updated_at")?;
    match &revision {
        AuthorizationRevision::Organization {
            revision,
            state,
            permission_policy_digest,
            ..
        } => {
            let state = organization_state(
                crate::person_request::floor_delivery::organization_state(*state),
            );
            if organization.is_some() {
                statements.push(exact(
                    "UPDATE organization_authorization_floors SET minimum_revision=?,state=?,permission_policy_digest=?,updated_at=? WHERE id=?",
                    vec![integer(revision.get(), "minimum_revision")?,text(state),text(permission_policy_digest.as_str()),now,text(organization_id)],
                ));
            } else {
                statements.push(exact(
                    "INSERT INTO organization_authorization_floors(id,minimum_revision,state,permission_policy_digest,request_reject_through_ms,created_at,updated_at) SELECT ?1,?2,?3,?4,0,?5,?5 WHERE NOT EXISTS(SELECT 1 FROM organization_authorization_floors WHERE id=?1)",
                    vec![text(organization_id),integer(revision.get(), "minimum_revision")?,text(state),text(permission_policy_digest.as_str()),now],
                ));
            }
        }
        AuthorizationRevision::Person {
            person_id,
            subject_id,
            revision,
            state,
            ..
        } => {
            let state = person_state(crate::person_request::floor_delivery::person_state(*state));
            if person.is_some() {
                statements.push(exact(
                    "UPDATE person_organization_authorization_floors SET minimum_revision=?,state=?,updated_at=? WHERE id=? AND organization_id=? AND person_id=?",
                    vec![integer(revision.get(), "minimum_revision")?,text(state),now,text(subject_id.as_str()),text(organization_id),text(person_id.as_str())],
                ));
            } else {
                statements.push(exact(
                    "INSERT INTO person_organization_authorization_floors(id,organization_id,person_id,minimum_revision,state,created_at,updated_at) SELECT ?1,?2,?3,?4,?5,?6,?6 WHERE NOT EXISTS(SELECT 1 FROM person_organization_authorization_floors WHERE organization_id=?2 AND person_id=?3)",
                    vec![text(subject_id.as_str()),text(organization_id),text(person_id.as_str()),integer(revision.get(), "minimum_revision")?,text(state),now],
                ));
            }
        }
    }
    Ok(AuthorizationRevisionPlan {
        delivery,
        statements,
    })
}
