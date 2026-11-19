//! Observation must reject corrupt machine rows without changing their owner.
use super::support::*;
use central_storage::SqlValue;

#[expect(
    clippy::panic_in_result_fn,
    reason = "The real observer must reject corrupt storage and leave every owning row unchanged."
)]
#[tokio::test]
async fn machine_observation_rejects_corrupt_rows_without_partial_facts_or_writes() -> Result<()> {
    let f = MachineFixture::new().await?;
    let serial = 0xe301;
    let approval_reply = f
        .post(
            DEVICE,
            &approval(
                serial,
                INSTALLATION,
                "Observed machine",
                &[WORKSPACE],
                f.now(),
            ),
        )
        .await?;
    assert_eq!(approval_reply.0, 200);
    let enrollment_id = enrollment(&approval_reply.1);
    assert_eq!(
        f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
            .await?,
        (200, grant(&enrollment_id, INSTALLATION, f.now(), None))
    );
    let machine = f
        .rows("SELECT machine_id,enrollment_generation FROM login_machines")
        .await?;
    assert_eq!(machine.len(), 1);
    let (SqlValue::Text(machine_id), SqlValue::Integer(generation)) =
        (&machine[0][0], &machine[0][1])
    else {
        panic!("machine row")
    };
    let original_grant = f
        .rows("SELECT last_grant_at FROM login_machine_approvals")
        .await?;
    let SqlValue::Text(last_grant) = &original_grant[0][0] else {
        panic!("grant timestamp")
    };
    let runtime = f.inner.runtime().await;
    let healthy: Value = runtime.control("raw-signin-observe", &()).await?;
    assert_eq!(healthy["complete"], true);
    let machines = healthy["machines"].as_array().ok_or("machines missing")?;
    let approvals = healthy["machineApprovals"]
        .as_array()
        .ok_or("machine approvals missing")?;
    assert_eq!((machines.len(), approvals.len()), (1, 1));
    assert_eq!(machines[0]["machineId"], *machine_id);
    assert_eq!(machines[0]["machineId"], INSTALLATION);
    assert_eq!(machines[0]["state"], "enrolled");
    assert_eq!(machines[0]["enrollmentGeneration"], generation.get());
    assert_eq!(machines[0]["allowedWorkspaces"], json!([WORKSPACE]));
    assert_eq!(approvals[0]["approvalId"], enrollment_id);
    assert_eq!(approvals[0]["requestId"], id(serial));
    assert_eq!(approvals[0]["machineId"], *machine_id);
    assert_eq!(approvals[0]["machineClientId"], enrollment_id);
    assert_eq!(approvals[0]["state"], "enrolled");
    assert_eq!(approvals[0]["endReason"], Value::Null);
    assert_eq!(approvals[0]["endedAt"], Value::Null);
    assert_eq!(approvals[0]["allowedWorkspaces"], json!([WORKSPACE]));
    assert_eq!(approvals[0]["baseGeneration"], 0);
    let timestamp = |seconds| {
        chrono::DateTime::from_timestamp(seconds, 0)
            .unwrap()
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    assert_eq!(approvals[0]["createdAt"], timestamp(f.now()));
    assert_eq!(approvals[0]["lastGrantAt"], timestamp(f.now()));
    assert_eq!(approvals[0]["codeExpiresAt"], timestamp(f.now() + LIFETIME));
    let baseline = snapshot(&f).await?;
    for (corrupt, repair, original, field) in [
        (
            "UPDATE login_machines SET enrollment_generation=-1 WHERE machine_id=?",
            "UPDATE login_machines SET enrollment_generation=? WHERE machine_id=?",
            SqlValue::Integer(*generation),
            "login_machines.enrollment_generation",
        ),
        (
            "UPDATE login_machine_approvals SET last_grant_at='not-a-time' WHERE machine_id=?",
            "UPDATE login_machine_approvals SET last_grant_at=? WHERE machine_id=?",
            text(last_grant),
            "login_machine_approvals.last_grant_at",
        ),
    ] {
        f.execute(corrupt, vec![text(machine_id)]).await?;
        let corrupted = snapshot(&f).await?;
        let observed: std::result::Result<Value, central_storage::StoreFailure> =
            runtime.control("raw-signin-observe", &()).await;
        assert!(observed.is_err(), "observer accepted corrupt {field}");
        assert!(
            snapshot(&f).await? == corrupted,
            "observation modified machine owner rows"
        );
        f.execute(repair, vec![original, text(machine_id)]).await?;
        assert!(
            snapshot(&f).await? == baseline,
            "repair changed more than the test field"
        );
        let repaired: Value = runtime.control("raw-signin-observe", &()).await?;
        assert!(
            repaired == healthy,
            "repaired observation differs from healthy control"
        );
    }
    f.close().await
}
