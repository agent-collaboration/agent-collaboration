//! Apply one pushed fence revision to central's floors, in the Fleet's
//! transaction: a higher revision replaces, an equal one with the same payload
//! is idempotent, an older one never lowers the floor, and a deleted
//! organization never resumes. The acknowledgement is the floor retained.
use super::request_admission::{
    OrganizationFloor, OrganizationFloorState, PersonFloorState, read_organization_floor,
    read_person_floor,
};
use crate::generated_organization as queries;
use crate::identity_contract::{
    AuthorizationRevision, FenceRevision, OrganizationFenceState, PersonFenceState,
};
use durable_steps::{StoreError, storage::TransactionScope};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FloorDelivery {
    Acknowledged(FenceRevision),
    /// The same revision with a different payload, or another subject for the pair.
    Conflict,
    /// A person floor arrived before its organization's floor.
    OrganizationFloorMissing,
}

fn organization_state(state: OrganizationFenceState) -> (&'static str, OrganizationFloorState) {
    match state {
        OrganizationFenceState::Active => ("active", OrganizationFloorState::Active),
        OrganizationFenceState::Stopped => ("stopped", OrganizationFloorState::Stopped),
        OrganizationFenceState::Deleted => ("deleted", OrganizationFloorState::Deleted),
    }
}

fn person_state(state: PersonFenceState) -> (&'static str, PersonFloorState) {
    match state {
        PersonFenceState::Active => ("active", PersonFloorState::Active),
        PersonFenceState::Inactive => ("inactive", PersonFloorState::Inactive),
    }
}

fn retained(revision: super::FloorRevision) -> Result<FenceRevision, StoreError> {
    FenceRevision::try_from(revision.get())
        .map_err(|_| StoreError::backend("retained floor revision out of range"))
}

pub(crate) async fn deliver<S: TransactionScope>(
    scope: &mut S,
    revision: &AuthorizationRevision,
    now_ms: i64,
) -> Result<FloorDelivery, StoreError>
where
    <S::Database as Database>::Arguments: IntoArguments<S::Database>,
    for<'v> String: Type<S::Database> + Encode<'v, S::Database>,
    for<'v> Option<String>: Type<S::Database> + Decode<'v, S::Database>,
    for<'r> &'r str: ColumnIndex<<S::Database as Database>::Row>,
{
    let now = now_ms.to_string();
    match revision {
        AuthorizationRevision::Organization {
            organization_id,
            revision,
            state,
            permission_policy_digest,
        } => {
            let organization = organization_id.as_str();
            let (state_text, state) = organization_state(*state);
            let Some(current) = read_organization_floor(scope, organization).await? else {
                // The first floor starts the replay watermark at zero.
                queries::insert_organization_authorization_floor(
                    scope,
                    organization.into(),
                    revision.get().to_string(),
                    state_text.into(),
                    permission_policy_digest.as_str().into(),
                    "0".into(),
                    now,
                )
                .await
                .map_err(StoreError::backend)?;
                return Ok(FloorDelivery::Acknowledged(*revision));
            };
            let OrganizationFloor {
                revision: current_revision,
                state: current_state,
                policy_digest,
                ..
            } = &current;
            let current_revision = retained(*current_revision)?;
            if *revision == current_revision {
                return Ok(
                    if *current_state == state && policy_digest == permission_policy_digest.as_str()
                    {
                        FloorDelivery::Acknowledged(current_revision)
                    } else {
                        FloorDelivery::Conflict
                    },
                );
            }
            if *revision < current_revision || *current_state == OrganizationFloorState::Deleted {
                return Ok(FloorDelivery::Acknowledged(current_revision));
            }
            queries::update_organization_authorization_floor(
                scope,
                organization.into(),
                revision.get().to_string(),
                state_text.into(),
                permission_policy_digest.as_str().into(),
                now,
            )
            .await
            .map_err(StoreError::backend)?;
            Ok(FloorDelivery::Acknowledged(*revision))
        }
        AuthorizationRevision::Person {
            organization_id,
            person_id,
            subject_id,
            revision,
            state,
        } => {
            let organization = organization_id.as_str();
            if read_organization_floor(scope, organization)
                .await?
                .is_none()
            {
                return Ok(FloorDelivery::OrganizationFloorMissing);
            }
            let (state_text, state) = person_state(*state);
            let Some(current) = read_person_floor(scope, organization, person_id.as_str()).await?
            else {
                queries::insert_person_authorization_floor(
                    scope,
                    subject_id.as_str().into(),
                    organization.into(),
                    person_id.as_str().into(),
                    revision.get().to_string(),
                    state_text.into(),
                    now,
                )
                .await
                .map_err(StoreError::backend)?;
                return Ok(FloorDelivery::Acknowledged(*revision));
            };
            if current.subject_id != subject_id.as_str() {
                return Ok(FloorDelivery::Conflict);
            }
            let current_revision = retained(current.revision)?;
            if *revision == current_revision {
                return Ok(if current.state == state {
                    FloorDelivery::Acknowledged(current_revision)
                } else {
                    FloorDelivery::Conflict
                });
            }
            if *revision < current_revision {
                return Ok(FloorDelivery::Acknowledged(current_revision));
            }
            queries::update_person_authorization_floor(
                scope,
                subject_id.as_str().into(),
                revision.get().to_string(),
                state_text.into(),
                now,
            )
            .await
            .map_err(StoreError::backend)?;
            Ok(FloorDelivery::Acknowledged(*revision))
        }
    }
}

#[cfg(test)]
#[path = "floor_delivery_test.rs"]
mod tests;
