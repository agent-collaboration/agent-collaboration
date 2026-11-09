//! `decide_device_login` over the in-memory port: each test drives the public
//! entry and compares the whole store before and after.
use super::super::enrollment_records::{
    EnrollmentGeneration, InstallationRow, StoredEnrollment, StoredEnrollmentState,
};
use super::super::in_memory_enrollments::{EnrollmentStore, InMemoryEnrollments};
use super::super::test_fixtures::{
    APPROVER, CountingIds, MACHINE, NOW_MS, NOW_S, TestClock, WORKSPACE_A, WORKSPACE_B, approval,
    approver, empty_store, enrollment, installation, label, uuid7, with_enrolled, workspace,
};
use super::{SD8_MILLIS, decide_device_login};
use crate::identity_contract::{
    AllowanceState, DecideDeviceLogin, DeviceLoginDecision, DeviceLoginRefusal, FormerServiceId,
};
use crate::organization_authority::{OrganizationChangeKind, OrganizationChangeOrigin};
use fleet_identity_protocol::UnixSeconds;
use std::num::NonZeroU32;
use std::sync::Arc;

const REQUEST: u64 = 0x30;
const OTHER_MACHINE: u64 = 0x11;

async fn decide(
    port: &InMemoryEnrollments,
    request: &DecideDeviceLogin,
    received_at_ms: i64,
    now_ms: i64,
) -> DeviceLoginDecision {
    let clock = TestClock::at(now_ms);
    // Each decision mints from a fresh block, so repeats never reuse an ID.
    let minted = u64::try_from(port.snapshot().records.len() + port.snapshot().enrollments.len())
        .unwrap_or(0);
    decide_device_login(
        port,
        request.clone(),
        received_at_ms,
        clock,
        Arc::new(CountingIds::from(0xa000 + 0x100 * minted)),
    )
    .await
    .unwrap()
}

fn refused(reason: DeviceLoginRefusal) -> DeviceLoginDecision {
    DeviceLoginDecision::Refused { reason }
}

/// The refusal leaves every row as it was.
async fn assert_refused_unchanged(
    store: EnrollmentStore,
    request: &DecideDeviceLogin,
    received_at_ms: i64,
    reason: DeviceLoginRefusal,
) {
    let port = InMemoryEnrollments::with(store.clone());
    let answer = decide(&port, request, received_at_ms, NOW_MS).await;
    assert_eq!(answer, refused(reason));
    assert_eq!(port.snapshot(), store);
}

