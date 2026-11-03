//! Pure guard decisions consume a confirmation after the owning transaction reads.

use super::{
    ConfirmedPersonToken, PersonConfirmationRefusal, PersonConfirmationWindow, PersonTokenAudience,
    person_token::validate_token_expiry,
};
use crate::identity_contract::{
    CommandRefusal, OrganizationStanding, OwnerRefusal, PersonId, StandingRole,
};
use fleet_identity_protocol::{OrganizationId, UnixMillis, UnixSeconds};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct OwnerGuardSettings {
    confirmation_window: PersonConfirmationWindow,
    second_factor_window_seconds: u32,
}

impl Default for OwnerGuardSettings {
    fn default() -> Self {
        Self {
            confirmation_window: PersonConfirmationWindow::default(),
            second_factor_window_seconds: 900,
        }
    }
}

impl OwnerGuardSettings {
    pub fn from_windows(
        confirmation_window: PersonConfirmationWindow,
        second_factor_window_seconds: u32,
    ) -> Result<Self, PersonConfirmationRefusal> {
        if second_factor_window_seconds == 0 {
            return Err(PersonConfirmationRefusal::InvalidWindow);
        }
        Ok(Self {
            confirmation_window,
            second_factor_window_seconds,
        })
    }
}

pub struct AuthorizedOwner {
    person_id: PersonId,
}

impl AuthorizedOwner {
    #[must_use]
    pub fn person_id(&self) -> &PersonId {
        &self.person_id
    }
}

pub struct AuthorizedOperator {
    person_id: PersonId,
}

impl AuthorizedOperator {
    #[must_use]
    pub fn person_id(&self) -> &PersonId {
        &self.person_id
    }
}

/// The owner and standing arguments must come from this applying transaction.
pub fn authorize_owner_read(
    proof: ConfirmedPersonToken,
    organization: &OrganizationId,
    current_owner: Option<&PersonId>,
    caller_standing: &OrganizationStanding,
    now: UnixMillis,
    settings: OwnerGuardSettings,
) -> Result<AuthorizedOwner, OwnerRefusal> {
    let proof = current_owner_proof(
        proof,
        organization,
        current_owner,
        caller_standing,
        now,
        settings,
    )?;
    Ok(AuthorizedOwner {
        person_id: proof.token().person_id().clone(),
    })
}

/// Check the owner before checking factors or reading any mutation subject.
pub fn authorize_owner_write(
    proof: ConfirmedPersonToken,
    organization: &OrganizationId,
    current_owner: Option<&PersonId>,
    caller_standing: &OrganizationStanding,
    now: UnixMillis,
    settings: OwnerGuardSettings,
) -> Result<AuthorizedOwner, CommandRefusal> {
    let proof = current_owner_proof(
        proof,
        organization,
        current_owner,
        caller_standing,
        now,
        settings,
    )
    .map_err(|_| CommandRefusal::NotOwner {})?;
    if !factor_within_window(
        proof.token().second_factor_at(),
        now,
        settings.second_factor_window_seconds,
    ) {
        return Err(CommandRefusal::SecondFactorRequired {
            within_seconds: settings.second_factor_window_seconds,
        });
    }
    Ok(AuthorizedOwner {
        person_id: proof.token().person_id().clone(),
    })
}

/// Operator authority is fleet-wide and requires no organization-owner binding.
pub fn authorize_operator(
    proof: ConfirmedPersonToken,
    configured_operator: &PersonId,
    now: UnixMillis,
    settings: OwnerGuardSettings,
) -> Result<AuthorizedOperator, CommandRefusal> {
    if proof.token().purpose() != PersonTokenAudience::Operator
        || proof.token().person_id() != configured_operator
    {
        return Err(CommandRefusal::NotOwner {});
    }
    let proof = proof
        .require_fresh(now, settings.confirmation_window)
        .map_err(|_| CommandRefusal::NotOwner {})?;
    validate_token_expiry(proof.token().expires_at(), now)
        .map_err(|_| CommandRefusal::NotOwner {})?;
    Ok(AuthorizedOperator {
        person_id: proof.token().person_id().clone(),
    })
}

fn current_owner_proof(
    proof: ConfirmedPersonToken,
    organization: &OrganizationId,
    current_owner: Option<&PersonId>,
    caller_standing: &OrganizationStanding,
    now: UnixMillis,
    settings: OwnerGuardSettings,
) -> Result<ConfirmedPersonToken, OwnerRefusal> {
    if proof.token().purpose() != PersonTokenAudience::Admin
        || proof.token().organization_id() != organization
        || current_owner.is_none_or(|owner| owner != proof.token().person_id())
        || !matches!(
            caller_standing,
            OrganizationStanding::Admitted {
                role: StandingRole::Owner
            }
        )
    {
        return Err(OwnerRefusal::NotOwner {});
    }
    let proof = proof
        .require_fresh(now, settings.confirmation_window)
        .map_err(|_| OwnerRefusal::NotOwner {})?;
    validate_token_expiry(proof.token().expires_at(), now)
        .map_err(|_| OwnerRefusal::NotOwner {})?;
    Ok(proof)
}

fn factor_within_window(
    second_factor_at: Option<UnixSeconds>,
    now: UnixMillis,
    window_seconds: u32,
) -> bool {
    let Some(age_millis) = second_factor_at
        .and_then(|at| at.0.checked_mul(1_000))
        .and_then(|at| now.0.checked_sub(at))
    else {
        return false;
    };
    age_millis <= i64::from(window_seconds).saturating_mul(1_000)
}
