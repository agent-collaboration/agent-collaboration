//! `decide_machine_token` over the in-memory port. Refusals that change nothing
//! compare the whole store; the ones that do (idle expiry, lapse) assert the
//! exact transition.
use super::super::decision_support::MachineLoginSettings;
use super::super::enrollment_records::{
    EnrollmentGeneration, InstallationRow, StoredEnrollmentState,
};
use super::super::in_memory_enrollments::{EnrollmentStore, InMemoryEnrollments};
use super::super::test_fixtures::{
    APPROVER, CountingIds, MACHINE, NOW_MS, NOW_S, TestClock, client, empty_store, enrollment,
    installation, settings, uuid7, with_approved, with_enrolled,
};
use super::decide_machine_token;
use crate::identity_contract::{
    AllowanceState, DecideMachineToken, EndedMachineClient, EnrollmentRevocationReason,
    MachineTokenDecision, MachineTokenRefusal,
};
use crate::organization_authority::{OrganizationChangeKind, OrganizationChangeOrigin};
use fleet_identity_protocol::{OrganizationId, UnixSeconds};

const WORKING: u64 = 0x40;
const A: u64 = 0x41;
const B: u64 = 0x42;
const DAY_S: i64 = 24 * 60 * 60;

fn ask(id: u64, client_suffix: u64) -> DecideMachineToken {
    DecideMachineToken {
        organization_id: OrganizationId::try_from(uuid7(0x01)).unwrap(),
        enrollment_id: enrollment(id),
        client_id: client(client_suffix),
    }
}

/// The first Grant names the enrollment's own client: its ID.
fn first(id: u64) -> DecideMachineToken {
    ask(id, id)
}

async fn decide_with(
    port: &InMemoryEnrollments,
    request: &DecideMachineToken,
    now_ms: i64,
    settings: MachineLoginSettings,
) -> MachineTokenDecision {
    let clock = TestClock::at(now_ms);
    let minted = u64::try_from(port.snapshot().records.len()).unwrap_or(0);
    decide_machine_token(
        port,
        request,
        clock.as_ref(),
        &CountingIds::from(0xb000 + 0x100 * minted),
        settings,
    )
    .await
    .unwrap()
}

async fn decide(
    port: &InMemoryEnrollments,
    request: &DecideMachineToken,
    now_ms: i64,
) -> MachineTokenDecision {
    decide_with(port, request, now_ms, settings()).await
}

fn refused(reason: MachineTokenRefusal) -> MachineTokenDecision {
    MachineTokenDecision::Refuse { reason }
}

fn grant(id: u64, now_s: i64, superseded: Option<EndedMachineClient>) -> MachineTokenDecision {
    MachineTokenDecision::Grant {
        installation: installation(MACHINE),
        enrollment_id: enrollment(id),
        decided_at: UnixSeconds(now_s),
        grant_expires_at: UnixSeconds(now_s + 900),
        superseded,
    }
}

fn ended(id: u64, client_suffix: u64) -> EndedMachineClient {
    EndedMachineClient {
        enrollment_id: enrollment(id),
        client_id: client(client_suffix),
    }
}

fn row(store: &EnrollmentStore) -> InstallationRow {
    store
        .installations
        .get(&installation(MACHINE))
        .cloned()
        .unwrap()
}

fn state(store: &EnrollmentStore, id: u64) -> StoredEnrollmentState {
    store
        .enrollments
        .get(&enrollment(id))
        .unwrap()
        .state
        .clone()
}

async fn assert_refused_unchanged(
    port: &InMemoryEnrollments,
    request: &DecideMachineToken,
    now_ms: i64,
    reason: MachineTokenRefusal,
) {
    let before = port.snapshot();
    assert_eq!(decide(port, request, now_ms).await, refused(reason));
    assert_eq!(port.snapshot(), before);
}

/// A working enrollment at generation 1 and a newer approval made at it.
fn relogin_store() -> EnrollmentStore {
    with_approved(
        with_enrolled(empty_store(), MACHINE, WORKING, 1, NOW_S - 60),
        MACHINE,
        A,
        1,
        NOW_MS - 60_000,
    )
}

