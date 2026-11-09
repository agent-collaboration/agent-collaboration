//! Approval, first Grant, BA1, the N10 lapse, the pending repeat, refusals,
//! persistence and rollback, each checked against the store's own rows.
use super::super::enrollment_port::{
    Decided, EnrollmentQuery, MachineEnrollmentFailure, MachineEnrollmentPort,
};
use super::super::enrollment_records::{
    AllowanceRow, EnrollmentChange, EnrollmentGeneration, StoredEnrollmentState,
};
use super::case_values::{
    CODE_LIFETIME_S, MACHINE, NOW_MS, NOW_S, REQUEST, WORKSPACE_A, WORKSPACE_B, approval, approve,
    approved, b3_store, facts, grant, installation, typed, uuid7,
};
use super::expected_rows::{
    Enrollment, Entry, Installation, Origin, allowance, before, expect_changes, expect_rows,
    expect_unchanged, record,
};
use super::sql_case_host::CaseHost;
use super::{CaseFailure, CasePhase, CaseResult, check, check_eq};
use crate::identity_contract::{
    AllowanceState, DecideDeviceLogin, DeviceLoginDecision, DeviceLoginRefusal,
    MachineTokenDecision, MachineTokenRefusal,
};
use fleet_sync_protocol::PeerId;

/// The rows a machine's first approval writes from ID block `block`: its
/// installation, the approved enrollment, then per workspace an allowance and
/// its record.
pub(super) fn first_approval_rows(
    block: u64,
    machine: u64,
    request: u64,
    workspaces: &[u64],
    at_ms: i64,
    code_expires_at_s: i64,
) -> (Installation, Enrollment, Vec<Entry>) {
    let installation = Installation::approved(machine, at_ms);
    let enrollment = Enrollment::approved(block, machine, request, 0, at_ms, code_expires_at_s);
    let mut rows = vec![installation.entry(), enrollment.entry()];
    let mut next = block;
    for workspace in workspaces {
        let allowance_id = next + 1;
        rows.push(allowance(
            allowance_id,
            machine,
            *workspace,
            "active",
            at_ms,
            at_ms,
        ));
        rows.push(record(
            allowance_id + 1,
            request,
            "allowanceChanged",
            Origin::Approver,
            allowance_id,
            at_ms,
        ));
        next += 2;
    }
    (installation, enrollment, rows)
}

pub(super) async fn staged_corpus_initializes_once<Host: CaseHost>(host: &Host) -> CaseResult {
    host.initialize_pre_b3(Vec::new()).await?;
    check_eq!(host.initialize_b3().await?, 2, "the switch");
    check_eq!(host.initialize_b3().await?, 0, "a second switch");
    expect_rows(host, Vec::new(), "after the switch").await
}

/// The machine after one approval of both workspaces and its first Grant.
fn approved_and_granted() -> (Vec<Entry>, Vec<Entry>) {
    let (installation, enrollment, approval_rows) = first_approval_rows(
        0xa000,
        MACHINE,
        REQUEST,
        &[WORKSPACE_A, WORKSPACE_B],
        NOW_MS,
        NOW_S + CODE_LIFETIME_S,
    );
    let grant_rows = vec![
        enrollment.granted(NOW_MS + 1000).entry(),
        installation.at(1, NOW_MS + 1000).entry(),
        record(
            0xb000,
            REQUEST,
            "machineEnrolled",
            Origin::Approver,
            0xa000,
            NOW_MS + 1000,
        ),
    ];
    (approval_rows, grant_rows)
}

pub(super) async fn approval_and_first_grant<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[WORKSPACE_A, WORKSPACE_B]).await?;
    let (approval_rows, grant_rows) = approved_and_granted();
    let empty = before(host).await?;

    let id = approved(
        host,
        approval(REQUEST, MACHINE, &[WORKSPACE_A, WORKSPACE_B])?,
        NOW_MS,
        0xa000,
    )
    .await?;
    let approved_rows = expect_changes(host, &empty, approval_rows, "the approval").await?;

    let answer = grant(host, &id, NOW_MS + 1000, 0xb000).await?;
    check!(
        matches!(
            answer,
            MachineTokenDecision::Grant {
                superseded: None,
                ..
            }
        ),
        "the first Grant answered {answer:?}"
    );
    expect_changes(host, &approved_rows, grant_rows, "the first Grant").await?;
    // The port reads the activation back: the generation and active pointer.
    let read = facts(
        host,
        EnrollmentQuery {
            installation: Some(installation(MACHINE)?),
            ..EnrollmentQuery::default()
        },
    )
    .await?;
    check_eq!(
        read.installation.map(|row| (row.generation, row.active)),
        Some((EnrollmentGeneration(1), Some(id))),
        "the installation after the first Grant"
    );
    Ok(())
}

