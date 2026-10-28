//! Platform capability observations start from an actual enrolled session.

use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer_with_label, owner_assertion,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::Message;
use serde_json::{Value, json};
use std::time::Duration;

pub(super) async fn run(base: &str) -> ConformanceResult<()> {
    codec(base).await?;
    alarm_owner(base).await
}

pub(super) async fn observe(base: &str, command: Value) -> ConformanceResult<Value> {
    Ok(super::normal_session_support::bounded_http_client()?
        .post(format!("{base}/__conformance/admission"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&command)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

pub(super) async fn codec(base: &str) -> ConformanceResult<()> {
    let client = ConformanceClient::new(base)?;
    let service = "550e8400-e29b-41d4-a716-446655446001";
    let session = "019f0000-0000-7000-8000-000000006001";
    let label = "normal codec origin 🦊";
    let mut peer = enroll_admitted_peer_with_label(
        &client,
        service,
        "019f0000-0000-7000-8000-000000006002",
        "normal-session-codec",
        label,
        session,
    )
    .await?;
    let result = async {
        let command = json!({"kind":"sessionConnections","value":{"service":service}});
        let before = observe(base, command.clone()).await?;
        let [connection] = before.as_array().ok_or("connection snapshot is not an array")?.as_slice() else {
            return Err("codec fixture must have exactly one normal physical socket".into());
        };
        let id = connection.get("connection").and_then(Value::as_str)
            .ok_or("normal connection ID missing")?;
        let expected_origin = json!({"installation":service,"machine":label});
        if connection.get("peer") != Some(&json!(peer.peer))
            || connection.get("registration") != Some(&json!({
                "kind":"attached","sessionId":session,"schemaSupport":{"maxVersion":1}
            }))
            || connection.get("admissionOrigin") != Some(&expected_origin)
        {
            return Err(format!("codec fixture did not reach its exact normal binding: {before}").into());
        }
        let actual = observe(base, json!({"kind":"attachmentCodec","value":{
            "service":service,"connection":id
        }})).await?;
        let mut expected = Vec::new();
        for (support, sequence, expiry) in [
            ("0", "1", "0"),
            ("9007199254740993", "9007199254740993", "-9223372036854775808"),
            ("18446744073709551615", "18446744073709551615", "9223372036854775807"),
        ] {
            expected.push(json!({"connection":id,"peer":peer.peer,"sessionId":session,
                "schemaSupport":support,"attachSequence":sequence,"expiresAtMillis":expiry,
                "admissionOrigin":expected_origin}));
        }
        if actual != Value::Array(expected) {
            return Err(format!("workers-rs attachment codec changed exact values/origin: {actual}").into());
        }
        if observe(base, command).await? != before {
            return Err("codec observation did not restore the actual normal attachment".into());
        }
        peer.socket.send(Message::Text(json!({"kind":"hello","value":{
            "sessionId":session,"schemaSupport":{"maxVersion":1}
        }}).to_string())).await?;
        let response = tokio::time::timeout(Duration::from_secs(10), peer.socket.next()).await?
            .ok_or("codec observation lost the normal socket")??;
        let bytes = match response {
            Message::Text(text) => text.as_bytes().to_vec(),
            Message::Binary(bytes) => bytes.to_vec(),
            other => return Err(format!("normal Hello replay after codec returned {other:?}").into()),
        };
        if serde_json::from_slice::<Value>(&bytes)? != json!({"kind":"welcome","value":{
            "sessionId":session,"databases":[]
        }}) {
            return Err("normal Welcome changed after codec observation".into());
        }
        println!("SESSION_CAPABILITY_CASE normal_socket_workers_rs_exact_integer_codec_and_original_origin passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    let closed = peer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    result?;
    closed?;
    Ok(())
}

async fn alarm_owner(base: &str) -> ConformanceResult<()> {
    use super::holder_writer_retry::{read_status, received, send, start};
    use collaboration_central_conformance::{RuntimeControl, RuntimeControlRequest};
    use collaboration_central_core::{ObjectAddress, RegistryTime};
    use fleet_sync_protocol::{ReleaseAnswer, ReplicationPosition, ServerCall, ServerCallAnswer};
    use tokio::task::JoinSet;

    let client = ConformanceClient::new(base)?;
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?;
    let service = "550e8400-e29b-41d4-a716-446655446003";
    let session = "019f0000-0000-7000-8000-000000006003";
    let mut peer = enroll_admitted_peer_with_label(
        &client,
        service,
        "019f0000-0000-7000-8000-000000006004",
        "normal-session-alarm",
        "normal alarm owner",
        session,
    )
    .await?;
    let mut calls = JoinSet::new();
    let result = async {
        let connections = observe(base, json!({"kind":"sessionConnections","value":{
            "service":service
        }})).await?;
        let [connection] = connections.as_array().ok_or("alarm connections are not an array")?.as_slice() else {
            return Err("alarm fixture must own exactly one normal socket".into());
        };
        let connection = connection.get("connection").and_then(Value::as_str)
            .ok_or("normal alarm connection ID missing")?;
        let time = chrono::DateTime::from_timestamp_millis(1_900_000_000_000)
            .ok_or("invalid fixed alarm time")?;
        let pin = RuntimeControlRequest {
            address: ObjectAddress::Connection(message_board::ServiceId::try_from(service.to_owned())?),
            command: RuntimeControl::SetTime { now: RegistryTime(time) },
        };
        http.post(format!("{base}/admin/v1/conformance/runtime")).header("Cf-Access-Jwt-Assertion", owner_assertion()?).json(&pin).send().await?.error_for_status()?.bytes().await?;
        start(&http, base, &mut calls, json!({
            "attempt":"019f0000-0000-7000-8000-000000006005","holder":peer.peer,
            "request":{"kind":"release","value":{
                "database":"019f0000-0000-7000-8000-000000006006","epoch":1,
                "transfer":"019f0000-0000-7000-8000-000000006007"
            }}
        }));
        let (frame, request) = received(&mut peer.socket).await?;
        let ServerCall::Release { call, request } = request else {
            return Err("normal alarm case did not receive its actual Release".into());
        };
        let command = json!({"kind":"sessionAlarm","value":{
            "service":service,"connection":connection
        }});
        // The configured 20-second wait is armed by the real holder call.
        // A normal alarm event may still be settling its previous deadline;
        // wait for this call's own exact deadline, never a readiness sleep.
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let actual = observe(base, command.clone()).await?;
                if actual.get("connection") != Some(&json!(connection))
                    || actual.get("peer") != Some(&json!(peer.peer))
                    || actual.get("sessionId") != Some(&json!(session))
                {
                    return Err(format!("alarm was read from another normal session: {actual}").into());
                }
                if actual.get("alarmAtMillis") == Some(&json!("1900000020000")) {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                tokio::task::yield_now().await;
            }
        }).await.map_err(|_| "normal holder call did not arm its Connection object's exact alarm")??;
        let later = RuntimeControlRequest {
            address: pin.address.clone(),
            command: RuntimeControl::SetTime { now: RegistryTime(chrono::DateTime::from_timestamp_millis(1_900_000_005_000).ok_or("invalid later alarm time")?) },
        };
        http.post(format!("{base}/admin/v1/conformance/runtime")).header("Cf-Access-Jwt-Assertion", owner_assertion()?).json(&later).send().await?.error_for_status()?.bytes().await?;
        start(&http, base, &mut calls, json!({
            "attempt":"019f0000-0000-7000-8000-000000006008","holder":peer.peer,
            "request":{"kind":"release","value":{
                "database":"019f0000-0000-7000-8000-000000006009","epoch":1,
                "transfer":"019f0000-0000-7000-8000-000000006010"
            }}
        }));
        let (later_frame, later_request) = received(&mut peer.socket).await?;
        let ServerCall::Release { call: later_call, request: later_request } = later_request else { return Err("competing alarm did not receive its second real holder call".into()); };
        let actual = observe(base, command.clone()).await?;
        if actual.get("alarmAtMillis") != Some(&json!("1900000020000")) { return Err(format!("later pending deadline replaced the earliest same-object alarm: {actual}").into()); }
        let later_answer = ReleaseAnswer::Released { final_position: ReplicationPosition::new(later_request.epoch, fleet_sync_protocol::CommitSequence::new(0)) };
        send(&mut peer.socket, later_frame, ServerCallAnswer::Release(later_answer.clone())).await?;
        tokio::time::timeout(Duration::from_secs(10), async { loop {
            if read_status(&http, base, later_call.operation).await? == json!({"kind":"completed","value":{"kind":"release","value":later_answer}}) { return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(()); }
            tokio::task::yield_now().await;
        }}).await??;
        let answer = ReleaseAnswer::Released {
            final_position: ReplicationPosition::new(request.epoch, fleet_sync_protocol::CommitSequence::new(0)),
        };
        send(&mut peer.socket, frame, ServerCallAnswer::Release(answer.clone())).await?;
        let expected = json!({"kind":"completed","value":{"kind":"release","value":answer}});
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let actual = read_status(&http, base, call.operation).await?;
                if actual == expected {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                if actual.get("kind").and_then(Value::as_str) != Some("pending") {
                    return Err(format!("normal alarm holder call did not complete: {actual}").into());
                }
                tokio::task::yield_now().await;
            }
        }).await??;
        println!("SESSION_CAPABILITY_CASE normal_holder_call_arms_the_same_connection_objects_exact_alarm passed");
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }.await;
    calls.abort_all();
    while calls.join_next().await.is_some() {}
    let closed = peer
        .socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await;
    result?;
    closed?;
    Ok(())
}
