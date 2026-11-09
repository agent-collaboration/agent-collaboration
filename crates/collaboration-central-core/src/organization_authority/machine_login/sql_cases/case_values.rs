//! Fixed values, the injected clock and ID source, and the decision calls the
//! cases share. Nothing here panics: an invalid fixture value is a failure.
use super::super::decision_support::MachineLoginSettings;
use super::super::device_login_decision::decide_device_login;
use super::super::enrollment_port::{
    Decided, EnrollmentFacts, EnrollmentQuery, MachineEnrollmentFailure, MachineEnrollmentPort,
};
use super::super::machine_token_decision::decide_machine_token;
use super::sql_case_host::CaseHost;
use super::{CaseFailure, CaseResult};
use crate::identity_contract::{
    AllowedWorkspaces, ClientId, DecideDeviceLogin, DecideMachineToken, DeviceLoginDecision,
    DeviceLoginRequestId, EnrollPermission, MachineTokenDecision, PersonId, TokenLifetimeSeconds,
};
use durable_steps::Uuid;
use fleet_identity_protocol::{EnrollmentId, InstallationId, OrganizationId, UnixSeconds};
use fleet_sync_protocol::PeerId;
use message_board::{MachineLabel, WorkspaceId};
use runtime_primitives::{Clock, IdSource, SyncTime};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) const NOW_MS: i64 = 1_800_000_000_000;
pub(super) const NOW_S: i64 = NOW_MS / 1000;
pub(super) const DAY_MS: i64 = 24 * 60 * 60 * 1000;
pub(super) const ORGANIZATION: u64 = 0x01;
pub(super) const APPROVER: u64 = 0x02;
pub(super) const REVOKER: u64 = 0x03;
pub(super) const MACHINE: u64 = 0x10;
pub(super) const WORKSPACE_A: u64 = 0x20;
pub(super) const WORKSPACE_B: u64 = 0x21;
pub(super) const REQUEST: u64 = 0x30;
/// The ten-minute device code the cases approve with.
pub(super) const CODE_LIFETIME_S: i64 = 600;
pub(super) const LABEL: &str = "fixture machine";
pub(super) const FORMER: &str = "0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7d";

pub(super) fn uuid7(suffix: u64) -> String {
    format!("019f0000-0000-7000-8000-{suffix:012x}")
}

pub(super) fn typed<T: TryFrom<String>>(value: String) -> CaseResult<T> {
    T::try_from(value.clone()).map_err(|_| CaseFailure(format!("invalid fixture value {value}")))
}

/// A clock that always reads one time.
pub(super) struct At(pub i64);
impl Clock for At {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0)
    }
}
pub(super) fn at(now_ms: i64) -> Arc<dyn Clock> {
    Arc::new(At(now_ms))
}

/// UUIDv7s counting up from a block, so every minted ID is known.
struct Counting(AtomicU64);
impl IdSource for Counting {
    fn new_id(&self) -> Uuid {
        let next = self.0.fetch_add(1, Ordering::SeqCst);
        Uuid::parse_str(&uuid7(next)).unwrap_or_default()
    }
}
pub(super) fn ids(block: u64) -> Arc<dyn IdSource> {
    Arc::new(Counting(AtomicU64::new(block)))
}

pub(super) fn installation(suffix: u64) -> CaseResult<InstallationId> {
    typed(uuid7(suffix))
}
pub(super) fn organization() -> CaseResult<OrganizationId> {
    typed(uuid7(ORGANIZATION))
}

pub(super) fn settings() -> CaseResult<MachineLoginSettings> {
    TokenLifetimeSeconds::try_from(900)
        .map(MachineLoginSettings::new)
        .map_err(|error| CaseFailure(error.to_string()))
}

