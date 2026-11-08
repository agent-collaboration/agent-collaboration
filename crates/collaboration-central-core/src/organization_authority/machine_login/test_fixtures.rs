//! Shared values for the machine-login decision tests: a counting UUIDv7
//! source, fixed IDs, a device-login request and a seeded store.
use super::enrollment_records::{
    AllowanceRow, EnrollmentGeneration, InstallationRow, StoredEnrollment, StoredEnrollmentState,
};
use super::in_memory_enrollments::EnrollmentStore;
use crate::identity_contract::{
    AllowanceState, AllowedWorkspaces, ClientId, DecideDeviceLogin, DeviceLoginRequestId,
    EnrollPermission, PersonId,
};
use durable_steps::Uuid;
use fleet_identity_protocol::{EnrollmentId, InstallationId, OrganizationId, UnixSeconds};
use fleet_sync_protocol::PeerId;
use message_board::{MachineLabel, WorkspaceId};
use runtime_primitives::IdSource;
use std::sync::atomic::{AtomicU64, Ordering};

pub(super) const NOW_MS: i64 = 1_800_000_000_000;
pub(super) const NOW_S: i64 = NOW_MS / 1000;

/// UUIDv7s counting up from a fixed prefix, so every minted ID is predictable.
pub(super) struct CountingIds(AtomicU64);
impl CountingIds {
    pub fn new() -> Self {
        Self::from(0xa000)
    }
    pub fn from(first: u64) -> Self {
        Self(AtomicU64::new(first))
    }
}
impl IdSource for CountingIds {
    fn new_id(&self) -> Uuid {
        let next = self.0.fetch_add(1, Ordering::SeqCst);
        Uuid::parse_str(&format!("019f0000-0000-7000-8000-{next:012x}")).unwrap()
    }
}

pub(super) fn uuid7(suffix: u64) -> String {
    format!("019f0000-0000-7000-8000-{suffix:012x}")
}
pub(super) fn installation(suffix: u64) -> InstallationId {
    InstallationId::try_from(uuid7(suffix)).unwrap()
}
pub(super) fn enrollment(suffix: u64) -> EnrollmentId {
    EnrollmentId::try_from(uuid7(suffix)).unwrap()
}
pub(super) fn client(suffix: u64) -> ClientId {
    ClientId::try_from(uuid7(suffix)).unwrap()
}
pub(super) fn workspace(suffix: u64) -> WorkspaceId {
    WorkspaceId::try_from(uuid7(suffix)).unwrap()
}

pub(super) const ORGANIZATION: u64 = 0x01;
pub(super) const APPROVER: u64 = 0x02;
pub(super) const MACHINE: u64 = 0x10;
pub(super) const WORKSPACE_A: u64 = 0x20;
pub(super) const WORKSPACE_B: u64 = 0x21;

/// A device login for installation `machine` (also its peer), allowing `workspaces`.
pub(super) fn approval(request: u64, machine: u64, workspaces: &[u64]) -> DecideDeviceLogin {
    DecideDeviceLogin {
        organization_id: OrganizationId::try_from(uuid7(ORGANIZATION)).unwrap(),
        approver: PersonId::try_from(uuid7(APPROVER)).unwrap(),
        permission_checked: EnrollPermission::MachineEnroll,
        request_id: DeviceLoginRequestId::try_from(uuid7(request)).unwrap(),
        installation: installation(machine),
        peer: PeerId::try_from(uuid7(machine)).unwrap(),
        label: MachineLabel::try_from("fixture machine".to_owned()).unwrap(),
        allowed_workspaces: AllowedWorkspaces::try_from(
            workspaces
                .iter()
                .map(|suffix| workspace(*suffix))
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        approver_second_factor_at: Some(UnixSeconds(NOW_S)),
        former_service_id: None,
    }
}

/// An organization with both workspaces and no machines.
pub(super) fn empty_store() -> EnrollmentStore {
    let mut store = EnrollmentStore::default();
    store.workspaces.insert(workspace(WORKSPACE_A));
    store.workspaces.insert(workspace(WORKSPACE_B));
    store
}

/// Adds an enrolled machine `machine` with its enrollment `id`, its client and
/// an active allowance into workspace A, at `generation`.
pub(super) fn with_enrolled(
    mut store: EnrollmentStore,
    machine: u64,
    id: u64,
    generation: u64,
    last_grant_s: i64,
) -> EnrollmentStore {
    store.enrollments.insert(
        enrollment(id),
        StoredEnrollment {
            id: enrollment(id),
            installation: installation(machine),
            approval: approval(id + 0x100, machine, &[WORKSPACE_A]),
            base_generation: EnrollmentGeneration(generation.saturating_sub(1)),
            approved_at_ms: last_grant_s * 1000,
            state: StoredEnrollmentState::Enrolled {
                machine_client_id: client(id),
                last_grant_at: UnixSeconds(last_grant_s),
            },
        },
    );
    store.installations.insert(
        installation(machine),
        InstallationRow {
            installation: installation(machine),
            generation: EnrollmentGeneration(generation),
            active: Some(enrollment(id)),
        },
    );
    store.allowances.push(AllowanceRow {
        id: Uuid::parse_str(&uuid7(id + 0x200)).unwrap(),
        installation: installation(machine),
        workspace: workspace(WORKSPACE_A),
        state: AllowanceState::Active,
    });
    store
}

/// The applying clock, moved only by the test.
pub(super) struct TestClock(std::sync::atomic::AtomicI64);
impl TestClock {
    pub fn at(now_ms: i64) -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self(std::sync::atomic::AtomicI64::new(now_ms)))
    }
    pub fn set(&self, now_ms: i64) {
        self.0.store(now_ms, Ordering::SeqCst);
    }
}
impl runtime_primitives::Clock for TestClock {
    fn now(&self) -> runtime_primitives::SyncTime {
        runtime_primitives::SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}

/// Adds an approved, never-granted enrollment `id` of machine `machine`,
/// approved at `base` and at `approved_at_ms`. Creates the installation's row
/// at `base` if the machine has none.
pub(super) fn with_approved(
    mut store: EnrollmentStore,
    machine: u64,
    id: u64,
    base: u64,
    approved_at_ms: i64,
) -> EnrollmentStore {
    store.enrollments.insert(
        enrollment(id),
        StoredEnrollment {
            id: enrollment(id),
            installation: installation(machine),
            approval: approval(id + 0x100, machine, &[WORKSPACE_A]),
            base_generation: EnrollmentGeneration(base),
            approved_at_ms,
            state: StoredEnrollmentState::Approved,
        },
    );
    store
        .installations
        .entry(installation(machine))
        .or_insert_with(|| InstallationRow {
            installation: installation(machine),
            generation: EnrollmentGeneration(base),
            active: None,
        });
    store
}

pub(super) fn settings() -> super::decision_support::MachineLoginSettings {
    super::decision_support::MachineLoginSettings::new(
        crate::identity_contract::TokenLifetimeSeconds::try_from(900).unwrap(),
    )
}
