//! Holder correlation and lifecycle through normal admission and caller journals.

use super::{
    holder_writer_retry::{owner_post, pending_reply, read_status, received, send, start},
    normal_capability_cases::observe,
    normal_session_cases::{held_schema, hold_schema},
    normal_session_support::{NormalSession, bounded_http_client, data, frame, physical_close},
};
use collaboration_central_conformance::ConformanceResult;
use durable_steps::{CallId, OperationId};
use fleet_sync_protocol::{
    FrameId, ReleaseAnswer, ReplicationPosition, ServerCall, ServerCallAnswer,
};
use futures_util::SinkExt;
use reqwest_websocket::Message;
use serde_json::{Value, json};
use std::time::Duration;
use tokio::task::JoinSet;

const BASE_TIME: i64 = 1_900_000_000_000;

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    readiness(base).await?;
    correlation(base).await?;
    loss_and_retry(base).await?;
    beside_schema(base).await?;
    session_identity(base).await
}

fn release(session: &NormalSession) -> Value {
    json!({"attempt":durable_steps::Uuid::now_v7(),"holder":session.peer,
    "request":{"kind":"release","value":{
        "database":durable_steps::Uuid::now_v7(),"epoch":1,"transfer":durable_steps::Uuid::now_v7()
    }}})
}

async fn pin_registry(base: &str, millis: i64) -> ConformanceResult<()> {
    let now = chrono::DateTime::from_timestamp_millis(millis).ok_or("invalid registry time")?;
    owner_post(
        &bounded_http_client()?,
        base,
        "runtime",
        json!({"address":{"kind":"registry"},"command":{"kind":"setTime","value":{"now":now}}}),
    )
    .await?;
    Ok(())
}

async fn frames(session: &NormalSession, base: &str) -> ConformanceResult<Value> {
    observe(
        base,
        json!({"kind":"holderFrames","value":{"service":session.service}}),
    )
    .await
}

async fn observation(session: &NormalSession, base: &str) -> ConformanceResult<(usize, u64)> {
    Ok(serde_json::from_value(
        observe(
            base,
            json!({"kind":"holderObservation","value":{"service":session.service}}),
        )
        .await?,
    )?)
}

