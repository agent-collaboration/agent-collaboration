//! Admission, revocation with its ABA, and a converted machine's former id,
//! each checked against the store's own rows.
use super::super::machine_connection::{
    RevocationBy, admit_machine_connection, revoke_machine_enrollment,
};
use super::case_values::{
    CODE_LIFETIME_S, FORMER, LABEL, MACHINE, NOW_MS, NOW_S, REQUEST, REVOKER, WORKSPACE_A,
    add_workspace, approval, approve, approved, at, b3_store, grant, ids, installation,
    organization, settings, typed, uuid7,
};
use super::decision_cases::first_approval_rows;
use super::expected_rows::{
    Enrollment, Origin, allowance, before, expect_changes, expect_unchanged, record,
};
use super::sql_case_host::{CaseHost, LegacyMachine};
use super::{CaseFailure, CaseResult, check, check_eq};
use crate::identity_contract::{
    AdmitMachineConnection, ClientId, DecideDeviceLogin, DeviceLoginDecision, DeviceLoginRefusal,
    FormerServiceId, MachineConnectionAdmission, MachineConnectionRefusal, MachineTokenDecision,
    MachineTokenRefusal, PresentedMachine, RevokeMachineEnrollment, RevokeMachineEnrollmentResult,
};
use crate::person_request::AdminOperationId;
use fleet_identity_protocol::{EnrollmentId, OrganizationId, UnixSeconds};
use fleet_sync_protocol::PeerId;

const OPERATION: u64 = 0x50;

/// A connection from `peer` with the enrollment's own client.
fn connection(id: &EnrollmentId, peer: u64) -> Result<AdmitMachineConnection, CaseFailure> {
    Ok(AdmitMachineConnection {
        organization_id: organization()?,
        enrollment_id: id.clone(),
        installation: installation(MACHINE)?,
        presented: PresentedMachine {
            installation: installation(MACHINE)?,
            peer: PeerId::try_from(uuid7(peer)).map_err(|error| CaseFailure(error.to_string()))?,
        },
        token_expires_at: UnixSeconds(NOW_S + 900),
        client_id: typed::<ClientId>(id.as_str().to_owned())?,
    })
}

async fn admit<Host: CaseHost>(
    host: &Host,
    request: AdmitMachineConnection,
    now_ms: i64,
) -> CaseResult<MachineConnectionAdmission> {
    Ok(
        admit_machine_connection(host.port(), request, at(now_ms), ids(0xd000), settings()?)
            .await?,
    )
}

/// Admission reads the store live and writes nothing, admitted or refused.
pub(super) async fn admission_reads_live<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[WORKSPACE_A]).await?;
    let id = approved(
        host,
        approval(REQUEST, MACHINE, &[WORKSPACE_A])?,
        NOW_MS,
        0xa000,
    )
    .await?;
    grant(host, &id, NOW_MS + 500, 0xa080).await?;
    let rows = before(host).await?;

    let admitted = admit(host, connection(&id, MACHINE)?, NOW_MS + 2000).await?;
    let wrong_peer = admit(host, connection(&id, 0x11)?, NOW_MS + 2000).await?;
    let other_organization = admit(
        host,
        AdmitMachineConnection {
            organization_id: typed::<OrganizationId>(uuid7(0x09))?,
            ..connection(&id, MACHINE)?
        },
        NOW_MS + 2000,
    )
    .await?;
    let wrong_client = admit(
        host,
        AdmitMachineConnection {
            client_id: typed::<ClientId>(uuid7(0x99))?,
            ..connection(&id, MACHINE)?
        },
        NOW_MS + 2000,
    )
    .await?;

    check!(
        matches!(admitted, MachineConnectionAdmission::Admitted { .. }),
        "the machine's own connection answered {admitted:?}"
    );
    let refused = |reason| MachineConnectionAdmission::Refused { reason };
    check_eq!(
        (wrong_peer, other_organization, wrong_client),
        (
            refused(MachineConnectionRefusal::UnknownMachine {}),
            refused(MachineConnectionRefusal::MachineInAnotherOrganization {}),
            refused(MachineConnectionRefusal::WrongClient {})
        ),
        "the refused connections"
    );
    expect_unchanged(host, &rows, "admission").await
}

