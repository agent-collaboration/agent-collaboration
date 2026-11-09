//! Admission, revocation and the re-key over the real Tokio store.
use super::super::device_login_decision::decide_device_login;
use super::super::enrollment_port::{Decided, EnrollmentQuery, MachineEnrollmentPort};
use super::super::enrollment_records::{EnrollmentGeneration, StoredEnrollmentState};
use super::super::machine_connection::{
    RevocationBy, admit_machine_connection, revoke_machine_enrollment,
};
use super::super::machine_token_decision::decide_machine_token;
use super::super::sql_store_fixture::{FixturePort, LegacyMachine, SqlStoreFixture, TestError};
use super::super::test_fixtures::{
    CountingIds, MACHINE, NOW_MS, NOW_S, TestClock, WORKSPACE_A, approval, client, installation,
    settings, uuid7,
};
use crate::identity_contract::{
    AdmitMachineConnection, DecideDeviceLogin, DecideMachineToken, DeviceLoginDecision,
    DeviceLoginRefusal, FormerServiceId, MachineConnectionAdmission, MachineConnectionRefusal,
    MachineTokenDecision, MachineTokenRefusal, PersonId, PresentedMachine, RevokeMachineEnrollment,
    RevokeMachineEnrollmentResult,
};
use crate::person_request::AdminOperationId;
use fleet_identity_protocol::{EnrollmentId, OrganizationId, UnixSeconds};
use fleet_sync_protocol::PeerId;
use std::sync::Arc;

fn organization() -> OrganizationId {
    OrganizationId::try_from(uuid7(0x01)).unwrap()
}

async fn enrolled(port: &FixturePort, request: u64, at_ms: i64, ids: u64) -> EnrollmentId {
    let answer = decide_device_login(
        port,
        approval(request, MACHINE, &[WORKSPACE_A]),
        at_ms,
        TestClock::at(at_ms),
        Arc::new(CountingIds::from(ids)),
    )
    .await
    .unwrap();
    let DeviceLoginDecision::Approved { enrollment_id } = answer else {
        panic!("expected Approved, got {answer:?}");
    };
    let suffix = u64::from_str_radix(&enrollment_id.as_str()[24..], 16).unwrap();
    let granted = decide_machine_token(
        port,
        DecideMachineToken {
            organization_id: organization(),
            enrollment_id: enrollment_id.clone(),
            client_id: client(suffix),
        },
        TestClock::at(at_ms + 500),
        Arc::new(CountingIds::from(ids + 0x80)),
        settings(),
    )
    .await
    .unwrap();
    assert!(matches!(granted, MachineTokenDecision::Grant { .. }));
    enrollment_id
}

fn connection(id: &EnrollmentId, peer: u64) -> AdmitMachineConnection {
    let suffix = u64::from_str_radix(&id.as_str()[24..], 16).unwrap();
    AdmitMachineConnection {
        organization_id: organization(),
        enrollment_id: id.clone(),
        installation: installation(MACHINE),
        presented: PresentedMachine {
            installation: installation(MACHINE),
            peer: PeerId::try_from(uuid7(peer)).unwrap(),
        },
        token_expires_at: UnixSeconds(NOW_S + 900),
        client_id: client(suffix),
    }
}

