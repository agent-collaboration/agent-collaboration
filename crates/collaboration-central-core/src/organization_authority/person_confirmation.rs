//! A fresh confirmation belongs to one request and one central-clock receipt.

use super::{
    ParsedPersonToken, PersonSignatureInput, PersonTokenAudience, PersonTokenPolicy,
    PersonTokenRefusal, SignInSessionRef, VerifiedPersonToken,
};
use crate::{SignatureValidity, identity_contract::PersonId};
use fleet_identity_protocol::{OrganizationId, UnixMillis, UnixSeconds};
use std::future::Future;

/// F3a owns the authenticated transport, issuer keys and real cryptography.
pub trait PersonSignInPort: Send + Sync {
    fn verify_person_signature<'request>(
        &'request self,
        input: PersonSignatureInput<'request>,
    ) -> impl Future<Output = Result<SignatureValidity, SignInUnavailable>> + Send + 'request;

    fn confirm_person_session<'request>(
        &'request self,
        bearer_token: &'request str,
    ) -> impl Future<Output = Result<PersonSessionConfirmation, SignInUnavailable>> + Send + 'request;
}

/// Each request makes its own signature and confirmation calls outside storage.
pub async fn verify_and_confirm_person<SignIn: PersonSignInPort>(
    sign_in: &SignIn,
    clock: &dyn runtime_primitives::Clock,
    policy: &PersonTokenPolicy,
    purpose: PersonTokenAudience,
    bearer_token: String,
) -> Result<ConfirmedPersonToken, PersonConfirmationRefusal> {
    let parsed = ParsedPersonToken::parse(bearer_token)?;
    let validity = sign_in
        .verify_person_signature(parsed.signature_input())
        .await?;
    let verified = parsed.verified(
        validity,
        policy,
        purpose,
        UnixMillis(clock.now().unix_millis()),
    )?;
    let answer = sign_in
        .confirm_person_session(verified.bearer_token())
        .await?;
    let received_at = UnixMillis(clock.now().unix_millis());
    verified.confirmed(answer, received_at)
}

/// Typed projection of K5, after its producer-owned wire shape is validated.
/// The provider's checkedAt is deliberately not a central freshness timestamp.
#[derive(Debug, Eq, PartialEq)]
pub enum PersonSessionConfirmation {
    Active {
        person_id: PersonId,
        session_ref: SignInSessionRef,
        audience: String,
        organization_id: OrganizationId,
        expires_at: UnixSeconds,
    },
    Inactive {},
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SignInUnavailable {
    #[error("the sign-in service's keys are unavailable")]
    KeysUnreachable,
    #[error("the sign-in service's confirmation is unavailable")]
    ConfirmationUnreachable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PersonConfirmationWindow(u32);

impl PersonConfirmationWindow {
    pub fn from_millis(millis: u32) -> Result<Self, PersonConfirmationRefusal> {
        if millis == 0 {
            return Err(PersonConfirmationRefusal::InvalidWindow);
        }
        Ok(Self(millis))
    }
}

impl Default for PersonConfirmationWindow {
    fn default() -> Self {
        Self(5_000)
    }
}

/// Owned, not Clone or serializable: the applying request consumes this proof.
pub struct ConfirmedPersonToken {
    token: VerifiedPersonToken,
    received_at: UnixMillis,
}

impl VerifiedPersonToken {
    pub fn confirmed(
        self,
        answer: PersonSessionConfirmation,
        received_at: UnixMillis,
    ) -> Result<ConfirmedPersonToken, PersonConfirmationRefusal> {
        let PersonSessionConfirmation::Active {
            person_id,
            session_ref,
            audience,
            organization_id,
            expires_at,
        } = answer
        else {
            return Err(PersonConfirmationRefusal::Inactive);
        };
        if self.person_id() != &person_id
            || self.session_ref() != &session_ref
            || self.audience() != audience
            || self.organization_id() != &organization_id
            || self.expires_at() != expires_at
        {
            return Err(PersonConfirmationRefusal::MismatchedProof);
        }
        Ok(ConfirmedPersonToken {
            token: self,
            received_at,
        })
    }
}

impl ConfirmedPersonToken {
    /// Call inside the owning apply transaction, after reading its current owner.
    pub fn require_fresh(
        self,
        now: UnixMillis,
        window: PersonConfirmationWindow,
    ) -> Result<Self, PersonConfirmationRefusal> {
        let elapsed = now
            .0
            .checked_sub(self.received_at.0)
            .ok_or(PersonConfirmationRefusal::Stale)?;
        if elapsed < 0 || elapsed > i64::from(window.0) {
            return Err(PersonConfirmationRefusal::Stale);
        }
        Ok(self)
    }

    #[must_use]
    pub fn token(&self) -> &VerifiedPersonToken {
        &self.token
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum PersonConfirmationRefusal {
    #[error("the person session is inactive")]
    Inactive,
    #[error("the confirmation names a different person proof")]
    MismatchedProof,
    #[error("the person confirmation is outside its apply window")]
    Stale,
    #[error("the person confirmation window must be positive")]
    InvalidWindow,
    #[error(transparent)]
    InvalidToken(#[from] PersonTokenRefusal),
    #[error(transparent)]
    Unavailable(#[from] SignInUnavailable),
}