#[tokio::test]
async fn a_first_grant_activates_supersedes_the_working_enrollment_and_bumps_the_generation() {
    let port = InMemoryEnrollments::with(relogin_store());
    let before = port.snapshot();

    let answer = decide(&port, &first(A), NOW_MS).await;

    assert_eq!(answer, grant(A, NOW_S, Some(ended(WORKING, WORKING))));
    let after = port.snapshot();
    assert_eq!(
        row(&after),
        InstallationRow {
            installation: installation(MACHINE),
            generation: EnrollmentGeneration(2),
            active: Some(enrollment(A)),
        }
    );
    assert_eq!(
        state(&after, A),
        StoredEnrollmentState::Enrolled {
            machine_client_id: client(A),
            last_grant_at: UnixSeconds(NOW_S),
        }
    );
    assert_eq!(
        state(&after, WORKING),
        StoredEnrollmentState::Revoked {
            machine_client_id: client(WORKING),
            reason: EnrollmentRevocationReason::Superseded {},
        }
    );
    // Supersession keeps the machine's allowances.
    assert_eq!(after.allowances, before.allowances);
    let records = after
        .records
        .iter()
        .map(|record| (record.kind, record.target_id.clone(), record.origin.clone()))
        .collect::<Vec<_>>();
    // Both records are attributed to the approval: its approver, with machine:enroll.
    let approver = OrganizationChangeOrigin::Person {
        person_id: uuid7(APPROVER),
        permission_checked: "machine:enroll".to_owned(),
    };
    assert_eq!(
        records,
        vec![
            (
                OrganizationChangeKind::MachineRevoked,
                uuid7(WORKING),
                approver.clone()
            ),
            (OrganizationChangeKind::MachineEnrolled, uuid7(A), approver),
        ]
    );
}

#[tokio::test]
async fn a_machines_first_ever_grant_supersedes_nothing() {
    let port = InMemoryEnrollments::with(with_approved(empty_store(), MACHINE, A, 0, NOW_MS));

    assert_eq!(
        decide(&port, &first(A), NOW_MS).await,
        grant(A, NOW_S, None)
    );
    assert_eq!(row(&port.snapshot()).generation, EnrollmentGeneration(1));
}

/// BA1 (1) and (2): of two approvals made at the same generation, the first
/// Grant wins, whichever was approved first; the loser can never activate.
#[tokio::test]
async fn of_two_approvals_at_one_generation_the_first_grant_wins_in_either_order() {
    for (winner, loser) in [(B, A), (A, B)] {
        let store = with_approved(relogin_store(), MACHINE, B, 1, NOW_MS - 30_000);
        let port = InMemoryEnrollments::with(store);
        assert!(matches!(
            decide(&port, &first(winner), NOW_MS).await,
            MachineTokenDecision::Grant { .. }
        ));

        assert_refused_unchanged(
            &port,
            &first(loser),
            NOW_MS + 1000,
            MachineTokenRefusal::Superseded {},
        )
        .await;
        assert_eq!(row(&port.snapshot()).active, Some(enrollment(winner)));
        assert_eq!(
            state(&port.snapshot(), loser),
            StoredEnrollmentState::Approved
        );
    }
}

/// BA1 (3): a repeat for an enrollment a newer one superseded mints nothing.
#[tokio::test]
async fn a_repeat_for_a_superseded_enrollment_is_refused() {
    let port = InMemoryEnrollments::with(relogin_store());
    decide(&port, &first(A), NOW_MS).await;

    assert_refused_unchanged(
        &port,
        &ask(WORKING, WORKING),
        NOW_MS + 1000,
        MachineTokenRefusal::Superseded {},
    )
    .await;
}

/// BA1 (5): a repeat of the active enrollment grants and only moves its
/// idle clock: no generation, no supersession, no record.
#[tokio::test]
async fn a_repeat_grant_of_the_active_enrollment_only_moves_its_last_grant() {
    let port = InMemoryEnrollments::with(relogin_store());
    decide(&port, &first(A), NOW_MS).await;
    let before = port.snapshot();

    let later_s = NOW_S + 600;
    let answer = decide(&port, &ask(A, A), later_s * 1000).await;

    assert_eq!(answer, grant(A, later_s, None));
    let mut expected = before;
    if let Some(enrollment) = expected.enrollments.get_mut(&enrollment(A)) {
        enrollment.state = StoredEnrollmentState::Enrolled {
            machine_client_id: client(A),
            last_grant_at: UnixSeconds(later_s),
        };
    }
    assert_eq!(port.snapshot(), expected);
}

/// An approval no one ever redeems leaves the working enrollment as it was.
#[tokio::test]
async fn an_approval_without_a_grant_leaves_the_working_enrollment_active() {
    let port = InMemoryEnrollments::with(relogin_store());

    let answer = decide(&port, &ask(WORKING, WORKING), NOW_MS).await;

    assert_eq!(
        answer,
        MachineTokenDecision::Grant {
            installation: installation(MACHINE),
            enrollment_id: enrollment(WORKING),
            decided_at: UnixSeconds(NOW_S),
            grant_expires_at: UnixSeconds(NOW_S + 900),
            superseded: None,
        }
    );
    assert_eq!(row(&port.snapshot()).active, Some(enrollment(WORKING)));
    assert_eq!(state(&port.snapshot(), A), StoredEnrollmentState::Approved);
}