async fn admit(port: &FixturePort, request: AdmitMachineConnection) -> MachineConnectionAdmission {
    admit_machine_connection(
        port,
        request,
        TestClock::at(NOW_MS + 2000),
        Arc::new(CountingIds::from(0xd000)),
        settings(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn admission_reads_the_real_store_live() -> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[WORKSPACE_A]).await?;
    let port = fixture.port();
    let id = enrolled(&port, 0x30, NOW_MS, 0xa000).await;
    let before = fixture.rows().await?;

    assert!(matches!(
        admit(&port, connection(&id, MACHINE)).await,
        MachineConnectionAdmission::Admitted { .. }
    ));
    assert_eq!(
        admit(&port, connection(&id, 0x11)).await,
        MachineConnectionAdmission::Refused {
            reason: MachineConnectionRefusal::UnknownMachine {},
        }
    );
    assert_eq!(
        admit(
            &port,
            AdmitMachineConnection {
                organization_id: OrganizationId::try_from(uuid7(0x09)).unwrap(),
                ..connection(&id, MACHINE)
            }
        )
        .await,
        MachineConnectionAdmission::Refused {
            reason: MachineConnectionRefusal::MachineInAnotherOrganization {},
        }
    );
    assert_eq!(
        admit(
            &port,
            AdmitMachineConnection {
                client_id: client(0x99),
                ..connection(&id, MACHINE)
            }
        )
        .await,
        MachineConnectionAdmission::Refused {
            reason: MachineConnectionRefusal::WrongClient {},
        }
    );
    assert_eq!(fixture.rows().await?, before);
    Ok(())
}

/// Revocation bumps the generation, ends every earlier approval (ABA) and
/// leaves the machine nothing: no Grant, no connection.
#[tokio::test]
async fn revocation_on_the_real_store_ends_the_machine_and_every_earlier_approval()
-> Result<(), TestError> {
    let fixture = SqlStoreFixture::b3(&[WORKSPACE_A]).await?;
    let port = fixture.port();
    let id = enrolled(&port, 0x30, NOW_MS, 0xa000).await;
    // An approval made while that enrollment is active.
    let pending = decide_device_login(
        &port,
        approval(0x31, MACHINE, &[WORKSPACE_A]),
        NOW_MS + 1000,
        TestClock::at(NOW_MS + 1000),
        Arc::new(CountingIds::from(0xa100)),
    )
    .await
    .unwrap();
    let DeviceLoginDecision::Approved {
        enrollment_id: pending,
    } = pending
    else {
        panic!("expected Approved");
    };

    let revocation = revoke_machine_enrollment(
        &port,
        &RevokeMachineEnrollment {
            organization_id: organization(),
            installation: installation(MACHINE),
        },
        RevocationBy {
            person: PersonId::try_from(uuid7(0x03)).unwrap(),
            operation: AdminOperationId::try_from(uuid7(0x50)).unwrap(),
        },
        TestClock::at(NOW_MS + 2000),
        Arc::new(CountingIds::from(0xc000)),
    )
    .await
    .unwrap();

    assert!(matches!(
        revocation.answer,
        RevokeMachineEnrollmentResult::Applied { .. }
    ));
    let after = port
        .transact(
            EnrollmentQuery {
                enrollment: Some(id.clone()),
                installation: Some(installation(MACHINE)),
                ..EnrollmentQuery::default()
            },
            |facts| Decided::unchanged(facts.clone()),
        )
        .await
        .unwrap();
    let row = after.installation.unwrap();
    assert_eq!(
        (row.generation, row.active),
        (EnrollmentGeneration(2), None)
    );
    assert!(matches!(
        after.enrollment.unwrap().state,
        StoredEnrollmentState::Revoked { .. }
    ));
    let suffix = u64::from_str_radix(&pending.as_str()[24..], 16).unwrap();
    let late = decide_machine_token(
        &port,
        DecideMachineToken {
            organization_id: organization(),
            enrollment_id: pending,
            client_id: client(suffix),
        },
        TestClock::at(NOW_MS + 3000),
        Arc::new(CountingIds::from(0xb900)),
        settings(),
    )
    .await
    .unwrap();
    assert_eq!(
        late,
        MachineTokenDecision::Refuse {
            reason: MachineTokenRefusal::Superseded {},
        }
    );
    assert_eq!(
        admit(&port, connection(&id, MACHINE)).await,
        MachineConnectionAdmission::Refused {
            reason: MachineConnectionRefusal::EnrollmentNotActive {},
        }
    );
    Ok(())
}

/// After the switch each machine is keyed by its peer id with its former id
/// recorded: the same mapping logs in, another installation can't take it.
#[tokio::test]
async fn a_converted_former_id_links_its_own_machine_only() -> Result<(), TestError> {
    let former = "0b6a1c5e-2f3d-4a8b-9c1d-2e3f4a5b6c7d";
    let fixture = SqlStoreFixture::pre_b3(vec![LegacyMachine {
        peer: uuid7(MACHINE),
        service: former.to_owned(),
        label: "fixture machine".to_owned(),
        last_seen: None,
        created_at: "2026-11-08T12:00:00Z".to_owned(),
    }])
    .await?;
    fixture.initialize_b3().await?;
    fixture.add_workspace(WORKSPACE_A).await?;
    let port = fixture.port();
    let rekey = |request: u64, machine: u64| DecideDeviceLogin {
        former_service_id: Some(FormerServiceId::try_from(former.to_owned()).unwrap()),
        ..approval(request, machine, &[WORKSPACE_A])
    };
    let login = |request: DecideDeviceLogin, ids: u64| {
        decide_device_login(
            &port,
            request,
            NOW_MS,
            TestClock::at(NOW_MS),
            Arc::new(CountingIds::from(ids)),
        )
    };

    assert!(matches!(
        login(rekey(0x30, MACHINE), 0xa000).await.unwrap(),
        DeviceLoginDecision::Approved { .. }
    ));
    let before = fixture.rows().await?;
    assert_eq!(
        login(rekey(0x31, 0x11), 0xa100).await.unwrap(),
        DeviceLoginDecision::Refused {
            reason: DeviceLoginRefusal::FormerServiceLinkedElsewhere {},
        }
    );
    assert_eq!(fixture.rows().await?, before);
    Ok(())
}
