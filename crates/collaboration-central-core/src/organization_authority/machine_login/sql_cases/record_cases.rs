//! One machine's records across two allowances, a superseding first Grant, a
//! repeat Grant and an automatic idle expiry: each record with its own key and
//! full target, each step writing only its own rows.
use super::case_values::{
    CODE_LIFETIME_S, DAY_MS, MACHINE, NOW_MS, NOW_S, REQUEST, WORKSPACE_A, WORKSPACE_B, approval,
    approved, b3_store, grant,
};
use super::decision_cases::first_approval_rows;
use super::expected_rows::{
    Enrollment, Origin, allowance, before, expect_changes, expect_unchanged, record,
};
use super::sql_case_host::CaseHost;
use super::{CaseResult, check, check_eq};
use crate::identity_contract::{MachineTokenDecision, MachineTokenRefusal};

pub(super) async fn records_keep_their_own_keys<Host: CaseHost>(host: &Host) -> CaseResult {
    b3_store(host, &[WORKSPACE_A, WORKSPACE_B]).await?;
    let (installation, first, approval_rows) = first_approval_rows(
        0xa000,
        MACHINE,
        REQUEST,
        &[WORKSPACE_A, WORKSPACE_B],
        NOW_MS,
        NOW_S + CODE_LIFETIME_S,
    );
    let rows = before(host).await?;
    let first_id = approved(
        host,
        approval(REQUEST, MACHINE, &[WORKSPACE_A, WORKSPACE_B])?,
        NOW_MS,
        0xa000,
    )
    .await?;
    let rows = expect_changes(host, &rows, approval_rows, "the first approval").await?;
    grant(host, &first_id, NOW_MS + 1000, 0xb000).await?;
    let first = first.granted(NOW_MS + 1000);
    let rows = expect_changes(
        host,
        &rows,
        vec![
            first.entry(),
            installation.at(1, NOW_MS + 1000).entry(),
            record(
                0xb000,
                REQUEST,
                "machineEnrolled",
                Origin::Approver,
                0xa000,
                NOW_MS + 1000,
            ),
        ],
        "the first Grant",
    )
    .await?;

    // A second approval for workspace A, already allowed: only its enrollment.
    let second_id = approved(
        host,
        approval(REQUEST + 1, MACHINE, &[WORKSPACE_A])?,
        NOW_MS + 2000,
        0xa100,
    )
    .await?;
    let second = Enrollment::approved(
        0xa100,
        MACHINE,
        REQUEST + 1,
        1,
        NOW_MS + 2000,
        NOW_S + CODE_LIFETIME_S,
    );
    let rows = expect_changes(host, &rows, vec![second.entry()], "the second approval").await?;

    // Its first Grant supersedes the first enrollment.
    let answer = grant(host, &second_id, NOW_MS + 3000, 0xb100).await?;
    check!(
        matches!(
            answer,
            MachineTokenDecision::Grant {
                superseded: Some(_),
                ..
            }
        ),
        "the superseding Grant answered {answer:?}"
    );
    let second = second.granted(NOW_MS + 3000);
    let rows = expect_changes(
        host,
        &rows,
        vec![
            first
                .ended("revoked", Some("superseded"), NOW_MS + 3000)
                .entry(),
            record(
                0xb100,
                REQUEST + 1,
                "machineRevoked",
                Origin::Approver,
                0xa000,
                NOW_MS + 3000,
            ),
            second.entry(),
            installation.at(2, NOW_MS + 3000).entry(),
            record(
                0xb101,
                REQUEST + 1,
                "machineEnrolled",
                Origin::Approver,
                0xa100,
                NOW_MS + 3000,
            ),
        ],
        "the superseding Grant",
    )
    .await?;

    // A repeat Grant moves the last Grant and writes no record.
    grant(host, &second_id, NOW_MS + 4000, 0xb200).await?;
    let second = second.granted(NOW_MS + 4000);
    let rows = expect_changes(host, &rows, vec![second.entry()], "the repeat Grant").await?;

    // Thirty days after the last Grant: an automatic expiry (N07), recorded,
    // and each active allowance withdrawn with its own record.
    let idle_ms = NOW_MS + 4000 + 30 * DAY_MS;
    let expired = grant(host, &second_id, idle_ms, 0xb300).await?;
    check_eq!(
        expired,
        MachineTokenDecision::Refuse {
            reason: MachineTokenRefusal::IdleExpired {},
        },
        "the Grant after thirty idle days"
    );
    let rows = expect_changes(
        host,
        &rows,
        vec![
            second.ended("expired", None, idle_ms).entry(),
            record(
                0xb300,
                0xa100,
                "machineExpired",
                Origin::Automatic,
                0xa100,
                idle_ms,
            ),
            installation.at(2, idle_ms).entry(),
            allowance(0xa001, MACHINE, WORKSPACE_A, "withdrawn", NOW_MS, idle_ms),
            record(
                0xb301,
                0xa100,
                "allowanceChanged",
                Origin::Automatic,
                0xa001,
                idle_ms,
            ),
            allowance(0xa003, MACHINE, WORKSPACE_B, "withdrawn", NOW_MS, idle_ms),
            record(
                0xb302,
                0xa100,
                "allowanceChanged",
                Origin::Automatic,
                0xa003,
                idle_ms,
            ),
        ],
        "the idle expiry",
    )
    .await?;

    // A later look at the expired enrollment writes nothing more.
    let again = grant(host, &second_id, idle_ms + 1000, 0xb400).await?;
    check_eq!(
        again,
        MachineTokenDecision::Refuse {
            reason: MachineTokenRefusal::EnrollmentNotActive {},
        },
        "a Grant after the expiry"
    );
    expect_unchanged(host, &rows, "a Grant after the expiry").await?;

    // The store's own reader sees every record, each under a distinct key.
    let records = host.records().await?;
    let mut keys = records
        .iter()
        .map(|record| record.request_id.as_str().to_owned())
        .collect::<Vec<_>>();
    keys.sort();
    keys.dedup();
    check_eq!((keys.len(), records.len()), (8, 8), "distinct record keys");
    Ok(())
}
