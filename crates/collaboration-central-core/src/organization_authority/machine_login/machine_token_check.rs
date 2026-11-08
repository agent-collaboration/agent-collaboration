//! Central's machine-token check (C9, PD:596), in order: an allowed `alg`, the
//! pinned issuer, the signature against the issuer's cached keys (B2's cache,
//! resolution and verifier), central's machine audience, then time on the
//! clock read after the signature: `now < exp + D`, `iat ≤ now + D` and,
//! with no leeway, `now < grant_expires_at` (PD:578). Admission then reads
//! the enrollment live, for a new connection and for each renewal.
use super::decision_support::MachineLoginSettings;
use super::enrollment_port::{MachineEnrollmentFailure, MachineEnrollmentPort};
use super::machine_connection::admit_machine_connection;
use crate::identity_contract::{
    AdmitMachineConnection, AdmittedMachine, ClientId, MachineConnectionAdmission,
    MachineConnectionRefusal, MachinePrincipal, MachineTokenClaims, PresentedMachine,
};
use crate::person_request::{Audience, CompactJwt, TokenHeader, from_json};
use crate::{Ed25519Verification, IssuerKeySource, IssuerSignatureVerifier, SignatureValidity};
use fleet_identity_protocol::{
    EnrollmentId, InstallationId, MachineTokenRenewal, MachineTokenRenewalRefusal, OrganizationId,
    UnixSeconds,
};
use runtime_primitives::Clock;
use serde::Deserialize;
use std::sync::Arc;

/// D: the most two service clocks may differ (PD:578).
pub const ISSUER_CLOCK_SKEW_SECONDS: i64 = 60;

/// Central's trust in the sign-in service's machine tokens.
#[derive(Clone, Debug)]
pub struct MachineTokenPolicy {
    pub issuer: String,
    /// Central's own machine address: every machine token names it.
    pub machine_audience: String,
}

/// What a valid machine token says, before the enrollment is read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedMachineToken {
    pub organization_id: OrganizationId,
    pub installation: InstallationId,
    pub enrollment_id: EnrollmentId,
    pub client_id: ClientId,
    pub expires_at: UnixSeconds,
}

/// Each step of the check refuses for its own reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum MachineTokenCheckRefusal {
    #[error("the token is not a compact JWT")]
    Malformed,
    #[error("the token's header names no allowed algorithm and key")]
    UnsupportedHeader,
    #[error("the token's issuer is not the pinned sign-in service")]
    Issuer,
    #[error("the token's signature does not verify")]
    Signature,
    #[error("the issuer's keys are unusable")]
    IssuerKey,
    #[error("signature verification is unavailable")]
    VerificationUnavailable,
    #[error("the token does not name central's machine address")]
    Audience,
    #[error("the token expired")]
    Expired,
    #[error("the token was issued in the future")]
    IssuedInFuture,
    #[error("the machine claims are malformed")]
    Claims,
    #[error("the enrollment's grant expired")]
    GrantExpired,
}

impl MachineTokenCheckRefusal {
    /// The renewal answer: time refusals are expiries, the rest invalid tokens.
    const fn renewal(self) -> MachineTokenRenewalRefusal {
        match self {
            Self::Expired | Self::GrantExpired => MachineTokenRenewalRefusal::TokenExpired {},
            Self::Malformed
            | Self::UnsupportedHeader
            | Self::Issuer
            | Self::Signature
            | Self::IssuerKey
            | Self::VerificationUnavailable
            | Self::Audience
            | Self::IssuedInFuture
            | Self::Claims => MachineTokenRenewalRefusal::TokenInvalid {},
        }
    }
}

/// The registered claims C9 reads, beside the machine extension claims. Other
/// registered claims (`sub`, `jti`, `azp`) are accepted and unread.
#[derive(Deserialize)]
struct MachineTokenBody {
    aud: Audience,
    exp: i64,
    iat: i64,
    client_id: ClientId,
    #[serde(flatten)]
    machine: MachineTokenClaims,
}

#[derive(Deserialize)]
struct IssuerOnly {
    iss: String,
}

pub struct MachineTokenVerifier<Ed, Keys> {
    signatures: IssuerSignatureVerifier<Ed, Keys>,
    clock: Arc<dyn Clock>,
    policy: MachineTokenPolicy,
}

impl<Ed: Ed25519Verification, Keys: IssuerKeySource> MachineTokenVerifier<Ed, Keys> {
    #[must_use]
    pub fn new(
        signatures: IssuerSignatureVerifier<Ed, Keys>,
        clock: Arc<dyn Clock>,
        policy: MachineTokenPolicy,
    ) -> Self {
        Self {
            signatures,
            clock,
            policy,
        }
    }

