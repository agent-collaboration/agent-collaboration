//! Publication failures and finalize rollback on normally enrolled connections.

use super::{
    normal_capability_cases::observe,
    normal_session_support::{NormalSession, data, frame, physical_close},
};
use collaboration_central_conformance::ConformanceResult;
use futures_util::SinkExt;
use reqwest_websocket::Message;
use serde_json::{Value, json};
use std::time::Duration;

const BASE_TIME: i64 = 1_900_000_000_000;

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    publication(base).await?;
    rollback(base, "callback").await?;
    rollback(base, "sql").await
}

pub(super) async fn hold_schema(session: &NormalSession, base: &str) -> ConformanceResult<()> {
    session.pin(base, BASE_TIME).await?;
    session
        .control(
            base,
            json!({"kind":"holdNextDispatch","value":{"destination":{"kind":"registry"}}}),
        )
        .await?;
    Ok(())
}

pub(super) async fn held_schema(session: &NormalSession, base: &str) -> ConformanceResult<Value> {
    session.pin(base, BASE_TIME).await?;
    let observed = session
        .control(base, json!({"kind":"awaitHeldDispatch"}))
        .await?;
    let bytes: Vec<u8> = serde_json::from_value(
        observed
            .pointer("/dispatch/value/request")
            .cloned()
            .ok_or("schema dispatch bytes missing")?,
    )?;
    let request: Value = serde_json::from_slice(&bytes)?;
    if request.get("kind") != Some(&json!("fleet-sync.report-schema-support"))
        || request.pointer("/request/peer") != Some(&json!(session.peer))
        || request.pointer("/request/sessionId") != Some(&json!(session.session))
    {
        return Err(format!(
            "held dispatch is not the normal session's schema publication: {request}"
        )
        .into());
    }
    Ok(request)
}

async fn publication(base: &str) -> ConformanceResult<()> {
    let mut healthy = NormalSession::attached(base, "publication-healthy").await?;
    let mut failed = NormalSession::open(base, "publication-failed").await?;
    let result = async {
        hold_schema(&failed, base).await?;
        failed.fault(base, json!({"kind":"failRegistrationPublication","value":{"connection":failed.connection}})).await?;
        failed.hello().await?;
        held_schema(&failed, base).await?;
        failed.control(base, json!({"kind":"resumeDispatch"})).await?;
        physical_close(&mut failed.socket).await?;
        let fault = failed.fault(base, json!({"kind":"observe"})).await?;
        if fault.get("publication") != Some(&json!({"kind":"triggered","value":{"connection":failed.connection}})) {
            return Err(format!("targeted publication fault was not observed: {fault}").into());
        }
        println!("SESSION_CASE normal_committed_attachment_error_closes_exactly_failed_socket passed");
        healthy.hello().await?;
        healthy.welcome().await?;
        if healthy.registration(base).await? != json!({"kind":"attached","sessionId":healthy.session,"schemaSupport":{"maxVersion":1}}) {
            return Err("failed publication changed the separate healthy session".into());
        }
        healthy.hello().await?;
        healthy.welcome().await?;
        println!("SESSION_CASE normal_healthy_socket_remains_usable passed");
        let frame = "019f0000-0000-7000-8000-000000006301";
        healthy.socket.send(Message::Text(json!({"kind":"request","value":{
            "frame":frame,"body":{"kind":"lease","value":{"kind":"register","value":{
                "attempt":"019f0000-0000-7000-8000-000000006302",
                "database":"019f0000-0000-7000-8000-000000006303",
                "writer":"019f0000-0000-7000-8000-000000006304","origin":{"kind":"empty"}
            }}}
        }}).to_string())).await?;
        if data(&mut healthy.socket).await? != json!({"kind":"answer","value":{
            "frame":frame,"body":{"kind":"lease","value":{"kind":"register","value":{
                "kind":"refused","value":{"kind":"notWriter","value":{"current":null}}
            }}}
        }}) { return Err("Register mismatch lost the exact correlated NotWriter".into()); }
        healthy.hello().await?;
        healthy.welcome().await?;
        println!("SESSION_CASE normal_register_peer_mismatch_refused_without_closing_socket passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let closed = healthy.close().await;
    let _ = failed.close().await;
    result?;
    closed
}

async fn rollback(base: &str, fault: &str) -> ConformanceResult<()> {
    let mut session = NormalSession::open(base, &format!("finalize-{fault}")).await?;
    let result = async {
        hold_schema(&session, base).await?;
        session.fault(base, json!({"kind":"armBarrier","value":{"connection":session.connection}})).await?;
        session.fault(base, json!({"kind":"failFinalize","value":{"fault":fault}})).await?;
        session.hello().await?;
        let held = held_schema(&session, base).await?;
        let operation = held.pointer("/identity/operation").cloned().ok_or("schema CallId operation missing")?;
        session.control(base, json!({"kind":"resumeDispatch"})).await?;
        let observed = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let observed = session.fault(base, json!({"kind":"observe"})).await?;
                if observed.pointer("/finalize/kind") == Some(&json!("triggered")) { return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(observed); }
                tokio::task::yield_now().await;
            }
        }).await??;
        if observed.pointer("/finalize/value/failure/kind") != Some(&json!(fault))
            || observed.pointer("/finalize/value/failure/value/written").and_then(Value::as_u64).is_none_or(|written| written == 0) {
            return Err(format!("finalize fault did not follow real callback writes: {observed}").into());
        }
        if fault == "sql" && (observed.pointer("/finalize/value/failure/value/cause/kind") != Some(&json!("backend"))
            || !observed.pointer("/finalize/value/failure/value/cause/message").and_then(Value::as_str).is_some_and(|message| message.contains("integer overflow"))) {
            return Err(format!("SQL finalize fault omitted actual SQLx overflow: {observed}").into());
        }
        let registration = session.registration(base).await?;
        if registration.get("kind") != Some(&json!("attaching")) { return Err(format!("rollback leaked Attached: {registration}").into()); }
        let journal = observe(base, json!({"kind":"sessionJournal","value":{"service":session.service,"operation":operation}})).await?;
        if journal.pointer("/operation/id") != Some(&operation)
            || journal.pointer("/operation/kind") != Some(&json!("fleet-sync.publish-schema-support"))
            || journal.pointer("/operation/execution/state") != Some(&json!("running"))
            || journal.pointer("/report/name") != Some(&json!("report"))
            || journal.pointer("/report/state/state") != Some(&json!("call_done"))
            || journal.get("attach") != Some(&Value::Null) {
            return Err(format!("rollback leaked the attach receipt or lost its real report: {journal}").into());
        }
        observe(base, json!({"kind":"sessionBarrier","value":{"service":session.service,"connection":session.connection}})).await?;
        if !matches!(frame(&mut session.socket).await?, Message::Binary(bytes) if bytes.as_ref() == b"session-finalize-barrier") {
            return Err("rollback leaked Welcome before the ordered capability barrier".into());
        }
        session.pin(base, BASE_TIME + 60_000).await?;
        session.welcome().await?;
        session.hello().await?;
        session.welcome().await?;
        println!("SESSION_CASE normal_rollback_{fault}_discards_registration_welcome_and_attach_receipt_then_retries passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let closed = session.close().await;
    result?;
    closed
}
