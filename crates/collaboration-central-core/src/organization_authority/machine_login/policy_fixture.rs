//! Pure fact/plan fixtures; these do not emulate or prove a database commit.
use super::{
    Decided, DecisionTime, EnrollmentChange, EnrollmentFacts, MachineLoginSettings, MachineRow,
    MachineState, StoredEnrollment, StoredEnrollmentState, decide_machine_token,
};
use crate::identity_contract::{
    DecideDeviceLogin, DecideMachineToken, EnrollPermission, MachineTokenDecision,
    MachineTokenRefusal, PersonId,
};
use fleet_identity_protocol::{EnrollmentId, InstallationId, OrganizationId, UnixSeconds};
use message_board::{MachineLabel, WorkspaceId};
use runtime_primitives::IdSource;
use std::sync::atomic::{AtomicU64, Ordering};
use uuid::Uuid;

pub(super) const NOW_MS: i64 = 1_800_000_000_123;
pub(super) const NOW_S: i64 = NOW_MS / 1000;
pub(super) fn id(suffix: u64) -> String {
    format!("018f1111-2222-7000-8000-{suffix:012x}")
}
pub(super) fn installation() -> InstallationId {
    id(10).try_into().unwrap()
}
pub(super) fn workspace(suffix: u64) -> WorkspaceId {
    id(suffix).try_into().unwrap()
}
pub(super) fn person() -> PersonId {
    id(20).try_into().unwrap()
}
pub(super) fn label(value: &str) -> MachineLabel {
    value.to_owned().try_into().unwrap()
}
pub(super) fn settings() -> MachineLoginSettings {
    MachineLoginSettings::new(900_u32.try_into().unwrap())
}
pub(super) fn time(now_ms: i64) -> DecisionTime {
    DecisionTime { now_ms }
}

pub(super) struct CountingIds(AtomicU64);
impl CountingIds {
    pub(super) fn new() -> Self {
        Self(AtomicU64::new(1000))
    }
    pub(super) fn minted(&self) -> u64 {
        self.0.load(Ordering::SeqCst) - 1000
    }
}
impl IdSource for CountingIds {
    fn new_id(&self) -> durable_steps::Uuid {
        Uuid::parse_str(&id(self.0.fetch_add(1, Ordering::SeqCst))).unwrap()
    }
}

pub(super) fn empty_facts() -> EnrollmentFacts {
    EnrollmentFacts {
        organization_id: OrganizationId::try_from(id(1)).unwrap(),
        organization_has_enrollment: false,
        prior_approval: None,
        enrollment: None,
        machine: None,
        active: None,
        missing_workspaces: Vec::new(),
    }
}
pub(super) fn approval() -> DecideDeviceLogin {
    DecideDeviceLogin {
        organization_id: empty_facts().organization_id,
        approver: person(),
        permission_checked: EnrollPermission::MachineEnroll,
        request_id: id(30).try_into().unwrap(),
        installation: installation(),
        peer: id(10).try_into().unwrap(),
        label: label("original"),
        allowed_workspaces: vec![workspace(50), workspace(51)].try_into().unwrap(),
        approver_second_factor_at: None,
        code_expires_at: UnixSeconds(NOW_S + 600),
    }
}
pub(super) fn machine(generation: u64) -> MachineRow {
    MachineRow {
        id: Uuid::parse_str(&id(11)).unwrap(),
        installation: installation(),
        label: label("original"),
        allowed_workspaces: vec![workspace(50), workspace(51)],
        generation: generation.try_into().unwrap(),
        state: MachineState::Revoked,
    }
}
pub(super) fn approved(suffix: u64, generation: u64) -> StoredEnrollment {
    StoredEnrollment {
        id: EnrollmentId::try_from(id(suffix)).unwrap(),
        machine_id: machine(generation).id,
        installation: installation(),
        request_id: id(suffix + 100).try_into().unwrap(),
        approved_by: person(),
        label: label("original"),
        allowed_workspaces: vec![workspace(50), workspace(51)].try_into().unwrap(),
        base_generation: generation.try_into().unwrap(),
        approved_at_ms: NOW_MS - 60_000,
        code_expires_at: UnixSeconds(NOW_S + 600),
        state: StoredEnrollmentState::Approved,
        ended_at_ms: None,
    }
}
pub(super) fn enrolled(suffix: u64, generation: u64, last_grant: i64) -> StoredEnrollment {
    StoredEnrollment {
        state: StoredEnrollmentState::Enrolled {
            client_id: id(suffix).try_into().unwrap(),
            last_grant_at: UnixSeconds(last_grant),
        },
        ..approved(suffix, generation)
    }
}
pub(super) fn facts(
    candidate: StoredEnrollment,
    active: Option<StoredEnrollment>,
    generation: u64,
) -> EnrollmentFacts {
    let mut machine = machine(generation);
    if active.is_some() {
        machine.state = MachineState::Enrolled;
    }
    EnrollmentFacts {
        organization_has_enrollment: true,
        enrollment: Some(candidate),
        machine: Some(machine),
        active,
        ..empty_facts()
    }
}
pub(super) fn token(enrollment: &StoredEnrollment) -> DecideMachineToken {
    DecideMachineToken {
        organization_id: empty_facts().organization_id,
        enrollment_id: enrollment.id.clone(),
        client_id: enrollment.id.as_str().to_owned().try_into().unwrap(),
    }
}
pub(super) fn decide_token(facts: &EnrollmentFacts, now_ms: i64) -> Decided<MachineTokenDecision> {
    decide_machine_token(
        &token(facts.enrollment.as_ref().unwrap()),
        facts,
        time(now_ms),
        &CountingIds::new(),
        &settings(),
    )
    .unwrap()
}
pub(super) fn assert_refused(plan: &Decided<MachineTokenDecision>, reason: MachineTokenRefusal) {
    assert_eq!(plan.answer, MachineTokenDecision::Refuse { reason });
    assert!(plan.changes.is_empty());
    assert_eq!(plan.written_at_ms, None);
}
pub(super) fn records<T>(
    plan: &Decided<T>,
) -> Vec<&crate::organization_authority::OrganizationChangeRecord> {
    plan.changes
        .iter()
        .filter_map(|change| match change {
            EnrollmentChange::Record(record) => Some(record),
            _ => None,
        })
        .collect()
}

/// Project a decision's facts for the next pure-policy invocation only.
pub(super) fn project<T>(facts: &EnrollmentFacts, plan: &Decided<T>) -> EnrollmentFacts {
    let mut next = facts.clone();
    for change in &plan.changes {
        match change {
            EnrollmentChange::PutMachine(machine) => next.machine = Some(machine.clone()),
            EnrollmentChange::InsertEnrollment(row) | EnrollmentChange::ReplaceEnrollment(row) => {
                if next
                    .enrollment
                    .as_ref()
                    .is_some_and(|candidate| candidate.id == row.id)
                {
                    next.enrollment = Some(row.clone());
                }
                if matches!(row.state, StoredEnrollmentState::Enrolled { .. }) {
                    next.active = Some(row.clone());
                } else if next
                    .active
                    .as_ref()
                    .is_some_and(|active| active.id == row.id)
                {
                    next.active = None;
                }
            }
            EnrollmentChange::Record(_) => {}
        }
    }
    next
}