/// A device login for installation `machine` (also its peer), allowing
/// `workspaces`, with a ten-minute code from `NOW_S`.
pub(super) fn approval(
    request: u64,
    machine: u64,
    workspaces: &[u64],
) -> CaseResult<DecideDeviceLogin> {
    Ok(DecideDeviceLogin {
        organization_id: organization()?,
        approver: typed::<PersonId>(uuid7(APPROVER))?,
        permission_checked: EnrollPermission::MachineEnroll,
        request_id: typed::<DeviceLoginRequestId>(uuid7(request))?,
        installation: installation(machine)?,
        peer: PeerId::try_from(uuid7(machine)).map_err(|error| CaseFailure(error.to_string()))?,
        label: typed::<MachineLabel>(LABEL.to_owned())?,
        allowed_workspaces: AllowedWorkspaces::try_from(
            workspaces
                .iter()
                .map(|suffix| typed::<WorkspaceId>(uuid7(*suffix)))
                .collect::<CaseResult<Vec<_>>>()?,
        )
        .map_err(|error| CaseFailure(error.to_string()))?,
        approver_second_factor_at: Some(UnixSeconds(NOW_S)),
        former_service_id: None,
        code_expires_at: UnixSeconds(NOW_S + CODE_LIFETIME_S),
    })
}

/// A B3 store with these workspaces and no machines.
pub(super) async fn b3_store<Host: CaseHost>(host: &Host, workspaces: &[u64]) -> CaseResult {
    host.initialize_pre_b3(Vec::new()).await?;
    host.initialize_b3().await?;
    for workspace in workspaces {
        add_workspace(host, *workspace).await?;
    }
    Ok(())
}

pub(super) async fn add_workspace<Host: CaseHost>(host: &Host, suffix: u64) -> CaseResult {
    Ok(host
        .execute(format!(
            "INSERT INTO fleet_workspaces(workspace_id, name, description, state, repository_count, \
             actor_kind, fleet_position, time_basis, origin_installation, origin_machine, \
             created_at, updated_at) VALUES('{}', 'workspace {suffix}', '', 'active', 0, 'human', \
             {suffix}, 'router', '{}', 'fixture', 0, 0)",
            uuid7(suffix),
            uuid7(0x0f)
        ))
        .await?)
}

pub(super) async fn approve<Host: CaseHost>(
    host: &Host,
    request: DecideDeviceLogin,
    now_ms: i64,
    block: u64,
) -> Result<DeviceLoginDecision, MachineEnrollmentFailure> {
    decide_device_login(host.port(), request, now_ms, at(now_ms), ids(block)).await
}

/// Approves and returns the new enrollment's ID, or fails the case.
pub(super) async fn approved<Host: CaseHost>(
    host: &Host,
    request: DecideDeviceLogin,
    now_ms: i64,
    block: u64,
) -> CaseResult<EnrollmentId> {
    match approve(host, request, now_ms, block).await? {
        DeviceLoginDecision::Approved { enrollment_id } => Ok(enrollment_id),
        refused @ DeviceLoginDecision::Refused { .. } => Err(CaseFailure(format!(
            "expected an approval, got {refused:?}"
        ))),
    }
}

/// A token request from the enrollment's own client (its ID, PD:956).
pub(super) async fn grant<Host: CaseHost>(
    host: &Host,
    enrollment: &EnrollmentId,
    now_ms: i64,
    block: u64,
) -> CaseResult<MachineTokenDecision> {
    Ok(decide_machine_token(
        host.port(),
        DecideMachineToken {
            organization_id: organization()?,
            enrollment_id: enrollment.clone(),
            client_id: typed::<ClientId>(enrollment.as_str().to_owned())?,
        },
        at(now_ms),
        ids(block),
        settings()?,
    )
    .await?)
}

/// The facts the port reads for this query, unchanged.
pub(super) async fn facts<Host: CaseHost>(
    host: &Host,
    query: EnrollmentQuery,
) -> CaseResult<EnrollmentFacts> {
    Ok(host
        .port()
        .transact(query, |facts| Decided::unchanged(facts.clone()))
        .await?)
}
