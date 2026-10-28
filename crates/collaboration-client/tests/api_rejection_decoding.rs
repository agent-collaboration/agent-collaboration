//! Published rejections and refused deliveries keep their typed evidence through the client.
use collaboration_client::protocol::{
    DeliveryNextAction, DeliveryOutcome, DeliveryRejectionReason, OperationEffect,
    SessionMessageSendParams,
};
use collaboration_client::{ClientError, CollaborationClient, operation_failure_from_client_error};
use serde_json::{Value, json};

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "support/scripted_api.rs"]
mod scripted_api;
use scripted_api::{SCRIPTED_SERVICE_ID, ScriptedApi};

#[tokio::test]
async fn client_preserves_valid_cursor_errors() {
    // Arrange: valid errors have deliberately different data shapes.
    let cases = [
        ("addresses_list", json!({"kind":"snapshotExpired"})),
        (
            "journal_read",
            json!({"kind":"historyExpired","current":{"journalId":"00000000-0000-4000-8000-000000000001","earliestSequence":4,"lastSequence":9}}),
        ),
    ];
    for (tool, data) in cases {
        // Act: the Router publishes the rejection as an operation failure tool error.
        let result = receive_failure(tool, published_rejection(-32050, data.clone()))
            .await
            .unwrap();
        // Assert: validity does not erase cursor bounds, correlation or partial effects.
        assert!(
            matches!(result,ClientError::Rejected {code:-32050,data:Some(actual)} if actual==data),
            "{tool}"
        );
    }
}

#[tokio::test]
async fn message_result_preserves_native_rejection_reason_action_and_code() {
    let (api, mut calls) = ScriptedApi::start().unwrap();
    let fixture = tokio::spawn(async move {
        let request = calls.next().await.unwrap();
        assert_eq!(request.tool, "message_send");
        let push_id = "01900000-0000-7000-8000-000000000003";
        let target = request.arguments["target"].clone();
        request.fail(json!({
            "mcpResult":"error",
            "kind":"rejected",
            "message":"Delivery was rejected",
            "effect":"none",
            "pushId":push_id,
            "link":format!("router://{SCRIPTED_SERVICE_ID}/push/{push_id}"),
            "target":target,
            "targetIdentity":"fixture",
            "deliveryState":"rejected",
            "receipt":{
                "outcome":{"kind":"rejected","reason":"childThread","nextAction":"inspectTarget","clientCode":-32000,"detail":null},
                "reachability":"codexAppServer","client":null
            }
        }));
    });
    let client = CollaborationClient::connect(api.directory(), "error-contract", "1")
        .await
        .unwrap();
    let request: SessionMessageSendParams = serde_json::from_value(json!({
        "target":{"endpoint":{"serviceId":SCRIPTED_SERVICE_ID,"endpointId":"codex-local"},"sessionId":"owned"},
        "message":{"kind":"humanUser","text":"fixture"},"mode":"auto",
        "generationGuard":null
    }))
    .unwrap();
    let receipt = client.send_human_input(request).await.unwrap();
    let collaboration_client::protocol::MessageSendResult::Local(receipt) = receipt else {
        panic!("local rejection returned remote sender custody");
    };
    assert!(
        matches!(receipt.receipt.outcome, DeliveryOutcome::Rejected(rejection)
        if rejection.reason == DeliveryRejectionReason::ChildThread
            && rejection.next_action == DeliveryNextAction::InspectTarget
            && rejection.client_code == Some(-32000))
    );
    fixture.await.unwrap();
}

/// The tool error the Router publishes for a rejection, as its presentation builds it.
fn published_rejection(code: i64, data: Value) -> Value {
    let failure = operation_failure_from_client_error(
        ClientError::Rejected {
            code,
            data: Some(data),
        },
        OperationEffect::None,
    );
    let mut published = serde_json::to_value(failure).unwrap_or(Value::Null);
    if let Some(fields) = published.as_object_mut() {
        fields.insert("mcpResult".to_owned(), json!("error"));
    }
    published
}