    pub async fn verify(
        &self,
        token: &str,
    ) -> Result<VerifiedMachineToken, MachineTokenCheckRefusal> {
        use MachineTokenCheckRefusal as Refusal;
        let parsed = CompactJwt::parse(token).map_err(|_| Refusal::Malformed)?;
        let header: TokenHeader = from_json(
            &parsed.header,
            crate::person_request::PersonRequestRefusal::UnsupportedHeader,
        )
        .map_err(|_| Refusal::UnsupportedHeader)?;
        if header.alg != "EdDSA"
            || header.kid.is_empty()
            || header
                .typ
                .as_ref()
                .is_some_and(|typ| typ != "JWT" && typ != "at+jwt")
        {
            return Err(Refusal::UnsupportedHeader);
        }
        let issuer: IssuerOnly = from_json(
            &parsed.claims,
            crate::person_request::PersonRequestRefusal::MalformedJwt,
        )
        .map_err(|_| Refusal::Claims)?;
        if issuer.iss != self.policy.issuer {
            return Err(Refusal::Issuer);
        }
        match self
            .signatures
            .verify(&header.kid, &parsed.signed, &parsed.signature)
            .await
        {
            Ok(SignatureValidity::Valid) => {}
            Ok(SignatureValidity::Invalid) => return Err(Refusal::Signature),
            Err(crate::IssuerSignatureRefusal::Keys(
                crate::IssuerKeyResolutionRefusal::KeySet(_),
            )) => return Err(Refusal::IssuerKey),
            Err(_) => return Err(Refusal::VerificationUnavailable),
        }
        let body: MachineTokenBody = from_json(
            &parsed.claims,
            crate::person_request::PersonRequestRefusal::MalformedJwt,
        )
        .map_err(|_| Refusal::Claims)?;
        if !body.aud.names(&self.policy.machine_audience) {
            return Err(Refusal::Audience);
        }
        // Sampled after the signature and key fetch, so a wait counts.
        let now_ms = self.clock.now().unix_millis();
        let skew_ms = ISSUER_CLOCK_SKEW_SECONDS * 1000;
        if now_ms >= body.exp.saturating_mul(1000).saturating_add(skew_ms) {
            return Err(Refusal::Expired);
        }
        if body.iat.saturating_mul(1000) > now_ms.saturating_add(skew_ms) {
            return Err(Refusal::IssuedInFuture);
        }
        if now_ms >= body.machine.grant_expires_at.0.saturating_mul(1000) {
            return Err(Refusal::GrantExpired);
        }
        let MachinePrincipal::Machine {
            installation_id,
            enrollment_id,
        } = body.machine.principal;
        Ok(VerifiedMachineToken {
            organization_id: body.machine.organization_id,
            installation: installation_id,
            enrollment_id,
            client_id: body.client_id,
            expires_at: UnixSeconds(body.exp),
        })
    }
}

/// How a token presented on a connection fared: refused by the check, refused
/// by the enrollment, or admitted.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MachineTokenAdmission {
    TokenRefused(MachineTokenCheckRefusal),
    Admission(MachineConnectionAdmission),
}

/// A new connection: the check, then `AdmitMachineConnection` read live.
pub async fn admit_machine_token<Ed, Keys, P>(
    verifier: &MachineTokenVerifier<Ed, Keys>,
    port: &P,
    token: &str,
    presented: PresentedMachine,
    settings: MachineLoginSettings,
) -> Result<MachineTokenAdmission, MachineEnrollmentFailure>
where
    Ed: Ed25519Verification,
    Keys: IssuerKeySource,
    P: MachineEnrollmentPort,
{
    match verifier.verify(token).await {
        Ok(verified) => {
            admit_verified(verifier.clock.as_ref(), port, verified, presented, settings)
                .await
                .map(MachineTokenAdmission::Admission)
        }
        Err(refusal) => Ok(MachineTokenAdmission::TokenRefused(refusal)),
    }
}

async fn admit_verified<P: MachineEnrollmentPort>(
    clock: &dyn Clock,
    port: &P,
    verified: VerifiedMachineToken,
    presented: PresentedMachine,
    settings: MachineLoginSettings,
) -> Result<MachineConnectionAdmission, MachineEnrollmentFailure> {
    let request = AdmitMachineConnection {
        organization_id: verified.organization_id,
        enrollment_id: verified.enrollment_id,
        installation: verified.installation,
        presented,
        token_expires_at: verified.expires_at,
        client_id: verified.client_id,
    };
    admit_machine_connection(port, &request, clock, settings).await
}

/// `RenewMachineToken` on an open connection: the fresh token is checked
/// like a new connection and must name the connection's own installation
/// and enrollment.
pub async fn renew_machine_token<Ed, Keys, P>(
    verifier: &MachineTokenVerifier<Ed, Keys>,
    port: &P,
    token: &str,
    connection: &AdmittedMachine,
    settings: MachineLoginSettings,
) -> Result<MachineTokenRenewal, MachineEnrollmentFailure>
where
    Ed: Ed25519Verification,
    Keys: IssuerKeySource,
    P: MachineEnrollmentPort,
{
    let refused = |reason| Ok(MachineTokenRenewal::Refused { reason });
    let verified = match verifier.verify(token).await {
        Ok(verified) => verified,
        Err(refusal) => return refused(refusal.renewal()),
    };
    if verified.installation != connection.installation {
        return refused(MachineTokenRenewalRefusal::WrongInstallation {});
    }
    // A token for another enrollment of this machine means this connection's
    // enrollment is no longer the one being renewed.
    if verified.enrollment_id != connection.enrollment_id {
        return refused(MachineTokenRenewalRefusal::EnrollmentNotActive {});
    }
    let presented = PresentedMachine {
        installation: connection.installation.clone(),
        peer: connection.peer,
    };
    match admit_verified(verifier.clock.as_ref(), port, verified, presented, settings).await? {
        MachineConnectionAdmission::Admitted { .. } => Ok(MachineTokenRenewal::Renewed {}),
        MachineConnectionAdmission::Refused { reason } => refused(match reason {
            MachineConnectionRefusal::WrongClient {} => MachineTokenRenewalRefusal::WrongClient {},
            MachineConnectionRefusal::UnknownMachine {} => {
                MachineTokenRenewalRefusal::WrongInstallation {}
            }
            MachineConnectionRefusal::EnrollmentNotActive {}
            | MachineConnectionRefusal::Superseded {}
            | MachineConnectionRefusal::MachineInAnotherOrganization {} => {
                MachineTokenRenewalRefusal::EnrollmentNotActive {}
            }
        }),
    }
}

#[cfg(test)]
#[path = "machine_token_check_test.rs"]
mod tests;
