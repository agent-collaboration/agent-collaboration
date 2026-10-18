use crate::{
    ConformanceClient, ConformanceResult, holder_fixtures::*, session_scenarios::SessionFixture,
};
use collaboration_replication_protocol::*;
use serde_json::{Value, json};

/// Raw bytes traverse both real socket receivers; no typed producer can repair
/// the bad ReadScope before it reaches the decode boundary.
pub async fn run_malformed_frame_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut failures = Vec::new();
    for (name, bytes, expected_detail) in [
        ("correlated body", None, None),
        ("undecodable JSON", Some(b"{".to_vec()), Some("undecodable")),
        (
            "missing request ID",
            Some(br#"{"kind":"request","body":{}}"#.to_vec()),
            Some("requestWithoutFrameId"),
        ),
        (
            "invalid request ID",
            Some(br#"{"kind":"request","frame_id":"wrong","body":{}}"#.to_vec()),
            Some("requestWithoutFrameId"),
        ),
    ] {
        let result = run_case(client, bytes, expected_detail).await;
        match result {
            Ok(()) => println!("MALFORMED {name}: passed"),
            Err(error) => {
                println!("MALFORMED {name}: FAILED {error}");
                failures.push(format!("{name}: {error}"));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; ").into())
    }
}

async fn run_case(
    client: &ConformanceClient,
    bytes: Option<Vec<u8>>,
    detail: Option<&str>,
) -> ConformanceResult<()> {
    let mut fixture = SessionFixture::create(client).await?;
    let baseline = fixture.copy().await?;
    let read = SessionRequest::Read {
        scope: ReadScope::ProjectCopy {
            project: fixture.project.clone(),
        },
        after: None,
    };
    let before = fixture.old_socket.request(read.clone()).await?;
    check(
        matches!(&before, SessionAnswer::ReadPage(ReadPage::ProjectCopy(ProjectCopyRead::Page(page))) if page.position == position(1, 1)? && page.rows.is_empty()),
        "literal initialized copy control before raw frame",
    )?;
    let proof = async {
        let frame_id = FrameId::new(uuid::Uuid::now_v7().to_string())?;
        let raw = bytes.unwrap_or(serde_json::to_vec(&json!({
            "kind": "request", "frame_id": frame_id,
            "body": {"kind":"read", "value": {"scope":{"kind":"projectCopy","value":{"project":fixture.project}},"after":null}}
        }))?);
        fixture.old_socket.send_raw(raw).await?;
        let actual = serde_json::to_value(fixture.old_socket.receive().await?)?;
        let expected: Value = match detail {
            Some(detail) => json!({"kind":"closed","value":{"reason":{"kind":"malformedFrame","value":{"detail":detail}}}}),
            None => json!({"kind":"answer","value":{"frame_id":frame_id,"body":{"kind":"refused","value":{"kind":"invalidShape","field":"body"}}}}),
        };
        equal(actual, expected)?;
        if detail.is_some() {
            check(fixture.old_socket.await_server_close(std::time::Duration::from_secs(3)).await?, "typed Closed must be followed by physical socket close")?;
        } else {
            // Exactly one answer: a duplicate refusal would prevent this
            // fresh frame ID from receiving its matching control response.
            equal(fixture.old_socket.request(read).await?, before)?;
        }
        equal(fixture.copy().await?, baseline)?;
        Ok(())
    }.await;
    // Malformed cases deliberately close the old socket; don't hide a proof
    // failure behind a cleanup error on that already closed transport.
    let _ = fixture.old_socket.close().await;
    fixture.new_socket.close().await?;
    proof
}
