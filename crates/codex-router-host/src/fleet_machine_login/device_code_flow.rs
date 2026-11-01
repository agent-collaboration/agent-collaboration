//! C8 polling and token timing, independent of HTTP, persistence and operator sockets.
use std::time::Duration;

use codex_router_core::redaction::SecretString;
use fleet_identity_protocol::{EnrollmentId, InstallationId, OrganizationId};
use fleet_sync_protocol::PeerId;
use message_board::MachineLabel;
use runtime_primitives::SyncTime;

use super::login_clock::{FleetLoginClock, LoginFuture};
use super::login_contract::{
    FleetLoginEvent, FleetLoginRefusal, FleetLoginUnknown, RefreshCredentialKeeping, UserCode,
    VerificationUri,
};
use super::token_response::LearnedFleetCentral;

#[derive(Clone, Debug)]
pub(super) struct DeviceLoginRequest {
    pub organization_id: OrganizationId,
    pub installation: InstallationId,
    pub peer: PeerId,
    pub label: MachineLabel,
}

#[derive(Clone, Debug)]
pub(super) struct RefreshCredentialBundle {
    pub machine_client_id: EnrollmentId,
    pub refresh_credential: SecretString,
}

#[derive(Clone, Debug)]
pub(super) struct IssuedMachineToken {
    pub token: SecretString,
    pub valid_until: SyncTime,
    pub credential: RefreshCredentialBundle,
}

#[derive(Clone, Debug)]
pub(super) struct IssuedMachineLogin {
    pub token: IssuedMachineToken,
    pub central: LearnedFleetCentral,
}

pub(super) struct DeviceCodeAuthorization {
    pub device_code: SecretString,
    pub user_code: UserCode,
    pub verification_uri: VerificationUri,
    pub expires_at: SyncTime,
    pub interval_seconds: u64,
}

pub(super) enum DevicePollResult {
    Pending,
    SlowDown,
    Issued(IssuedMachineLogin),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum IssuerFailure {
    AccessDenied,
    CodeExpired,
    InvalidGrant,
    Timeout,
    Unavailable,
    InvalidResponse,
}

#[derive(Debug)]
pub(super) enum DeviceCodeFailure {
    Refused(FleetLoginRefusal),
    OutcomeUnknown(FleetLoginUnknown),
}

fn issuer_failure(failure: IssuerFailure, request: &DeviceLoginRequest) -> DeviceCodeFailure {
    match failure {
        IssuerFailure::AccessDenied | IssuerFailure::InvalidGrant => {
            DeviceCodeFailure::Refused(FleetLoginRefusal::AccessDenied {})
        }
        IssuerFailure::CodeExpired => DeviceCodeFailure::Refused(FleetLoginRefusal::CodeExpired {}),
        IssuerFailure::Timeout | IssuerFailure::Unavailable | IssuerFailure::InvalidResponse => {
            DeviceCodeFailure::OutcomeUnknown(FleetLoginUnknown::DeviceRedemption {
                organization_id: request.organization_id.clone(),
                installation: request.installation.clone(),
            })
        }
    }
}

pub(super) trait MachineTokenIssuer: Send + Sync {
    fn authorize<'a>(
        &'a self,
        request: &'a DeviceLoginRequest,
    ) -> LoginFuture<'a, Result<DeviceCodeAuthorization, IssuerFailure>>;
    fn poll<'a>(
        &'a self,
        device_code: &'a SecretString,
    ) -> LoginFuture<'a, Result<DevicePollResult, IssuerFailure>>;
    fn refresh<'a>(
        &'a self,
        credential: &'a RefreshCredentialBundle,
    ) -> LoginFuture<'a, Result<IssuedMachineToken, IssuerFailure>>;
}

pub(super) async fn run_device_code_login(
    issuer: &dyn MachineTokenIssuer,
    clock: &dyn FleetLoginClock,
    request: &DeviceLoginRequest,
    keeping: RefreshCredentialKeeping,
    show_code: impl FnOnce(FleetLoginEvent),
) -> Result<IssuedMachineLogin, DeviceCodeFailure> {
    let authorization = issuer
        .authorize(request)
        .await
        .map_err(|failure| issuer_failure(failure, request))?;
    if authorization.interval_seconds == 0 || authorization.expires_at <= clock.now() {
        return Err(issuer_failure(IssuerFailure::InvalidResponse, request));
    }
    show_code(FleetLoginEvent::Code {
        user_code: authorization.user_code,
        verification_uri: authorization.verification_uri,
        keeping,
    });
    let mut interval_seconds = authorization.interval_seconds;
    loop {
        let deadline = clock
            .now()
            .after(Duration::from_secs(interval_seconds))
            .min(authorization.expires_at);
        clock
            .wait_until(deadline)
            .await
            .map_err(|()| DeviceCodeFailure::Refused(FleetLoginRefusal::IssuerUnavailable {}))?;
        if clock.now() >= authorization.expires_at {
            return Err(DeviceCodeFailure::Refused(
                FleetLoginRefusal::CodeExpired {},
            ));
        }
        match issuer.poll(&authorization.device_code).await {
            Ok(DevicePollResult::Issued(token)) => return Ok(token),
            Ok(DevicePollResult::Pending) => {}
            // RFC 8628 section 3.5: +5 seconds applies to all later polls.
            Ok(DevicePollResult::SlowDown) => interval_seconds = interval_seconds.saturating_add(5),
            // A timed-out redemption may already have consumed the code. The
            // approved F11 policy stops with an unknown ending, without replay.
            Err(failure) => return Err(issuer_failure(failure, request)),
        }
    }
}

pub(super) fn renewal_deadline(now: SyncTime, expires_at: SyncTime) -> SyncTime {
    let remaining = expires_at
        .unix_millis()
        .saturating_sub(now.unix_millis())
        .max(0);
    // Renew with one sixth of the token's remaining lifetime still available.
    let delay = remaining.saturating_sub((remaining / 6).max(1)).max(0);
    SyncTime::from_unix_millis(now.unix_millis().saturating_add(delay))
}

#[cfg(test)]
#[path = "device_code_flow_tests.rs"]
mod tests;
