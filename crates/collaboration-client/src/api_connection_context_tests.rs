//! Capture the real Unix HTTP request; no request argument supplies the call context.
use super::{ApiConnection, ToolAnswer};
use collaboration_protocol::{SessionRef, UNIX_SESSION_CONTEXT_HEADER, UnixSessionContext};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixListener;

fn session(session_id: &str) -> SessionRef {
    serde_json::from_value(json!({
        "endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"},
        "sessionId":session_id
    }))
    .unwrap()
}

async fn capture_request(listener: UnixListener) -> (Option<UnixSessionContext>, Value) {
    let (stream, _) = listener.accept().await.unwrap();
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).await.unwrap();
    assert_eq!(line, "POST /mcp HTTP/1.1\r\n");
    let mut length = None;
    let mut context = None;
    loop {
        line.clear();
        assert_ne!(reader.read_line(&mut line).await.unwrap(), 0);
        if line == "\r\n" {
            break;
        }
        let (name, value) = line.trim_end().split_once(':').unwrap();
        if name.eq_ignore_ascii_case("content-length") {
            length = Some(value.trim().parse::<usize>().unwrap());
        }
        if name.eq_ignore_ascii_case(UNIX_SESSION_CONTEXT_HEADER) {
            context = Some(serde_json::from_str(value.trim()).unwrap());
        }
    }
    let mut body = vec![0; length.unwrap()];
    reader.read_exact(&mut body).await.unwrap();
    let request: Value = serde_json::from_slice(&body).unwrap();
    let response = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"result":{
        "content":[],"structuredContent":{"fixture":"accepted"},"isError":false
    }}))
    .unwrap();
    let headers = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.len()
    );
    reader
        .get_mut()
        .write_all(headers.as_bytes())
        .await
        .unwrap();
    reader.get_mut().write_all(&response).await.unwrap();
    reader.get_mut().shutdown().await.unwrap();
    (context, request)
}

#[tokio::test]
async fn unix_session_context_sends_current_session_independently_of_the_declared_sender() {
    let current = session("current-S");
    let declared_sender = session("declared-T");
    for supplied in [Some(current.clone()), None] {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("api.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let capture = tokio::spawn(capture_request(listener));
        let mut connection = ApiConnection::new(path.to_str().unwrap());
        if let Some(session) = &supplied {
            connection = connection.with_session_context(session.clone()).unwrap();
        }
        let answer = connection
            .exchange("fixture_context", json!({"sender":declared_sender}), None)
            .await
            .unwrap();
        assert!(
            matches!(answer, ToolAnswer::Success(value) if value == json!({"fixture":"accepted"}))
        );
        let (context, request) = capture.await.unwrap();
        assert_eq!(
            context,
            supplied.map(|session| UnixSessionContext { session })
        );
        assert_eq!(
            request["params"]["arguments"]["sender"],
            json!(declared_sender)
        );
    }
}
