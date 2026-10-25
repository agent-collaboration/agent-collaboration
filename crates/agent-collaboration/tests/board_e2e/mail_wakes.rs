//! LM04/LM05: creator-bound slots and due-relative ends through real listeners.
use super::mail_fixture::{MailFixture, at, identity, set_field};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};
use std::time::Duration;

fn request(suite: &MailFixture, name: &str) -> FixtureResult<Value> {
    let sender = suite.session(name)?;
    Ok(
        json!({"operationId":identity(),"requestedBy":creator(&sender)?,
        "replace":false,"message":{"target":suite.target,"content":{"kind":"agent","sender":sender,"text":"LM04 literal wake reminder"},"delivery":"auto"},
        "timing":{"kind":"after","seconds":600},"expiry":{"kind":"none"}}),
    )
}

fn creator(session: &Value) -> FixtureResult<Value> {
    Ok(
        json!({"kind":"session","endpoint":at(session, "/endpoint")?,"sessionId":at(session, "/sessionId")?}),
    )
}

async fn refuse(suite: &MailFixture, input: Value, expected: Value) -> FixtureResult {
    let reply = suite.host()?.mcp_reply("wake_send", input).await?;
    assert!(reply.is_error, "wake should refuse: {}", reply.value);
    for (field, value) in expected
        .as_object()
        .ok_or("expected refusal must be object")?
    {
        assert_eq!(
            reply.value.get(field),
            Some(value),
            "wake refusal field {field}"
        );
    }
    assert_eq!(
        reply.value.pointer("/effects/mutation"),
        Some(&json!("none"))
    );
    Ok(())
}

async fn show(suite: &MailFixture, id: &Value) -> FixtureResult<Value> {
    suite.host()?.mcp("wake_show", json!({"wakeupId":id})).await
}

fn require_same_schedule(expected: &Value, current: &Value) {
    // latestEventCursor includes observation time; scheduling state is persisted.
    for field in [
        "definition",
        "state",
        "nextDueAt",
        "firstFire",
        "pendingDeliveryId",
    ] {
        assert_eq!(
            current.get(field),
            expected.get(field),
            "refusal changed {field}"
        );
    }
}

pub async fn slots_and_limits(suite: &mut MailFixture) -> FixtureResult {
    let sender = suite.session("lm04-creator")?;
    let operation = "019f0000-0000-7000-8000-000000000404";
    let first = suite
        .cli(&[
            "wake",
            "send",
            "--to",
            &suite.target.to_string(),
            "--from",
            &sender.to_string(),
            "--text",
            "LM04 literal wake reminder",
            "--after",
            "10m",
            "--operation-id",
            operation,
        ])
        .await?;
    let first_id = at(&first, "/definition/wakeupId")?.clone();
    assert_eq!(
        first.pointer("/definition/createdBy"),
        Some(&creator(&sender)?)
    );
    for replacement in [false, true] {
        let mut outsider = request(suite, "lm04-outsider")?;
        set_field(&mut outsider, "replace", json!(replacement))?;
        refuse(
            suite,
            outsider,
            json!({"kind":"wakeExists","wakeup":first_id,"creator":creator(&sender)?}),
        )
        .await?;
        require_same_schedule(&first, &show(suite, &first_id).await?);
    }
    let mut replacement = request(suite, "lm04-creator")?;
    set_field(&mut replacement, "replace", json!(true))?;
    let second = suite.host()?.mcp("wake_send", replacement.clone()).await?;
    let second_id = at(&second, "/definition/wakeupId")?.clone();
    assert_ne!(second_id, first_id);
    assert_eq!(
        at(&show(suite, &first_id).await?, "/state")?,
        &json!("cancelled")
    );
    assert_eq!(suite.host()?.mcp("wake_send", replacement).await?, second);
    let mut target = request(suite, "lm04-target-request")?;
    set_field(&mut target, "replace", json!(true))?;
    set_field(&mut target, "requestedBy", creator(&suite.target)?)?;
    set_field(
        target
            .pointer_mut("/message/content")
            .ok_or("wake content absent")?,
        "sender",
        suite.target.clone(),
    )?;
    let target_wake = suite.host()?.mcp("wake_send", target).await?;
    let current_id = at(&target_wake, "/definition/wakeupId")?.clone();
    assert_eq!(
        target_wake.pointer("/definition/createdBy"),
        Some(&creator(&suite.target)?)
    );
    assert_eq!(
        at(&show(suite, &second_id).await?, "/state")?,
        &json!("cancelled")
    );
    let mut replay = request(suite, "lm04-creator")?;
    set_field(&mut replay, "operationId", json!(operation))?;
    assert_eq!(suite.host()?.mcp("wake_send", replay).await?, first);
    lifetime_refusals(suite, &current_id, &target_wake).await?;
    let owner_wake = owner_repeat_replacement(suite, &current_id).await?;
    assert_eq!(suite.peer()?.count_after_barrier().await?, 0);
    suite.stop_host().await?;
    suite.reopen_host().await?;
    require_same_schedule(
        &owner_wake,
        &show(suite, at(&owner_wake, "/definition/wakeupId")?).await?,
    );
    assert_eq!(suite.peer()?.count_after_barrier().await?, 0);
    suite.stop_host().await?;
    assert_eq!(suite.peer()?.count_after_barrier().await?, 0);
    Ok(())
}