/// The approved SQL keeps no request body, so a repeated request ID is refused
/// as pending the owner, with nothing written. A placeholder, not N03.
pub(super) async fn repeat_pending_owner<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[WORKSPACE_A]).await?;
    approved(
        host,
        approval(REQUEST, MACHINE, &[WORKSPACE_A])?,
        NOW_MS,
        0xa000,
    )
    .await?;
    let rows = before(host).await?;

    let repeat = approve(
        host,
        approval(REQUEST, MACHINE, &[WORKSPACE_A])?,
        NOW_MS,
        0xa100,
    )
    .await;

    check_eq!(
        repeat,
        Err(MachineEnrollmentFailure::RepeatPendingOwner),
        "the repeat"
    );
    expect_unchanged(host, &rows, "the repeat").await
}

pub(super) async fn refusals_write_nothing<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[WORKSPACE_A]).await?;
    let rows = before(host).await?;

    let unknown = approve(
        host,
        approval(REQUEST, MACHINE, &[WORKSPACE_B])?,
        NOW_MS,
        0xa000,
    )
    .await;
    let wrong_peer = approve(
        host,
        DecideDeviceLogin {
            peer: PeerId::try_from(uuid7(0x11)).map_err(|error| CaseFailure(error.to_string()))?,
            ..approval(REQUEST + 1, MACHINE, &[WORKSPACE_A])?
        },
        NOW_MS,
        0xa000,
    )
    .await;

    check_eq!(
        (unknown, wrong_peer),
        (
            Ok(DeviceLoginDecision::Refused {
                reason: DeviceLoginRefusal::UnknownWorkspace {},
            }),
            Ok(DeviceLoginDecision::Refused {
                reason: DeviceLoginRefusal::NotPermitted {},
            })
        ),
        "the refusals"
    );
    expect_unchanged(host, &rows, "the refusals").await
}

/// BA1: of two approvals at one generation, the first Grant wins; the other is
/// refused superseded and writes nothing.
pub(super) async fn first_grant_wins<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[WORKSPACE_A]).await?;
    let (installation, _, _) = first_approval_rows(
        0xa000,
        MACHINE,
        REQUEST,
        &[WORKSPACE_A],
        NOW_MS,
        NOW_S + CODE_LIFETIME_S,
    );
    let a = approved(
        host,
        approval(REQUEST, MACHINE, &[WORKSPACE_A])?,
        NOW_MS,
        0xa000,
    )
    .await?;
    let rows = before(host).await?;
    let b = approved(
        host,
        approval(REQUEST + 1, MACHINE, &[WORKSPACE_A])?,
        NOW_MS + 100,
        0xa100,
    )
    .await?;
    // Same label, allowance already active: only the enrollment is new.
    let second = Enrollment::approved(
        0xa100,
        MACHINE,
        REQUEST + 1,
        0,
        NOW_MS + 100,
        NOW_S + CODE_LIFETIME_S,
    );
    let rows = expect_changes(host, &rows, vec![second.entry()], "the second approval").await?;

    let answer = grant(host, &b, NOW_MS + 1000, 0xb000).await?;
    check!(
        matches!(answer, MachineTokenDecision::Grant { .. }),
        "b's first Grant answered {answer:?}"
    );
    let rows = expect_changes(
        host,
        &rows,
        vec![
            second.granted(NOW_MS + 1000).entry(),
            installation.at(1, NOW_MS + 1000).entry(),
            record(
                0xb000,
                REQUEST + 1,
                "machineEnrolled",
                Origin::Approver,
                0xa100,
                NOW_MS + 1000,
            ),
        ],
        "b's first Grant",
    )
    .await?;

    let late = grant(host, &a, NOW_MS + 2000, 0xb100).await?;

    check_eq!(
        late,
        MachineTokenDecision::Refuse {
            reason: MachineTokenRefusal::Superseded {},
        },
        "a's late Grant"
    );
    expect_unchanged(host, &rows, "a's late Grant").await
}

