//! Admission and revocation over the in-memory port, through their public
//! entries. Each admission case keeps every other field valid, so the one
//! wrong field decides it.
use super::super::enrollment_records::{EnrollmentGeneration, StoredEnrollmentState};
use super::super::in_memory_enrollments::{EnrollmentStore, InMemoryEnrollments};
use super::super::machine_token_decision::decide_machine_token;
use super::super::test_fixtures::{
    CountingIds, MACHINE, NOW_MS, NOW_S, TestClock, client, empty_store, enrollment, installation,
    settings, uuid7, with_approved, with_enrolled,
};
use super::{
    CloseMachineConnection, RevocationBy, admit_machine_connection, revoke_machine_enrollment,
};
use crate::identity_contract::{
    AdmitMachineConnection, AdmittedMachine, AllowanceState, CommandRefusal, DecideMachineToken,
    EndedMachineClient, EnrollmentRevocationReason, MachineConnectionAdmission,
    MachineConnectionRefusal, MachineTokenDecision, MachineTokenRefusal, PersonId,
    PresentedMachine, RevokeMachineEnrollment, RevokeMachineEnrollmentResult,
};
use crate::organization_authority::{OrganizationChangeKind, OrganizationChangeOrigin};
use crate::person_request::AdminOperationId;
use fleet_identity_protocol::{OrganizationId, UnixSeconds};
use fleet_sync_protocol::PeerId;
use std::sync::Arc;

const WORKING: u64 = 0x40;
const A: u64 = 0x41;
const B: u64 = 0x42;
const OWNER: u64 = 0x03;
const DAY_S: i64 = 24 * 60 * 60;

fn organization(suffix: u64) -> OrganizationId {
    OrganizationId::try_from(uuid7(suffix)).unwrap()
}
fn peer(suffix: u64) -> PeerId {
    PeerId::try_from(uuid7(suffix)).unwrap()
}

/// A valid connection for the working enrollment of `MACHINE`.
fn connection() -> AdmitMachineConnection {
    AdmitMachineConnection {
        organization_id: organization(0x01),
        enrollment_id: enrollment(WORKING),
        installation: installation(MACHINE),
        presented: PresentedMachine {
            installation: installation(MACHINE),
            peer: peer(MACHINE),
        },
        token_expires_at: UnixSeconds(NOW_S + 900),
        client_id: client(WORKING),
    }
}

fn working_store() -> EnrollmentStore {
    with_enrolled(empty_store(), MACHINE, WORKING, 1, NOW_S - 60)
}

async fn admit(
    port: &InMemoryEnrollments,
    request: &AdmitMachineConnection,
) -> MachineConnectionAdmission {
    let clock = TestClock::at(NOW_MS);
    let minted = u64::try_from(port.snapshot().records.len()).unwrap_or(0);
    admit_machine_connection(
        port,
        request.clone(),
        clock,
        Arc::new(CountingIds::from(0xd800 + 0x100 * minted)),
        settings(),
    )
    .await
    .unwrap()
}

async fn assert_admission_refused(
    store: EnrollmentStore,
    request: AdmitMachineConnection,
    reason: MachineConnectionRefusal,
) {
    let port = InMemoryEnrollments::with(store.clone());
    assert_eq!(
        admit(&port, &request).await,
        MachineConnectionAdmission::Refused { reason }
    );
    assert_eq!(port.snapshot(), store);
}

#[tokio::test]
async fn the_active_enrollment_on_its_own_machine_and_client_is_admitted() {
    let port = InMemoryEnrollments::with(working_store());

    assert_eq!(
        admit(&port, &connection()).await,
        MachineConnectionAdmission::Admitted {
            machine: AdmittedMachine {
                installation: installation(MACHINE),
                peer: peer(MACHINE),
                organization_id: organization(0x01),
                enrollment_id: enrollment(WORKING),
                token_expires_at: UnixSeconds(NOW_S + 900),
            },
        }
    );
    assert_eq!(port.snapshot(), working_store());
}

#[tokio::test]
async fn a_connection_naming_another_machine_anywhere_is_an_unknown_machine() {
    let unknown = MachineConnectionRefusal::UnknownMachine {};
    assert_admission_refused(
        working_store(),
        AdmitMachineConnection {
            enrollment_id: enrollment(0x77),
            ..connection()
        },
        unknown.clone(),
    )
    .await;
    // The token names another installation than its enrollment's.
    assert_admission_refused(
        working_store(),
        AdmitMachineConnection {
            installation: installation(0x11),
            ..connection()
        },
        unknown.clone(),
    )
    .await;
    // The connection presents another installation, or the right one with another peer.
    for presented in [
        PresentedMachine {
            installation: installation(0x11),
            peer: peer(MACHINE),
        },
        PresentedMachine {
            installation: installation(MACHINE),
            peer: peer(0x11),
        },
    ] {
        assert_admission_refused(
            working_store(),
            AdmitMachineConnection {
                presented,
                ..connection()
            },
            unknown.clone(),
        )
        .await;
    }
}

