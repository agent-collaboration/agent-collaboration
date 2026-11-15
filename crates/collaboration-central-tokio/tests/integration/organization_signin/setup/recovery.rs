use super::support::*;
use serde_json::json;
use std::time::Duration;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Lost-answer recovery must expose the first committed bytes through the owning reopened store."
)]
#[tokio::test]
async fn lost_create_answer_recovers_identical_result_after_reopen() -> Result {
    let mut f = Fixture::new().await?;
    f.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let operation = "019f0000-0000-7000-8000-00000000d771";
    f.runtime(A)
        .await?
        .lose_reply_after_apply(operation.to_owned().try_into()?);
    let lost = create(&f, &owner, operation, "Lost reply", "retained description").await?;
    assert_eq!(lost.status, 503);
    assert!(lost.body.is_empty());
    assert_eq!(f.rows(A,"SELECT (SELECT COUNT(*) FROM workspaces),(SELECT COUNT(*) FROM request_log),(SELECT COUNT(*) FROM organization_change_records),(SELECT COUNT(*) FROM organization_used_requests)",vec![]).await?,vec![vec![number(1)?,number(1)?,number(2)?,number(1)?]],"the lost answer followed a real complete commit");
    let retained = f
        .rows(
            A,
            "SELECT answer FROM request_log WHERE request_id=?",
            vec![text(operation)],
        )
        .await?;
    let bytes = match retained.first().and_then(|row| row.first()) {
        Some(central_storage::SqlValue::Text(answer)) => answer.as_bytes().to_vec(),
        _ => return Err("retained answer missing".into()),
    };
    let before = durable_effects(&f).await?;
    f.reopen().await?;
    let recovered = create(&f, &owner, operation, "Lost reply", "retained description").await?;
    assert_eq!(recovered.status, 200);
    assert_eq!(
        recovered.body, bytes,
        "reopen replay returns the exact first retained answer"
    );
    assert_eq!(
        recovered.value()?,
        json!({"result":"applied","workspace":{"workspaceId":operation,"name":"Lost reply","description":"retained description","state":"active"}})
    );
    assert!(
        durable_effects(&f).await? == before,
        "recovery rewrote the directory, answer, actor or audit"
    );
    assert_eq!(
        f.rows(A, "SELECT COUNT(*) FROM organization_used_requests", vec![])
            .await?,
        vec![vec![number(2)?]]
    );
    assert_eq!(
        create(&f, &owner, operation, "Lost reply", "retained description")
            .await?
            .body,
        bytes
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A real trigger on the final ledger cleanup statement must roll back all earlier prepared effects."
)]
#[tokio::test]
async fn last_ledger_statement_failure_rolls_back_create_record_marker_and_actor_then_retry_applies_once()
-> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let expired_operation = "019f0000-0000-7000-8000-00000000d781";
    assert_eq!(
        create(&f, &owner, expired_operation, "Earlier real create", "")
            .await?
            .status,
        200
    );
    f.clock.advance(Duration::from_secs(86_401))?;
    let mut other = signed::Person::member(A, f.clock_time())?;
    other.permissions = vec!["workspace:create".to_owned()];
    // This is the actual final Statement from request_log::retain. The expired
    // row exists because a real older Create committed, so DELETE must visit it.
    f.apply(A,"CREATE TRIGGER setup_last_ledger_failure BEFORE DELETE ON request_log BEGIN SELECT RAISE(ABORT,'fixture last ledger cleanup failure'); END",vec![]).await?;
    let before = all_effects(&f).await?;
    let operation = "019f0000-0000-7000-8000-00000000d782";
    let jti = signed::fresh_request_id(f.clock_time())?;
    let headers = other.headers(
        signed::CREATE,
        &intent(operation, "Rollback create", "atomic description"),
        &jti,
        f.clock_time(),
    )?;
    let body = serde_json::to_string(&body(operation, "Rollback create", "atomic description"))?;
    let failed = post(&f.client(), signed::CREATE, body.clone(), headers.clone()).await?;
    assert_eq!(failed.status, 503);
    assert!(failed.body.is_empty());
    assert!(
        all_effects(&f).await? == before,
        "final ledger failure partially committed directory/audit/marker/actor/ledger"
    );
    f.apply(A, "DROP TRIGGER setup_last_ledger_failure", vec![])
        .await?;
    let healthy = post(&f.client(), signed::CREATE, body, headers).await?;
    assert_eq!(healthy.status, 200);
    assert_eq!(
        healthy.value()?.get("result"),
        Some(&json!("applied")),
        "exact same proof remains fresh after rollback"
    );
    assert_eq!(f.rows(A,"SELECT (SELECT COUNT(*) FROM workspaces),(SELECT COUNT(*) FROM organization_change_records),(SELECT COUNT(*) FROM organization_used_requests),(SELECT COUNT(*) FROM request_log),(SELECT COUNT(*) FROM actor_identities)",vec![]).await?,vec![vec![number(2)?,number(3)?,number(2)?,number(1)?,number(2)?]]);
    assert_eq!(
        create(
            &f,
            &other,
            operation,
            "Rollback create",
            "atomic description"
        )
        .await?
        .body,
        healthy.body
    );
    f.close().await
}