/// N10: approved at minute 9, the code expires at minute 10. A Grant 1 ms
/// before the deadline activates; at it and after, the approval lapses and
/// only its own row changes. One machine per instant.
pub(super) async fn code_deadline_lapses<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[WORKSPACE_A]).await?;
    for (offset, at_ms, lapses) in [
        (0, NOW_MS - 1, false),
        (1, NOW_MS, true),
        (2, NOW_MS + 1, true),
    ] {
        let machine = MACHINE + offset;
        let block = 0xa000 + offset * 0x100;
        let approved_at = NOW_MS - 60_000;
        let request = DecideDeviceLogin {
            code_expires_at: fleet_identity_protocol::UnixSeconds(NOW_S),
            // A second factor taken at approval: only the first machine is exempt.
            approver_second_factor_at: Some(fleet_identity_protocol::UnixSeconds(
                approved_at.div_euclid(1000),
            )),
            ..approval(REQUEST + offset, machine, &[WORKSPACE_A])?
        };
        let rows = before(host).await?;
        let id = approved(host, request, approved_at, block).await?;
        let (installation, enrollment, approval_rows) = first_approval_rows(
            block,
            machine,
            REQUEST + offset,
            &[WORKSPACE_A],
            approved_at,
            NOW_S,
        );
        let rows = expect_changes(host, &rows, approval_rows, "the approval").await?;

        let answer = grant(host, &id, at_ms, 0xb000 + offset * 0x100).await?;

        if lapses {
            check_eq!(
                answer,
                MachineTokenDecision::Refuse {
                    reason: MachineTokenRefusal::EnrollmentNotActive {},
                },
                "a Grant at {at_ms}"
            );
            expect_changes(
                host,
                &rows,
                vec![enrollment.ended("lapsed", None, at_ms).entry()],
                &format!("a Grant at {at_ms}"),
            )
            .await?;
        } else {
            check!(
                matches!(answer, MachineTokenDecision::Grant { .. }),
                "a Grant at {at_ms} answered {answer:?}"
            );
            expect_changes(
                host,
                &rows,
                vec![
                    enrollment.granted(at_ms).entry(),
                    installation.at(1, at_ms).entry(),
                    record(
                        0xb000 + offset * 0x100,
                        REQUEST + offset,
                        "machineEnrolled",
                        Origin::Approver,
                        block,
                        at_ms,
                    ),
                ],
                &format!("a Grant at {at_ms}"),
            )
            .await?;
        }
    }
    Ok(())
}

/// Written in one request, read back in a later one on the same persistence:
/// a reopened owning store on Tokio, the same named object on workerd.
pub(super) async fn persistence<Host: CaseHost>(host: &Host, phase: CasePhase) -> CaseResult {
    let (mut rows, grant_rows) = approved_and_granted();
    // The first Grant rewrites the enrollment and installation.
    rows.retain(|(key, _)| !grant_rows.iter().any(|(granted, _)| granted == key));
    rows.extend(grant_rows);
    if phase == CasePhase::Write {
        b3_store(host, &[WORKSPACE_A, WORKSPACE_B]).await?;
        let id = approved(
            host,
            approval(REQUEST, MACHINE, &[WORKSPACE_A, WORKSPACE_B])?,
            NOW_MS,
            0xa000,
        )
        .await?;
        grant(host, &id, NOW_MS + 1000, 0xb000).await?;
    }
    expect_rows(host, rows, "the persisted machine").await?;
    let read = facts(
        host,
        EnrollmentQuery {
            enrollment: Some(typed(uuid7(0xa000))?),
            installation: Some(installation(MACHINE)?),
            ..EnrollmentQuery::default()
        },
    )
    .await?;
    check!(
        matches!(
            read.enrollment.map(|enrollment| enrollment.state),
            Some(StoredEnrollmentState::Enrolled { .. })
        ),
        "the persisted enrollment is not enrolled"
    );
    check_eq!(
        read.installation
            .map(|row| (row.generation, row.active.map(|id| id.as_str().to_owned()))),
        Some((EnrollmentGeneration(1), Some(uuid7(0xa000)))),
        "the persisted installation"
    );
    Ok(())
}

/// An engine refusal after real writes rolls the whole decision back: the
/// allowance written first is gone too.
pub(super) async fn failure_after_writes_rolls_back<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[WORKSPACE_A]).await?;
    let id = approved(
        host,
        approval(REQUEST, MACHINE, &[WORKSPACE_A])?,
        NOW_MS,
        0xa000,
    )
    .await?;
    let rows = before(host).await?;
    let stored = facts(
        host,
        EnrollmentQuery {
            enrollment: Some(id),
            ..EnrollmentQuery::default()
        },
    )
    .await?
    .enrollment
    .ok_or_else(|| CaseFailure("the approved enrollment reads back absent".to_owned()))?;
    let duplicate_id = typed(uuid7(0x7777))?;
    let allowance = AllowanceRow {
        id: durable_steps::Uuid::parse_str(&uuid7(0x7001))
            .map_err(|error| CaseFailure(error.to_string()))?,
        installation: installation(MACHINE)?,
        workspace: typed(uuid7(WORKSPACE_B))?,
        state: AllowanceState::Active,
    };

    let failed = host
        .port()
        .transact(EnrollmentQuery::default(), move |_| {
            let mut duplicate = stored;
            duplicate.id = duplicate_id;
            Decided::writing(
                (),
                vec![
                    EnrollmentChange::PutAllowance(allowance),
                    // The same request ID again: the engine's UNIQUE refuses it.
                    EnrollmentChange::InsertEnrollment(duplicate),
                ],
                NOW_MS + 1,
            )
        })
        .await;

    check_eq!(
        failed,
        Err(MachineEnrollmentFailure::Unavailable),
        "the failing write"
    );
    expect_unchanged(host, &rows, "the failing write").await
}