async fn validated_answer(
    session: &NormalSession,
    base: &str,
    before: u64,
    pending: usize,
) -> ConformanceResult<()> {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let actual = observation(session, base).await?;
            if actual.1 > before {
                if actual.0 != pending {
                    return Err(format!(
                        "mismatched validated answer consumed a waiter: {actual:?}"
                    )
                    .into());
                }
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    Ok(())
}

async fn refused(session: &mut NormalSession, base: &str) -> ConformanceResult<()> {
    let input = release(session);
    let mut tasks = JoinSet::new();
    start(&bounded_http_client()?, base, &mut tasks, input);
    let actual = tokio::select! {
        reply = tasks.join_next() => reply.ok_or("ineligible holder call disappeared")??,
        message = frame(&mut session.socket) => {
            settle(&mut tasks).await;
            return Err(format!("ineligible normal holder emitted a wire frame before refusal: {:?}", message?).into());
        }
    };
    settle(&mut tasks).await;
    let actual = actual?;
    if actual.get("kind") != Some(&json!("pending"))
        || actual.pointer("/value/step/state/attempt/reach") != Some(&json!("never_sent"))
        || !frames(session, base)
            .await?
            .as_array()
            .is_some_and(Vec::is_empty)
    {
        return Err(
            format!("ineligible normal holder was not refused before send: {actual}").into(),
        );
    }
    Ok(())
}

async fn request(
    session: &mut NormalSession,
    base: &str,
    tasks: &mut JoinSet<ConformanceResult<Value>>,
    input: Value,
) -> ConformanceResult<(FrameId, CallId, ReleaseAnswer)> {
    start(&bounded_http_client()?, base, tasks, input);
    let (frame, request) = received(&mut session.socket).await?;
    let ServerCall::Release { call, request } = request else {
        return Err("holder did not receive Release".into());
    };
    Ok((
        frame,
        call,
        ReleaseAnswer::Released {
            final_position: ReplicationPosition::new(
                request.epoch,
                fleet_sync_protocol::CommitSequence::new(0),
            ),
        },
    ))
}

async fn completed(
    base: &str,
    operation: OperationId,
    answer: ReleaseAnswer,
) -> ConformanceResult<()> {
    let expected = json!({"kind":"completed","value":{"kind":"release","value":answer}});
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let actual = read_status(&bounded_http_client()?, base, operation).await?;
            if actual == expected {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            if actual.get("kind") != Some(&json!("pending")) {
                return Err(format!("holder did not complete exact caller: {actual}").into());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    Ok(())
}

async fn settle(tasks: &mut JoinSet<ConformanceResult<Value>>) {
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
}

async fn readiness(base: &str) -> ConformanceResult<()> {
    pin_registry(base, BASE_TIME).await?;
    let mut session = NormalSession::open(base, "holder-readiness").await?;
    let result = async {
        refused(&mut session, base).await?;
        session.barrier(base).await?;
        hold_schema(&session, base).await?;
        session.hello().await?;
        held_schema(&session, base).await?;
        refused(&mut session, base).await?;
        session.barrier(base).await?;
        session.control(base, json!({"kind":"resumeDispatch"})).await?;
        session.welcome().await?;
        session.pin(base, 2_000_000_000_000).await?;
        refused(&mut session, base).await?;
        session.barrier(base).await?;
        session.pin(base, BASE_TIME).await?;
        let mut replacement = session.replacement(base, "holder-readiness", BASE_TIME).await?;
        let proof = async {
            if data(&mut session.socket).await? != json!({"kind":"closed","value":{"reason":{"kind":"connectionReplaced"}}}) { return Err("same-session replacement lost typed close".into()); }
            physical_close(&mut session.socket).await?;
            let mut tasks = JoinSet::new();
            let input = release(&replacement);
            let answer = async {
                let (frame, call, answer) = request(&mut replacement, base, &mut tasks, input).await?;
                send(&mut replacement.socket, frame, ServerCallAnswer::Release(answer.clone())).await?;
                completed(base, call.operation, answer).await
            }.await;
            settle(&mut tasks).await;
            answer?;
            println!("HOLDER_CONTROL_CASE normal_opening_attaching_expiry_and_replacement_selection passed");
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }.await;
        let closed = replacement.close().await;
        proof?;
        closed
    }.await;
    let _ = session.close().await;
    result
}

async fn correlation(base: &str) -> ConformanceResult<()> {
    let mut holder = NormalSession::attached(base, "holder-correlation").await?;
    let mut other = NormalSession::attached(base, "holder-correlation-other").await?;
    holder.pin(base, BASE_TIME).await?;
    let mut tasks = JoinSet::new();
    let input = release(&holder);
    let result = async {
        let (frame, call, answer) = request(&mut holder, base, &mut tasks, input).await?;
        let before = observation(&other, base).await?.1;
        send(&mut other.socket, frame, ServerCallAnswer::Release(answer.clone())).await?;
        validated_answer(&other, base, before, 0).await?;
        if frames(&holder, base).await? != json!([frame]) { return Err("other connection consumed the target waiter".into()); }
        let before = observation(&holder, base).await?.1;
        send(&mut holder.socket, FrameId::new(durable_steps::Uuid::now_v7())?, ServerCallAnswer::Release(answer.clone())).await?;
        validated_answer(&holder, base, before, 1).await?;
        send(&mut holder.socket, frame, ServerCallAnswer::Release(answer.clone())).await?;
        completed(base, call.operation, answer).await?;
        if frames(&holder, base).await? != json!([]) { return Err("successful answer retained its waiter".into()); }
        println!("HOLDER_CONTROL_CASE normal_exact_connection_frame_and_separate_event_without_guard passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    settle(&mut tasks).await;
    let closed = holder.close().await;
    let other_closed = other.close().await;
    result?;
    closed?;
    other_closed
}

async fn loss_and_retry(base: &str) -> ConformanceResult<()> {
    let mut holder = NormalSession::attached(base, "holder-loss").await?;
    holder.pin(base, BASE_TIME).await?;
    let mut tasks = JoinSet::new();
    let input = release(&holder);
    let result = async {
        let (lost_frame, call, answer) = request(&mut holder, base, &mut tasks, input).await?;
        let mut replacement = holder.replacement(base, "holder-loss", BASE_TIME).await?;
        let proof = async {
            if data(&mut holder.socket).await?
                != json!({"kind":"closed","value":{"reason":{"kind":"connectionReplaced"}}})
            {
                return Err("pending holder replacement lost typed close".into());
            }
            physical_close(&mut holder.socket).await?;
            pending_reply(&mut tasks).await?;
            if frames(&replacement, base).await? != json!([]) {
                return Err("closed connection retained its pending waiter".into());
            }
            pin_registry(base, BASE_TIME + 60_000).await?;
            let (timed_frame, retry) = received(&mut replacement.socket).await?;
            let ServerCall::Release {
                call: retry_call, ..
            } = retry
            else {
                return Err("close retry lost Release".into());
            };
            if retry_call != call || timed_frame == lost_frame {
                return Err("close retry changed CallId or reused frame".into());
            }
            replacement.pin(base, BASE_TIME + 3_000).await?;
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    if frames(&replacement, base).await? == json!([]) {
                        return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await??;
            pin_registry(base, BASE_TIME + 120_000).await?;
            let (fresh, retry) = received(&mut replacement.socket).await?;
            let ServerCall::Release {
                call: retry_call, ..
            } = retry
            else {
                return Err("alarm retry lost Release".into());
            };
            if retry_call != call || fresh == timed_frame {
                return Err("alarm retry changed durable CallId or reused frame".into());
            }
            let before = observation(&replacement, base).await?.1;
            send(
                &mut replacement.socket,
                timed_frame,
                ServerCallAnswer::Release(answer.clone()),
            )
            .await?;
            validated_answer(&replacement, base, before, 1).await?;
            send(
                &mut replacement.socket,
                fresh,
                ServerCallAnswer::Release(answer.clone()),
            )
            .await?;
            completed(base, call.operation, answer).await?;
            println!(
                "HOLDER_CONTROL_CASE normal_close_alarm_and_late_answer_on_fresh_frame passed"
            );
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        }
        .await;
        let closed = replacement.close().await;
        proof?;
        closed
    }
    .await;
    settle(&mut tasks).await;
    let _ = holder.close().await;
    result
}

pub(super) async fn beside_schema(base: &str) -> ConformanceResult<()> {
    let mut holder = NormalSession::attached(base, "holder-beside-schema").await?;
    holder.pin(base, BASE_TIME).await?;
    let mut attaching = NormalSession::open(base, "holder-held-schema").await?;
    let mut tasks = JoinSet::new();
    let input = release(&holder);
    let result = async {
        hold_schema(&attaching, base).await?;
        attaching.hello().await?;
        held_schema(&attaching, base).await?;
        let (frame, call, answer) = request(&mut holder, base, &mut tasks, input).await?;
        send(
            &mut holder.socket,
            frame,
            ServerCallAnswer::Release(answer.clone()),
        )
        .await?;
        completed(base, call.operation, answer).await?;
        let observed = attaching.control(base, json!({"kind":"observe"})).await?;
        if observed.pointer("/dispatch/kind") != Some(&json!("held")) {
            return Err("schema publication was not held beside the completed answer".into());
        }
        attaching
            .control(base, json!({"kind":"resumeDispatch"}))
            .await?;
        attaching.welcome().await?;
        println!(
            "HOLDER_CONTROL_CASE normal_answer_completes_beside_a_held_schema_publication passed"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    settle(&mut tasks).await;
    let closed = holder.close().await;
    let attaching_closed = attaching.close().await;
    result?;
    closed?;
    attaching_closed
}

async fn session_identity(base: &str) -> ConformanceResult<()> {
    let mut holder = NormalSession::attached(base, "holder-session-identity").await?;
    let result = async {
        observe(base, json!({"kind":"holderSessionIdentity","value":{"service":holder.service,"connection":holder.connection}})).await?;
        holder.socket.send(Message::Text(json!({"kind":"hello","value":{
            "sessionId":durable_steps::Uuid::now_v7(),"schemaSupport":{"maxVersion":1}
        }}).to_string())).await?;
        if data(&mut holder.socket).await? != json!({"kind":"closed","value":{"reason":{"kind":"malformedFrame"}}}) { return Err("changed SessionId was not rejected before holder correlation".into()); }
        physical_close(&mut holder.socket).await?;
        println!("HOLDER_CONTROL_CASE normal_admission_then_hub_session_identity_primitive_and_changed_session_wire_close passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let _ = holder.close().await;
    result
}