async fn owner_repeat_replacement(suite: &MailFixture, current_id: &Value) -> FixtureResult<Value> {
    let mut owner = request(suite, "lm04-owner-replacement")?;
    set_field(&mut owner, "requestedBy", json!({"kind":"owner"}))?;
    set_field(&mut owner, "replace", json!(true))?;
    // The accepted side of the repeat bounds matters too; this creates no shortened repeat.
    set_field(
        &mut owner,
        "timing",
        json!({"kind":"interval","seconds":300}),
    )?;
    set_field(
        &mut owner,
        "expiry",
        json!({"kind":"after","seconds":604800}),
    )?;
    let owner_wake = suite.host()?.mcp("wake_send", owner).await?;
    assert_eq!(
        owner_wake.pointer("/definition/createdBy"),
        Some(&json!({"kind":"owner"}))
    );
    assert_eq!(
        at(&show(suite, current_id).await?, "/state")?,
        &json!("cancelled")
    );
    Ok(owner_wake)
}

async fn lifetime_refusals(
    suite: &MailFixture,
    current: &Value,
    retained: &Value,
) -> FixtureResult {
    let cases = [
        (
            json!({"kind":"after","seconds":86401}),
            json!({"kind":"none"}),
            json!({"kind":"tooFarAhead","max":86400}),
        ),
        (
            json!({"kind":"after","seconds":600}),
            json!({"kind":"after","seconds":87001}),
            json!({"kind":"lifetimeTooLong","max":86400}),
        ),
        (
            json!({"kind":"interval","seconds":300}),
            json!({"kind":"none"}),
            json!({"kind":"expiryRequired"}),
        ),
        (
            json!({"kind":"interval","seconds":300}),
            json!({"kind":"after","seconds":604801}),
            json!({"kind":"lifetimeTooLong","max":604800}),
        ),
        (
            json!({"kind":"interval","seconds":299}),
            json!({"kind":"after","seconds":7200}),
            json!({"kind":"intervalTooShort","min":300}),
        ),
        (
            json!({"kind":"cron","expression":"* * * * *","timezone":"UTC"}),
            json!({"kind":"after","seconds":7200}),
            json!({"kind":"intervalTooShort","min":300}),
        ),
    ];
    for (timing, expiry, expected) in cases {
        let mut input = request(suite, "lm04-invalid-replacement")?;
        set_field(&mut input, "requestedBy", json!({"kind":"owner"}))?;
        set_field(&mut input, "replace", json!(true))?;
        set_field(&mut input, "timing", timing)?;
        set_field(&mut input, "expiry", expiry)?;
        refuse(suite, input, expected).await?;
        require_same_schedule(retained, &show(suite, current).await?);
    }
    Ok(())
}