#[tokio::test]
async fn a_client_other_than_the_enrollments_own_is_refused_first_and_later() {
    let port = InMemoryEnrollments::with(relogin_store());
    assert_refused_unchanged(
        &port,
        &ask(A, 0x99),
        NOW_MS,
        MachineTokenRefusal::WrongClient {},
    )
    .await;
    decide(&port, &first(A), NOW_MS).await;
    assert_refused_unchanged(
        &port,
        &ask(A, 0x99),
        NOW_MS + 1000,
        MachineTokenRefusal::WrongClient {},
    )
    .await;
}

#[tokio::test]
async fn an_unknown_enrollment_is_not_active() {
    let port = InMemoryEnrollments::with(relogin_store());
    assert_refused_unchanged(
        &port,
        &first(0x77),
        NOW_MS,
        MachineTokenRefusal::EnrollmentNotActive {},
    )
    .await;
}

#[tokio::test]
async fn thirty_days_without_a_grant_expires_the_enrollment_and_withdraws_its_allowances() {
    let last_s = NOW_S - 30 * DAY_S;
    let store = with_enrolled(empty_store(), MACHINE, WORKING, 1, last_s);
    let just_before = InMemoryEnrollments::with(store.clone());
    assert!(matches!(
        decide(&just_before, &ask(WORKING, WORKING), NOW_MS - 1000).await,
        MachineTokenDecision::Grant { .. }
    ));

    let port = InMemoryEnrollments::with(store.clone());
    let answer = decide(&port, &ask(WORKING, WORKING), NOW_MS).await;

    assert_eq!(answer, refused(MachineTokenRefusal::IdleExpired {}));
    // Exactly the expiry: expired, no active enrollment, allowances withdrawn,
    // the generation unchanged, and no record.
    let mut expected = store;
    if let Some(enrollment) = expected.enrollments.get_mut(&enrollment(WORKING)) {
        enrollment.state = StoredEnrollmentState::Expired {
            machine_client_id: client(WORKING),
        };
    }
    if let Some(row) = expected.installations.get_mut(&installation(MACHINE)) {
        row.active = None;
    }
    for allowance in &mut expected.allowances {
        allowance.state = AllowanceState::Withdrawn;
    }
    assert_eq!(port.snapshot(), expected);
    assert_refused_unchanged(
        &port,
        &ask(WORKING, WORKING),
        NOW_MS + 1000,
        MachineTokenRefusal::EnrollmentNotActive {},
    )
    .await;
}

/// Each Grant restarts the idle clock: past the first cutoff but inside the
/// refreshed one still grants.
#[tokio::test]
async fn every_grant_restarts_the_thirty_day_idle_clock() {
    let port = InMemoryEnrollments::with(with_enrolled(empty_store(), MACHINE, WORKING, 1, NOW_S));
    decide(&port, &ask(WORKING, WORKING), (NOW_S + 20 * DAY_S) * 1000).await;

    let answer = decide(&port, &ask(WORKING, WORKING), (NOW_S + 40 * DAY_S) * 1000).await;

    assert!(matches!(answer, MachineTokenDecision::Grant { .. }));
}

#[tokio::test]
async fn the_grant_expires_after_the_configured_lifetime() {
    let port = InMemoryEnrollments::with(with_approved(empty_store(), MACHINE, A, 0, NOW_MS));
    let mut short = settings();
    short.grant_lifetime = crate::identity_contract::TokenLifetimeSeconds::try_from(60).unwrap();

    let answer = decide_with(&port, &first(A), NOW_MS, short).await;

    let MachineTokenDecision::Grant {
        decided_at,
        grant_expires_at,
        ..
    } = answer
    else {
        panic!("expected a Grant, got {answer:?}");
    };
    assert_eq!(
        (decided_at, grant_expires_at),
        (UnixSeconds(NOW_S), UnixSeconds(NOW_S + 60))
    );
}

/// An approval lapses a device-code lifetime after central approved it; a
/// late first Grant then displaces nothing.
#[tokio::test]
async fn a_first_grant_at_the_device_code_lifetime_finds_the_approval_lapsed() {
    let approved_at = NOW_MS - MachineLoginSettings::DEVICE_CODE_LIFETIME_MS;
    let store = with_approved(
        with_enrolled(empty_store(), MACHINE, WORKING, 1, NOW_S - 60),
        MACHINE,
        A,
        1,
        approved_at,
    );
    let in_time = InMemoryEnrollments::with(store.clone());
    assert!(matches!(
        decide(&in_time, &first(A), NOW_MS - 1).await,
        MachineTokenDecision::Grant { .. }
    ));

    let port = InMemoryEnrollments::with(store.clone());
    let answer = decide(&port, &first(A), NOW_MS).await;

    assert_eq!(answer, refused(MachineTokenRefusal::EnrollmentNotActive {}));
    let mut expected = store;
    if let Some(enrollment) = expected.enrollments.get_mut(&enrollment(A)) {
        enrollment.state = StoredEnrollmentState::Lapsed;
    }
    assert_eq!(port.snapshot(), expected);
}