#[tokio::test]
async fn a_token_naming_another_organization_is_refused() {
    assert_admission_refused(
        working_store(),
        AdmitMachineConnection {
            organization_id: organization(0x09),
            ..connection()
        },
        MachineConnectionRefusal::MachineInAnotherOrganization {},
    )
    .await;
}

#[tokio::test]
async fn a_client_other_than_the_recorded_one_is_refused() {
    assert_admission_refused(
        working_store(),
        AdmitMachineConnection {
            client_id: client(0x99),
            ..connection()
        },
        MachineConnectionRefusal::WrongClient {},
    )
    .await;
}

#[tokio::test]
async fn a_superseded_enrollment_is_refused_after_the_newer_first_grant() {
    let port = InMemoryEnrollments::with(with_approved(working_store(), MACHINE, A, 1, NOW_MS));
    let clock = TestClock::at(NOW_MS);
    decide_machine_token(
        &port,
        DecideMachineToken {
            organization_id: organization(0x01),
            enrollment_id: enrollment(A),
            client_id: client(A),
        },
        clock,
        Arc::new(CountingIds::new()),
        settings(),
    )
    .await
    .unwrap();

    assert_admission_refused(
        port.snapshot(),
        connection(),
        MachineConnectionRefusal::Superseded {},
    )
    .await;
}

#[tokio::test]
async fn an_idle_enrollment_expires_at_admission_with_the_exact_expiry() {
    let store = with_enrolled(empty_store(), MACHINE, WORKING, 1, NOW_S - 30 * DAY_S);
    let port = InMemoryEnrollments::with(store.clone());

    assert_eq!(
        admit(&port, &connection()).await,
        MachineConnectionAdmission::Refused {
            reason: MachineConnectionRefusal::EnrollmentNotActive {},
        }
    );
    let after = port.snapshot();
    let mut expected = store;
    if let Some(row) = expected.enrollments.get_mut(&enrollment(WORKING)) {
        row.state = StoredEnrollmentState::Expired {
            machine_client_id: client(WORKING),
        };
        row.ended_at_ms = Some(NOW_MS);
    }
    if let Some(row) = expected.installations.get_mut(&installation(MACHINE)) {
        row.active = None;
    }
    for allowance in &mut expected.allowances {
        allowance.state = AllowanceState::Withdrawn;
    }
    expected.records.clone_from(&after.records);
    assert_eq!(after, expected);
    // The admission that first observes the expiry records it, automatically (N07).
    let kinds = after
        .records
        .iter()
        .map(|record| (record.kind, record.origin.clone()))
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            (
                OrganizationChangeKind::MachineExpired,
                OrganizationChangeOrigin::Automatic {}
            ),
            (
                OrganizationChangeKind::AllowanceChanged,
                OrganizationChangeOrigin::Automatic {}
            ),
        ]
    );
}

fn by_owner() -> RevocationBy {
    RevocationBy {
        person: PersonId::try_from(uuid7(OWNER)).unwrap(),
        operation: AdminOperationId::try_from(uuid7(0x50)).unwrap(),
    }
}

fn revoke_request() -> RevokeMachineEnrollment {
    RevokeMachineEnrollment {
        organization_id: organization(0x01),
        installation: installation(MACHINE),
    }
}

