use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use collaboration_protocol::{AutomationPageRequest, OperationId, WakeSendRequest};
use serde_json::{Value, json};
#[path = "support/central_catalog_fixture.rs"]
mod fixture;
#[path = "support/legacy_wake.rs"]
mod legacy;
use fixture::{CatalogFixture, ProofResult};
#[tokio::test]
async fn wake_cursor_uses_closed_scoped_base64url_contract() -> ProofResult {
    exercise_cursor(5, 1).await
}
#[tokio::test]
async fn large_wake_messages_paginate_within_the_control_frame_limit() -> ProofResult {
    exercise_cursor(600000, 100).await
}
async fn exercise_cursor(text_bytes: usize, page_limit: u32) -> ProofResult {
    let fixture = CatalogFixture::open(|transport| transport, true).await?;
    let served = &fixture.api;
    let store = fixture
        .store
        .as_ref()
        .ok_or("local state fixture missing")?;
    let client = served.client("cursor-test").await?;
    // MS-10: independent page records occupy independent target slots.
    for index in 0..2 {
        let request: WakeSendRequest = serde_json::from_value(
            json!({"operationId":OperationId::generate(),"message":{"target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":format!("fixture-{index}")},"content":{"kind":"humanUser","text":"x".repeat(text_bytes)},"delivery":"auto","generationGuard":null},"timing":{"kind":"after","seconds":60},"requestedBy":{"kind":"owner"},"replace":false,"expiry":{"kind":"none"}}),
        )?;
        legacy::seed(store, request).await?;
    }
    let first = client
        .list_wakeups(AutomationPageRequest {
            cursor: None,
            limit: page_limit.try_into()?,
        })
        .await?;
    if first.records.len() != 1 {
        return Err("first page did not respect count or encoded frame budget".into());
    }
    let first_id = legacy::snapshot(first.records.first().ok_or("first wake missing")?.clone())?
        .definition
        .wakeup_id;
    let cursor = first.next_cursor.ok_or("missing cursor")?;
    let mut payload: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(&cursor)?)?;
    if payload.as_object().map(|object| object.len()) != Some(3)
        || payload.get("version") != Some(&json!(1))
        || payload.pointer("/position/kind") != Some(&json!("local"))
        || payload
            .pointer("/position/after")
            .and_then(Value::as_object)
            .map(|object| object.len())
            != Some(4)
    {
        return Err(
            "cursor did not retain its closed service scope and actual local position".into(),
        );
    }
    let mut unknown = payload.clone();
    unknown
        .pointer_mut("/position/after")
        .and_then(Value::as_object_mut)
        .ok_or("local cursor position missing")?
        .insert("unexpected".into(), json!(true));
    if client
        .list_wakeups(AutomationPageRequest {
            cursor: Some(URL_SAFE_NO_PAD.encode(serde_json::to_vec(&unknown)?)),
            limit: 1_u32.try_into()?,
        })
        .await
        .is_ok()
    {
        return Err("unknown nested cursor fields were accepted".into());
    }
    let second = client
        .list_wakeups(AutomationPageRequest {
            cursor: Some(cursor),
            limit: page_limit.try_into()?,
        })
        .await?;
    if second.records.len() != 1 || second.next_cursor.is_none() {
        return Err("cursor did not continue listing".into());
    }
    if legacy::snapshot(second.records.first().ok_or("second wake missing")?.clone())?
        .definition
        .wakeup_id
        == first_id
    {
        return Err("byte-limited pagination repeated the previous wake".into());
    }
    let central_end = client
        .list_wakeups(AutomationPageRequest {
            cursor: second.next_cursor,
            limit: page_limit.try_into()?,
        })
        .await?;
    if !central_end.records.is_empty() || central_end.next_cursor.is_some() {
        return Err("local phase did not finish through the actual empty central page".into());
    }
    payload.as_object_mut().ok_or("cursor not object")?.insert(
        "serviceId".into(),
        json!("00000000-0000-4000-8000-000000000003"),
    );
    let foreign = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload)?);
    if client
        .list_wakeups(AutomationPageRequest {
            cursor: Some(foreign),
            limit: 1.try_into()?,
        })
        .await
        .is_ok()
    {
        return Err("foreign service cursor was accepted".into());
    }
    fixture.close().await
}