fn millis(value: &Value, pointer: &str) -> FixtureResult<i64> {
    let timestamp = at(value, pointer)?
        .as_str()
        .ok_or("wake timestamp absent")?;
    Ok(chrono::DateTime::parse_from_rfc3339(timestamp)?.timestamp_millis())
}

async fn due_cli(
    suite: &MailFixture,
    delay: &str,
    lifetime: Option<&str>,
) -> FixtureResult<(i32, Value)> {
    let target = suite.target.to_string();
    let mut command = vec![
        "wake",
        "send",
        "--human-user",
        "--to",
        &target,
        "--text",
        "LM05 due-relative literal reminder",
        "--replace",
        "--after",
        delay,
    ];
    if let Some(lifetime) = lifetime {
        command.extend(["--for", lifetime]);
    }
    suite.cli_output(&command).await
}

pub async fn due_relative_end(suite: &mut MailFixture) -> FixtureResult {
    let (code, first) = due_cli(suite, "24h", None).await?;
    assert_eq!(code, 0);
    let first = at(&first, "/result/record")?;
    assert_eq!(
        millis(first, "/nextDueAt")? - millis(first, "/definition/createdAt")?,
        86_400_000
    );
    assert_eq!(
        millis(first, "/definition/expiresAt")? - millis(first, "/nextDueAt")?,
        86_400_000
    );
    let (code, exact) = due_cli(suite, "24h", Some("48h")).await?;
    assert_eq!(code, 0);
    let exact = at(&exact, "/result/record")?;
    assert_eq!(
        millis(exact, "/definition/expiresAt")? - millis(exact, "/definition/createdAt")?,
        172_800_000
    );
    let (code, refused) = due_cli(suite, "24h", Some("172801s")).await?;
    assert_eq!(code, 4);
    assert_eq!(
        refused.pointer("/error/kind"),
        Some(&json!("lifetimeTooLong"))
    );
    assert_eq!(refused.pointer("/error/max"), Some(&json!(86400)));
    assert_eq!(
        refused.pointer("/error/effects/mutation"),
        Some(&json!("none"))
    );
    require_same_schedule(
        exact,
        &show(suite, at(exact, "/definition/wakeupId")?).await?,
    );
    let (code, short) = due_cli(suite, "1s", None).await?;
    assert_eq!(code, 0);
    let id = at(&short, "/result/record/definition/wakeupId")?.clone();
    let received = suite
        .peer_mut()?
        .expect_text_with_timeout(
            "LM05 due-relative literal reminder",
            Duration::from_secs(15),
        )
        .await?;
    let shown = show(suite, &id).await?;
    assert!(!at(&shown, "/firstFire")?.is_null());
    assert_eq!(at(&shown, "/nextDueAt")?, &Value::Null);
    let push = received
        .split("/push/")
        .nth(1)
        .and_then(|tail| tail.get(..36))
        .ok_or("wake peer frame omitted push identity")?;
    uuid::Uuid::parse_str(push)?;
    let public_push = suite
        .host()?
        .mcp(
            "router_show",
            json!({"caller":suite.target,"reference":push}),
        )
        .await?;
    let origin: Value = serde_json::from_str(
        at(&public_push, "/record/originRouterRef")?
            .as_str()
            .ok_or("wake push origin absent")?,
    )?;
    assert_eq!(
        origin,
        json!({"kind":"wake","wakeupId":id,"occurrenceId":at(&shown, "/firstFire/occurrenceId")?})
    );
    assert_eq!(
        public_push.pointer("/record/body"),
        Some(&json!("LM05 due-relative literal reminder"))
    );
    assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    suite.stop_host().await?;
    suite.reopen_host().await?;
    let reopened = show(suite, &id).await?;
    assert_eq!(at(&reopened, "/firstFire")?, at(&shown, "/firstFire")?);
    assert_eq!(at(&reopened, "/definition")?, at(&shown, "/definition")?);
    assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    suite.stop_host().await?;
    assert_eq!(suite.peer()?.count_after_barrier().await?, 1);
    Ok(())
}