#[tokio::test]
async fn an_approval_writes_an_approved_enrollment_its_allowances_and_their_records() {
    let port = InMemoryEnrollments::with(empty_store());
    let request = approval(REQUEST, MACHINE, &[WORKSPACE_A, WORKSPACE_B]);

    let answer = decide(&port, &request, NOW_MS, NOW_MS).await;

    // CountingIds mints 0xa000 for the enrollment, then an allowance and its record per workspace.
    let DeviceLoginDecision::Approved { enrollment_id } = answer else {
        panic!("expected Approved, got {answer:?}");
    };
    assert_eq!(enrollment_id, enrollment(0xa000));
    let after = port.snapshot();
    assert_eq!(
        after.enrollments.get(&enrollment_id),
        Some(&StoredEnrollment {
            id: enrollment_id.clone(),
            installation: installation(MACHINE),
            request_id: request.request_id.clone(),
            approved_by: approver(),
            base_generation: EnrollmentGeneration(0),
            approved_at_ms: NOW_MS,
            code_expires_at: request.code_expires_at,
            state: StoredEnrollmentState::Approved,
            ended_at_ms: None,
        })
    );
    assert_eq!(
        after.installations.get(&installation(MACHINE)),
        Some(&InstallationRow {
            installation: installation(MACHINE),
            label: label(),
            generation: EnrollmentGeneration(0),
            active: None,
        })
    );
    let allowed = after
        .allowances
        .iter()
        .map(|row| (row.workspace.clone(), row.state.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        allowed,
        vec![
            (workspace(WORKSPACE_A), AllowanceState::Active),
            (workspace(WORKSPACE_B), AllowanceState::Active)
        ]
    );
    // Approved is internal: no machineEnrolled until the first Grant.
    let records = after
        .records
        .iter()
        .map(|record| (record.kind, record.target_id.clone(), record.origin.clone()))
        .collect::<Vec<_>>();
    let origin = OrganizationChangeOrigin::Person {
        person_id: uuid7(APPROVER),
        permission_checked: "machine:enroll".to_owned(),
    };
    assert_eq!(
        records,
        after
            .allowances
            .iter()
            .map(|row| (
                OrganizationChangeKind::AllowanceChanged,
                row.id.to_string(),
                origin.clone()
            ))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_repeat_of_the_same_request_answers_the_same_enrollment_and_writes_nothing() {
    let port = InMemoryEnrollments::with(empty_store());
    let request = approval(REQUEST, MACHINE, &[WORKSPACE_A]);
    let first = decide(&port, &request, NOW_MS, NOW_MS).await;
    let after_first = port.snapshot();

    // A later resend, with a fresh receipt and a later clock.
    let repeat = decide(&port, &request, NOW_MS + 60_000, NOW_MS + 60_000).await;

    assert_eq!(repeat, first);
    assert_eq!(port.snapshot(), after_first);
}

#[tokio::test]
async fn a_changed_body_under_a_used_request_id_is_not_permitted() {
    let port = InMemoryEnrollments::with(empty_store());
    decide(
        &port,
        &approval(REQUEST, MACHINE, &[WORKSPACE_A]),
        NOW_MS,
        NOW_MS,
    )
    .await;
    let store = port.snapshot();

    assert_refused_unchanged(
        store,
        &approval(REQUEST, MACHINE, &[WORKSPACE_B]),
        NOW_MS,
        DeviceLoginRefusal::NotPermitted {},
    )
    .await;
}

#[tokio::test]
async fn a_workspace_this_organization_lacks_refuses_the_whole_approval() {
    let mut store = empty_store();
    store.workspaces.remove(&workspace(WORKSPACE_B));

    assert_refused_unchanged(
        store,
        &approval(REQUEST, MACHINE, &[WORKSPACE_A, WORKSPACE_B]),
        NOW_MS,
        DeviceLoginRefusal::UnknownWorkspace {},
    )
    .await;
}

#[tokio::test]
async fn a_machine_another_organization_holds_is_refused() {
    let mut store = empty_store();
    store.claimed_elsewhere.insert(uuid7(MACHINE));

    assert_refused_unchanged(
        store,
        &approval(REQUEST, MACHINE, &[WORKSPACE_A]),
        NOW_MS,
        DeviceLoginRefusal::MachineInAnotherOrganization {},
    )
    .await;
}

#[tokio::test]
async fn sd8_accepts_exactly_five_seconds_after_receipt_and_refuses_one_millisecond_more() {
    let request = approval(REQUEST, MACHINE, &[WORKSPACE_A]);
    let within = InMemoryEnrollments::with(empty_store());
    assert!(matches!(
        decide(&within, &request, NOW_MS - SD8_MILLIS, NOW_MS).await,
        DeviceLoginDecision::Approved { .. }
    ));

    assert_refused_unchanged(
        empty_store(),
        &request,
        NOW_MS - SD8_MILLIS - 1,
        DeviceLoginRefusal::NotPermitted {},
    )
    .await;
}

/// A B3 machine is its peer (B3:126): a login naming another peer is refused,
/// and nothing is written.
#[tokio::test]
async fn a_device_login_whose_peer_is_not_its_installation_is_not_permitted() {
    let request = DecideDeviceLogin {
        peer: fleet_sync_protocol::PeerId::try_from(uuid7(0x11)).unwrap(),
        ..approval(REQUEST, MACHINE, &[WORKSPACE_A])
    };

    assert_refused_unchanged(
        empty_store(),
        &request,
        NOW_MS,
        DeviceLoginRefusal::NotPermitted {},
    )
    .await;
}

/// The applying clock is read inside the transaction: a wait before the
/// decision runs counts against SD8.
#[tokio::test]
async fn sd8_is_measured_on_the_clock_read_after_waiting_for_the_transaction() {
    let port = InMemoryEnrollments::with(empty_store());
    let clock = TestClock::at(NOW_MS);
    let held = clock.clone();
    port.before_next_decision(move || held.set(NOW_MS + SD8_MILLIS + 1));

    let answer = decide_device_login(
        &port,
        approval(REQUEST, MACHINE, &[WORKSPACE_A]),
        NOW_MS,
        clock,
        Arc::new(CountingIds::new()),
    )
    .await
    .unwrap();

    assert_eq!(answer, refused(DeviceLoginRefusal::NotPermitted {}));
    assert_eq!(port.snapshot(), empty_store());
}

fn second_factor(at_seconds: Option<i64>) -> DecideDeviceLogin {
    DecideDeviceLogin {
        approver_second_factor_at: at_seconds.map(UnixSeconds),
        ..approval(REQUEST, MACHINE, &[WORKSPACE_A])
    }
}

/// Any other machine in the organization, in any state, ends the exemption.
fn organization_with_history(state: StoredEnrollmentState) -> EnrollmentStore {
    let mut store = with_enrolled(empty_store(), OTHER_MACHINE, 0x40, 1, NOW_S);
    if let Some(other) = store.enrollments.get_mut(&enrollment(0x40)) {
        other.state = state;
    }
    if !matches!(
        store
            .enrollments
            .get(&enrollment(0x40))
            .map(|row| &row.state),
        Some(StoredEnrollmentState::Enrolled { .. })
    ) && let Some(row) = store.installations.get_mut(&installation(OTHER_MACHINE))
    {
        row.active = None;
    }
    store
}

#[tokio::test]
async fn the_second_factor_counts_for_900_seconds_once_the_organization_has_a_machine() {
    let store = organization_with_history(StoredEnrollmentState::Expired {
        machine_client_id: super::super::test_fixtures::client(0x40),
    });
    let fresh = InMemoryEnrollments::with(store.clone());
    assert!(matches!(
        decide(&fresh, &second_factor(Some(NOW_S - 900)), NOW_MS, NOW_MS).await,
        DeviceLoginDecision::Approved { .. }
    ));
    let required = DeviceLoginRefusal::SecondFactorRequired {
        within_seconds: NonZeroU32::new(900).unwrap(),
    };
    for stale in [Some(NOW_S - 901), Some(NOW_S + 1), None] {
        assert_refused_unchanged(
            store.clone(),
            &second_factor(stale),
            NOW_MS,
            required.clone(),
        )
        .await;
    }
}

#[tokio::test]
async fn only_an_organization_that_never_had_a_machine_skips_the_second_factor() {
    let first = InMemoryEnrollments::with(empty_store());
    assert!(matches!(
        decide(&first, &second_factor(None), NOW_MS, NOW_MS).await,
        DeviceLoginDecision::Approved { .. }
    ));
    let required = DeviceLoginRefusal::SecondFactorRequired {
        within_seconds: NonZeroU32::new(900).unwrap(),
    };
    for history in [
        StoredEnrollmentState::Approved,
        StoredEnrollmentState::Revoked {
            machine_client_id: super::super::test_fixtures::client(0x40),
            reason: crate::identity_contract::EnrollmentRevocationReason::Owner {},
        },
    ] {
        assert_refused_unchanged(
            organization_with_history(history),
            &second_factor(None),
            NOW_MS,
            required.clone(),
        )
        .await;
    }
}

#[tokio::test]
async fn approval_records_the_installations_current_generation_as_its_base() {
    // A previous enrollment of the same machine moved the generation to 3.
    let store = with_enrolled(empty_store(), MACHINE, 0x40, 3, NOW_S);
    let port = InMemoryEnrollments::with(store);

    let answer = decide(
        &port,
        &approval(REQUEST, MACHINE, &[WORKSPACE_A]),
        NOW_MS,
        NOW_MS,
    )
    .await;

    let DeviceLoginDecision::Approved { enrollment_id } = answer else {
        panic!("expected Approved, got {answer:?}");
    };
    let after = port.snapshot();
    let approved = after.enrollments.get(&enrollment_id).unwrap();
    assert_eq!(approved.base_generation, EnrollmentGeneration(3));
    // The working enrollment is untouched until the new one's first Grant.
    let row = after.installations.get(&installation(MACHINE)).unwrap();
    assert_eq!(
        (row.generation, row.active.clone()),
        (EnrollmentGeneration(3), Some(enrollment(0x40)))
    );
    // Workspace A was already allowed: no new allowance and no record.
    assert_eq!(after.allowances.len(), 1);
    assert!(after.records.is_empty());
}

fn former(value: &str) -> FormerServiceId {
    FormerServiceId::try_from(value.to_owned()).unwrap()
}
const FORMER_V4: &str = "0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7d";

fn rekeying(request: u64, machine: u64) -> DecideDeviceLogin {
    DecideDeviceLogin {
        former_service_id: Some(former(FORMER_V4)),
        ..approval(request, machine, &[WORKSPACE_A])
    }
}

/// An installation still keyed by its former UUIDv4 service id, enrolled and allowed.
fn pre_rekey_store() -> EnrollmentStore {
    let mut store = empty_store();
    let old = fleet_identity_protocol::InstallationId::try_from(FORMER_V4.to_owned()).unwrap();
    let seeded = with_enrolled(EnrollmentStore::default(), MACHINE, 0x40, 2, NOW_S);
    for (_, mut row) in seeded.enrollments {
        row.installation = old.clone();
        store.enrollments.insert(row.id.clone(), row);
    }
    for (_, mut row) in seeded.installations {
        row.installation = old.clone();
        store.installations.insert(old.clone(), row);
    }
    for mut row in seeded.allowances {
        row.installation = old.clone();
        store.allowances.push(row);
    }
    store
}

#[tokio::test]
async fn a_former_service_id_converts_its_installation_once_and_a_repeat_mapping_is_a_no_op() {
    let port = InMemoryEnrollments::with(pre_rekey_store());

    let first = decide(&port, &rekeying(REQUEST, MACHINE), NOW_MS, NOW_MS).await;

    assert!(matches!(first, DeviceLoginDecision::Approved { .. }));
    let after = port.snapshot();
    let old = fleet_identity_protocol::InstallationId::try_from(FORMER_V4.to_owned()).unwrap();
    assert!(!after.installations.contains_key(&old));
    assert_eq!(
        after
            .installations
            .get(&installation(MACHINE))
            .map(|row| (row.generation, row.active.clone())),
        Some((EnrollmentGeneration(2), Some(enrollment(0x40))))
    );
    assert!(
        after
            .allowances
            .iter()
            .all(|row| row.installation == installation(MACHINE))
    );
    assert!(
        after
            .enrollments
            .values()
            .all(|row| row.installation == installation(MACHINE))
    );
    assert_eq!(
        after.links,
        vec![(former(FORMER_V4), installation(MACHINE))]
    );

    // The Router sends the former id with every later login: the same mapping changes nothing more.
    let again = decide(&port, &rekeying(REQUEST + 1, MACHINE), NOW_MS, NOW_MS).await;
    assert!(matches!(again, DeviceLoginDecision::Approved { .. }));
    assert_eq!(port.snapshot().links, after.links);
    assert_eq!(port.snapshot().installations, after.installations);
}

#[tokio::test]
async fn a_former_service_id_linked_to_another_installation_is_not_linkable_here() {
    let port = InMemoryEnrollments::with(pre_rekey_store());
    decide(&port, &rekeying(REQUEST, MACHINE), NOW_MS, NOW_MS).await;
    let linked = port.snapshot();

    assert_refused_unchanged(
        linked,
        &rekeying(REQUEST + 1, OTHER_MACHINE),
        NOW_MS,
        DeviceLoginRefusal::FormerServiceLinkedElsewhere {},
    )
    .await;
}

#[tokio::test]
async fn a_former_service_id_naming_nothing_here_links_nothing() {
    let port = InMemoryEnrollments::with(empty_store());

    let answer = decide(&port, &rekeying(REQUEST, MACHINE), NOW_MS, NOW_MS).await;

    assert!(matches!(answer, DeviceLoginDecision::Approved { .. }));
    assert!(port.snapshot().links.is_empty());
}
