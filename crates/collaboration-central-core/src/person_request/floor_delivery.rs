//! Monotonic authorization-floor policy; storage plans apply its closed mutation.
use super::request_admission::{
    OrganizationFloor, OrganizationFloorState, PersonFloor, PersonFloorState,
};
use crate::identity_contract::{
    AuthorizationRevision, FenceRevision, OrganizationFenceState, PersonFenceState,
};
use central_storage::StoreFailure;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FloorDelivery {
    Acknowledged(FenceRevision),
    Conflict,
    OrganizationFloorMissing,
}

/// The retained floor is either unchanged or replaced by the typed push.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum FloorMutation {
    Unchanged,
    Store(AuthorizationRevision),
}

pub(crate) fn organization_state(state: OrganizationFenceState) -> OrganizationFloorState {
    match state {
        OrganizationFenceState::Active => OrganizationFloorState::Active,
        OrganizationFenceState::Stopped => OrganizationFloorState::Stopped,
        OrganizationFenceState::Deleted => OrganizationFloorState::Deleted,
    }
}

pub(crate) fn person_state(state: PersonFenceState) -> PersonFloorState {
    match state {
        PersonFenceState::Active => PersonFloorState::Active,
        PersonFenceState::Inactive => PersonFloorState::Inactive,
    }
}

pub(crate) fn decide_floor_delivery(
    incoming: &AuthorizationRevision,
    organization: Option<&OrganizationFloor>,
    person: Option<&PersonFloor>,
) -> Result<(FloorDelivery, FloorMutation), StoreFailure> {
    let unchanged = |delivery| Ok((delivery, FloorMutation::Unchanged));
    let store = |revision| {
        Ok((
            FloorDelivery::Acknowledged(revision),
            FloorMutation::Store(incoming.clone()),
        ))
    };
    let retained = |revision: super::FloorRevision| {
        FenceRevision::try_from(revision.get()).map_err(|_| {
            crate::workspace_decode_error::corrupt("authorization_floors.minimum_revision")
        })
    };
    match incoming {
        AuthorizationRevision::Organization {
            revision,
            state,
            permission_policy_digest,
            ..
        } => {
            let Some(current) = organization else {
                return store(*revision);
            };
            let current_revision = retained(current.revision)?;
            if *revision == current_revision {
                return unchanged(
                    if current.state == organization_state(*state)
                        && current.policy_digest == permission_policy_digest.as_str()
                    {
                        FloorDelivery::Acknowledged(current_revision)
                    } else {
                        FloorDelivery::Conflict
                    },
                );
            }
            if *revision < current_revision || current.state == OrganizationFloorState::Deleted {
                return unchanged(FloorDelivery::Acknowledged(current_revision));
            }
            store(*revision)
        }
        AuthorizationRevision::Person {
            subject_id,
            revision,
            state,
            ..
        } => {
            if organization.is_none() {
                return unchanged(FloorDelivery::OrganizationFloorMissing);
            }
            let Some(current) = person else {
                return store(*revision);
            };
            if current.subject_id != subject_id.as_str() {
                return unchanged(FloorDelivery::Conflict);
            }
            let current_revision = retained(current.revision)?;
            if *revision == current_revision {
                return unchanged(if current.state == person_state(*state) {
                    FloorDelivery::Acknowledged(current_revision)
                } else {
                    FloorDelivery::Conflict
                });
            }
            if *revision < current_revision {
                return unchanged(FloorDelivery::Acknowledged(current_revision));
            }
            store(*revision)
        }
    }
}

#[cfg(test)]
#[path = "floor_delivery_test.rs"]
mod tests;