async fn receive_failure(
    tool: &'static str,
    failure: Value,
) -> Result<ClientError, Box<dyn std::error::Error + Send + Sync>> {
    let (api, mut calls) = ScriptedApi::start()?;
    let fixture = tokio::spawn(async move {
        let request = calls.next().await?;
        if request.tool != tool {
            return Err(std::io::Error::other("unexpected tool"));
        }
        request.fail(failure);
        Ok(())
    });
    let client = CollaborationClient::connect(api.directory(), "error-contract", "1").await?;
    let endpoint = serde_json::from_value(
        json!({"serviceId":SCRIPTED_SERVICE_ID,"endpointId":"codex-local"}),
    )?;
    let result = match tool {
        "addresses_list" => client
            .list_addresses(&endpoint, 100, None)
            .await
            .map(|_| ()),
        "journal_read" => client
            .read_journal(
                &endpoint,
                serde_json::from_value(
                    json!({"journalId":"00000000-0000-4000-8000-000000000001","sequence":0}),
                )?,
                100,
                0,
            )
            .await
            .map(|_| ()),
        _ => return Err("unexpected fixture tool".into()),
    };
    fixture.await??;
    match result {
        Ok(()) => Err("the published failure was accepted".into()),
        Err(error) => Ok(error),
    }
}

#[tokio::test]
async fn rpc_error_requires_matching_call_id_before_preserving_rejection() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    // The published availability payload must not qualify another request's answer.
    // Exercise the real Unix HTTP decoder in both supported response carriers.
    for streaming in [false, true] {
        for response_id in [json!(1), json!(9), Value::Null] {
            let directory = tempfile::tempdir().unwrap();
            let listener =
                tokio::net::UnixListener::bind(directory.path().join("control.sock")).unwrap();
            std::fs::write(
                directory.path().join("service.json"),
                json!({
                    "version":3,"serviceId":SCRIPTED_SERVICE_ID,
                    "serviceEpoch":"00000000-0000-4000-8000-000000000002",
                    "machineLabel":"correlation-fixture","serviceVersion":"1",
                    "api":{"transport":"streamableHttpUnix","path":"control.sock"},
                    "mcp":{"transport":"streamableHttp","url":"http://127.0.0.1:0/mcp"}
                })
                .to_string(),
            )
            .unwrap();
            let expected_match = response_id == json!(1);
            let published = json!({"kind":"unavailable","stage":"discovery",
                "message":"private-correlation-sentinel"});
            let expected_data = published.clone();
            let fixture = tokio::spawn(async move {
                loop {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    let mut request = Vec::new();
                    let mut buffer = [0_u8; 4096];
                    let header_end = loop {
                        let count = stream.read(&mut buffer).await.unwrap();
                        if count == 0 {
                            break None;
                        }
                        request.extend_from_slice(buffer.get(..count).unwrap());
                        if let Some(position) =
                            request.windows(4).position(|part| part == b"\r\n\r\n")
                        {
                            break Some(position + 4);
                        }
                        assert!(request.len() < 65536);
                    };
                    let Some(header_end) = header_end else {
                        continue;
                    };
                    let headers = String::from_utf8_lossy(request.get(..header_end).unwrap());
                    let body_length: usize = headers
                        .lines()
                        .filter_map(|line| line.split_once(':'))
                        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                        .unwrap()
                        .1
                        .trim()
                        .parse()
                        .unwrap();
                    while request.len() < header_end + body_length {
                        let count = stream.read(&mut buffer).await.unwrap();
                        assert_ne!(count, 0);
                        request.extend_from_slice(buffer.get(..count).unwrap());
                    }
                    let request: Value =
                        serde_json::from_slice(request.get(header_end..).unwrap()).unwrap();
                    assert_eq!(request.get("id").unwrap(), 1);
                    assert_eq!(request.pointer("/params/name").unwrap(), "endpoints_list");
                    let message = json!({"jsonrpc":"2.0","id":response_id,
                        "error":{"code":-32050,"message":"unavailable","data":published}});
                    let (content_type, body) = if streaming {
                        (
                            "text/event-stream",
                            format!("event: message\ndata: {message}\n\n"),
                        )
                    } else {
                        ("application/json", message.to_string())
                    };
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                    stream.shutdown().await.unwrap();
                    break;
                }
            });
            let client = CollaborationClient::connect(directory.path(), "correlation", "1")
                .await
                .unwrap();
            let result =
                tokio::time::timeout(std::time::Duration::from_secs(5), client.list_endpoints())
                    .await
                    .unwrap()
                    .unwrap_err();
            tokio::time::timeout(std::time::Duration::from_secs(5), fixture)
                .await
                .unwrap()
                .unwrap();
            if expected_match {
                assert!(
                    matches!(result, ClientError::Rejected {code:-32050, data:Some(data)} if data == expected_data)
                );
            } else {
                assert!(
                    matches!(
                        result,
                        ClientError::Protocol("API answer does not match the call")
                    ),
                    "uncorrelated {streaming} error became {result:?}"
                );
                assert!(!format!("{result:?}").contains("private-correlation-sentinel"));
            }
        }
    }
}
