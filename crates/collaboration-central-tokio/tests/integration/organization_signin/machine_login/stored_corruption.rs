//! Persisted corruption is setup only; real private decisions must fail closed.
//! Carries historical sql_cases::constraint_cases::invalid_rows_refused.
use super::support::*;

async fn owning_snapshot(f: &MachineFixture) -> Result<Vec<Vec<Vec<central_storage::SqlValue>>>> {
    let mut rows = snapshot(f).await?;
    for sql in [
        "SELECT * FROM access_actor_identities ORDER BY actor_identity_id",
        "SELECT * FROM directory_workspaces ORDER BY workspace_id",
        "SELECT * FROM runtime_request_log ORDER BY request_id",
        "SELECT * FROM signin_organization_identity ORDER BY id",
        "SELECT * FROM signin_authorization_floors ORDER BY id",
        "SELECT * FROM signin_person_authorization_floors ORDER BY id",
        "SELECT * FROM admin_used_requests ORDER BY id",
        "SELECT * FROM mail_workspace_drains ORDER BY workspace_id",
        "SELECT * FROM mail_messages ORDER BY message_id",
        "SELECT * FROM mail_notification_windows ORDER BY window_id",
        "SELECT * FROM mail_notification_window_items ORDER BY window_item_id",
        "SELECT * FROM automation_wakes ORDER BY wake_id",
        "SELECT * FROM automation_schedules ORDER BY automation_id",
    ] {
        rows.push(f.rows(sql).await?);
    }
    Ok(rows)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Persisted invalid domains must produce an empty real-route 503 with opaque whole-store no-effect checks and a successful repaired control."
)]
#[tokio::test]
async fn persisted_invalid_lifecycle_identifiers_times_and_generation_refuse_empty_503_without_effects()
-> Result<()> {
    // Each probe owns a fresh physical store and otherwise valid real approval/Grant.
    // Direct SQL below corrupts storage deliberately; it is never a route-effect claim.
    for (domain, corrupt_sql) in [
        (
            "unknown lifecycle",
            "UPDATE login_machine_approvals SET state='paused'",
        ),
        (
            "enrolled without client",
            "UPDATE login_machine_approvals SET machine_client_id=NULL",
        ),
        (
            "approved with an end",
            "UPDATE login_machine_approvals SET state='approved',machine_client_id=NULL,last_grant_at=NULL,ended_at=created_at",
        ),
        (
            "unknown ending reason",
            "UPDATE login_machine_approvals SET state='ended',end_reason='bored',last_grant_at=NULL,ended_at=created_at",
        ),
        (
            "fractional last-grant seconds",
            "UPDATE login_machine_approvals SET last_grant_at='2030-03-17T17:46:40.001Z'",
        ),
        (
            "malformed request ID",
            "UPDATE login_machine_approvals SET request_id='not-a-uuid'",
        ),
        // This literal is intentionally an invalid stored version, not a generated ID.
        (
            "non-v7 request ID",
            "UPDATE login_machine_approvals SET request_id='019f0000-0000-4000-8000-00000000d401'",
        ),
        (
            "negative machine generation",
            "UPDATE login_machines SET enrollment_generation=-1",
        ),
    ] {
        let f = MachineFixture::new().await?;
        let enrollment_id = enrollment(
            &f.post(
                DEVICE,
                &approval(
                    0xd401,
                    INSTALLATION,
                    "Corruption control",
                    &[WORKSPACE],
                    f.now(),
                ),
            )
            .await?
            .1,
        )?;
        let request = token(&enrollment_id, &enrollment_id);
        assert_eq!(
            f.post(TOKEN, &request).await?,
            (200, grant(&enrollment_id, INSTALLATION, f.now(), None)),
            "{domain}: valid first Grant control"
        );
        let original = f
            .rows("SELECT request_id,machine_client_id,last_grant_at FROM login_machine_approvals")
            .await?;
        assert_eq!(original.len(), 1);
        let healthy = owning_snapshot(&f).await?;
        f.execute(corrupt_sql, vec![]).await?;
        let corrupted = owning_snapshot(&f).await?;
        assert!(
            corrupted != healthy,
            "{domain}: corruption setup changed no stored fact"
        );

        let reply = f.inner.post(TOKEN, &request).await?;
        assert_eq!(
            reply.status, 503,
            "{domain}: corrupt stored facts were accepted"
        );
        assert!(
            reply.body.is_empty(),
            "{domain}: storage refusal must have an empty body"
        );
        assert!(
            owning_snapshot(&f).await? == corrupted,
            "{domain}: failed decision repaired/coerced corrupt facts or wrote another owning row"
        );

        // Restore the original stored scalar values exactly. This is repair setup,
        // not a product recovery path; the same real decision must work afterwards.
        f.execute(
            "UPDATE login_machine_approvals SET state='enrolled',end_reason=NULL,ended_at=NULL,request_id=?,machine_client_id=?,last_grant_at=?",
            original[0].clone(),
        ).await?;
        f.execute("UPDATE login_machines SET enrollment_generation=1", vec![])
            .await?;
        assert!(
            owning_snapshot(&f).await? == healthy,
            "{domain}: repair did not restore the exact valid store"
        );
        assert_eq!(
            f.post(TOKEN, &request).await?,
            (200, grant(&enrollment_id, INSTALLATION, f.now(), None)),
            "{domain}: repaired active Grant control"
        );
        f.close().await?;
    }
    Ok(())
}