async fn revoke(port: &InMemoryEnrollments, now_ms: i64) -> super::MachineRevocation {
    let clock = TestClock::at(now_ms);
    revoke_machine_enrollment(
        port,
        &revoke_request(),
        by_owner(),
        clock,
        Arc::new(CountingIds::from(0xc000)),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn revocation_ends_the_enrollment_bumps_the_generation_and_withdraws_its_allowances() {
    let port = InMemoryEnrollments::with(working_store());
    let before = port.snapshot();

    let revocation = revoke(&port, NOW_MS).await;

    assert_eq!(
        revocation.answer,
        RevokeMachineEnrollmentResult::Applied {
            ended: EndedMachineClient {
                enrollment_id: enrollment(WORKING),
                client_id: client(WORKING),
            },
        }
    );
    assert_eq!(
        revocation.close,
        Some(CloseMachineConnection {
            installation: installation(MACHINE),
            enrollment_id: enrollment(WORKING),
        })
    );
    let after = port.snapshot();
    let mut expected = before;
    if let Some(row) = expected.enrollments.get_mut(&enrollment(WORKING)) {
        row.state = StoredEnrollmentState::Revoked {
            machine_client_id: client(WORKING),
            reason: EnrollmentRevocationReason::Owner {},
        };
        row.ended_at_ms = Some(NOW_MS);
    }
    if let Some(row) = expected.installations.get_mut(&installation(MACHINE)) {
        row.generation = EnrollmentGeneration(2);
        row.active = None;
    }
    for allowance in &mut expected.allowances {
        allowance.state = AllowanceState::Withdrawn;
    }
    expected.records.clone_from(&after.records);
    assert_eq!(after, expected);
    let records = after
        .records
        .iter()
        .map(|record| {
            (
                record.kind,
                record.target_id.clone(),
                record.request_id.as_str().to_owned(),
                record.origin.clone(),
            )
        })
        .collect::<Vec<_>>();
    // Every record is attributed to the revoker, with machine:revoke.
    let revoker = OrganizationChangeOrigin::Person {
        person_id: uuid7(OWNER),
        permission_checked: "machine:revoke".to_owned(),
    };
    assert_eq!(
        records,
        vec![
            (
                OrganizationChangeKind::MachineRevoked,
                uuid7(WORKING),
                format!("{}:machineRevoked:{}", uuid7(0x50), uuid7(WORKING)),
                revoker.clone()
            ),
            (
                OrganizationChangeKind::AllowanceChanged,
                after.allowances[0].id.to_string(),
                format!(
                    "{}:allowanceChanged:{}",
                    uuid7(0x50),
                    after.allowances[0].id
                ),
                revoker
            ),
        ]
    );
    // The machine gets nothing more: no token, no connection.
    let clock = TestClock::at(NOW_MS + 1000);
    let token = decide_machine_token(
        &port,
        DecideMachineToken {
            organization_id: organization(0x01),
            enrollment_id: enrollment(WORKING),
            client_id: client(WORKING),
        },
        clock,
        Arc::new(CountingIds::from(0xd000)),
        settings(),
    )
    .await
    .unwrap();
    assert_eq!(
        token,
        MachineTokenDecision::Refuse {
            reason: MachineTokenRefusal::EnrollmentNotActive {},
        }
    );
    assert_admission_refused(
        port.snapshot(),
        connection(),
        MachineConnectionRefusal::EnrollmentNotActive {},
    )
    .await;
}

#[tokio::test]
async fn revoking_a_machine_with_no_active_enrollment_is_not_found() {
    let store = with_approved(empty_store(), MACHINE, A, 0, NOW_MS);
    let port = InMemoryEnrollments::with(store.clone());

    let revocation = revoke(&port, NOW_MS).await;

    assert_eq!(
        revocation.answer,
        RevokeMachineEnrollmentResult::Refused {
            reason: CommandRefusal::NotFound {},
        }
    );
    assert_eq!(revocation.close, None);
    assert_eq!(port.snapshot(), store);
}

const C: u64 = 0x43;

fn ask(id: u64) -> DecideMachineToken {
    DecideMachineToken {
        organization_id: organization(0x01),
        enrollment_id: enrollment(id),
        client_id: client(id),
    }
}

async fn token(port: &InMemoryEnrollments, id: u64, now_ms: i64, ids: u64) -> MachineTokenDecision {
    let clock = TestClock::at(now_ms);
    decide_machine_token(
        port,
        ask(id),
        clock,
        Arc::new(CountingIds::from(ids)),
        settings(),
    )
    .await
    .unwrap()
}

/// BA1 (4), ABA: A is approved at g; B activates (g+1); C is approved while
/// B is active (at g+1); B is revoked (g+2). Neither late first Grant
/// activates: a revocation ends every approval made before it. C's refusal
/// rests on the revocation's own bump.
#[tokio::test]
async fn a_revocation_ends_every_approval_made_before_it() {
    let store = with_approved(
        with_approved(empty_store(), MACHINE, A, 0, NOW_MS),
        MACHINE,
        B,
        0,
        NOW_MS,
    );
    let granting = InMemoryEnrollments::with(store);
    assert!(matches!(
        token(&granting, B, NOW_MS + 1000, 0xe000).await,
        MachineTokenDecision::Grant { .. }
    ));
    let port = InMemoryEnrollments::with(with_approved(
        granting.snapshot(),
        MACHINE,
        C,
        1,
        NOW_MS + 1500,
    ));
    revoke(&port, NOW_MS + 2000).await;
    let before = port.snapshot();

    for late in [A, C] {
        assert_eq!(
            token(&port, late, NOW_MS + 3000, 0xf000).await,
            MachineTokenDecision::Refuse {
                reason: MachineTokenRefusal::Superseded {},
            }
        );
    }
    assert_eq!(port.snapshot(), before);
}