/// Revocation ends the active enrollment, bumps the generation, withdraws the
/// allowance with its records, and ends every earlier approval (ABA): no
/// Grant, no connection, nothing written by either refusal.
pub(super) async fn revocation_ends_machine_and_earlier_approvals<Host: CaseHost>(
    host: &Host,
) -> CaseResult {
    b3_store(host, &[WORKSPACE_A]).await?;
    let (installation_row, first, _) = first_approval_rows(
        0xa000,
        MACHINE,
        REQUEST,
        &[WORKSPACE_A],
        NOW_MS,
        NOW_S + CODE_LIFETIME_S,
    );
    let id = approved(
        host,
        approval(REQUEST, MACHINE, &[WORKSPACE_A])?,
        NOW_MS,
        0xa000,
    )
    .await?;
    grant(host, &id, NOW_MS + 500, 0xa080).await?;
    // An approval made while that enrollment is active.
    let pending = approved(
        host,
        approval(REQUEST + 1, MACHINE, &[WORKSPACE_A])?,
        NOW_MS + 1000,
        0xa100,
    )
    .await?;
    let rows = before(host).await?;

    let revocation = revoke_machine_enrollment(
        host.port(),
        &RevokeMachineEnrollment {
            organization_id: organization()?,
            installation: installation(MACHINE)?,
        },
        RevocationBy {
            person: typed(uuid7(REVOKER))?,
            operation: AdminOperationId::try_from(uuid7(OPERATION))
                .map_err(|error| CaseFailure(error.to_string()))?,
        },
        at(NOW_MS + 2000),
        ids(0xc000),
    )
    .await?;

    check!(
        matches!(
            revocation.answer,
            RevokeMachineEnrollmentResult::Applied { .. }
        ),
        "the revocation answered {:?}",
        revocation.answer
    );
    let rows = expect_changes(
        host,
        &rows,
        vec![
            first
                .granted(NOW_MS + 500)
                .ended("revoked", Some("owner"), NOW_MS + 2000)
                .entry(),
            installation_row.at(2, NOW_MS + 2000).entry(),
            record(
                0xc000,
                OPERATION,
                "machineRevoked",
                Origin::Revoker,
                0xa000,
                NOW_MS + 2000,
            ),
            allowance(
                0xa001,
                MACHINE,
                WORKSPACE_A,
                "withdrawn",
                NOW_MS,
                NOW_MS + 2000,
            ),
            record(
                0xc001,
                OPERATION,
                "allowanceChanged",
                Origin::Revoker,
                0xa001,
                NOW_MS + 2000,
            ),
        ],
        "the revocation",
    )
    .await?;

    let late = grant(host, &pending, NOW_MS + 3000, 0xb900).await?;
    let after = admit(host, connection(&id, MACHINE)?, NOW_MS + 3000).await?;

    check_eq!(
        (late, after),
        (
            MachineTokenDecision::Refuse {
                reason: MachineTokenRefusal::Superseded {},
            },
            MachineConnectionAdmission::Refused {
                reason: MachineConnectionRefusal::EnrollmentNotActive {},
            }
        ),
        "the earlier approval's Grant and the revoked connection"
    );
    expect_unchanged(host, &rows, "after the revocation").await
}

/// After the switch each machine is keyed by its peer id with its former id
/// recorded: a login carrying it writes only its enrollment and allowance;
/// another installation can't take it, and writes nothing.
pub(super) async fn converted_former_id_links_own_machine_only<Host: CaseHost>(
    host: &Host,
) -> CaseResult {
    host.initialize_pre_b3(vec![LegacyMachine {
        peer: uuid7(MACHINE),
        service: FORMER.to_owned(),
        label: LABEL.to_owned(),
        last_seen: None,
        created_at: "2026-11-08T12:00:00Z".to_owned(),
    }])
    .await?;
    host.initialize_b3().await?;
    add_workspace(host, WORKSPACE_A).await?;
    let rekey = |request: u64, machine: u64| -> CaseResult<DecideDeviceLogin> {
        Ok(DecideDeviceLogin {
            former_service_id: Some(typed::<FormerServiceId>(FORMER.to_owned())?),
            ..approval(request, machine, &[WORKSPACE_A])?
        })
    };
    let rows = before(host).await?;

    approved(host, rekey(REQUEST, MACHINE)?, NOW_MS, 0xa000).await?;
    let rows = expect_changes(
        host,
        &rows,
        vec![
            Enrollment::approved(0xa000, MACHINE, REQUEST, 0, NOW_MS, NOW_S + CODE_LIFETIME_S)
                .entry(),
            allowance(0xa001, MACHINE, WORKSPACE_A, "active", NOW_MS, NOW_MS),
            record(
                0xa002,
                REQUEST,
                "allowanceChanged",
                Origin::Approver,
                0xa001,
                NOW_MS,
            ),
        ],
        "the converted machine's login",
    )
    .await?;

    let elsewhere = approve(host, rekey(REQUEST + 1, 0x11)?, NOW_MS, 0xa100).await;

    check_eq!(
        elsewhere,
        Ok(DeviceLoginDecision::Refused {
            reason: DeviceLoginRefusal::FormerServiceLinkedElsewhere {},
        }),
        "another installation's login"
    );
    expect_unchanged(host, &rows, "another installation's login").await
}
