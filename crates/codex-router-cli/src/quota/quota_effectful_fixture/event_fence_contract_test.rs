//! The private completion fence observes journal flushes without creating product events.
use super::*;

#[test]
fn await_event_request_accepts_existing_kind_and_rejects_unknown_kind() {
    // Arrange: the same strict request envelope used by the existing private socket.
    let request = json!({"operation":"await_event","requestId":1,"after_ordinal":7,"kind":"grouping-invocation-settled"});
    // Act: admit the named existing event kind at the actual producer boundary.
    let admitted = serde_json::from_value::<configuration::FixtureControlRequest>(request.clone());
    // Assert: missing operation support is a real contract failure, not a discovery failure.
    assert!(admitted.is_ok(), "await_event was rejected: {admitted:?}");
    let mut unknown = request;
    unknown["kind"] = json!("invented-event");
    assert!(serde_json::from_value::<configuration::FixtureControlRequest>(unknown).is_err());
}

use super::super::event_fence::{FixtureEventFenceResult, FixtureEventKind};
use std::sync::mpsc;

struct HeldFlush {
    file: std::fs::File,
    entered: Option<tokio::sync::oneshot::Sender<()>>,
    release: mpsc::Receiver<()>,
}
impl std::io::Write for HeldFlush {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.file.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.entered
            .take()
            .ok_or_else(|| std::io::Error::other("held flush entered more than once"))?
            .send(())
            .map_err(|()| std::io::Error::other("held flush entry receiver closed"))?;
        self.release.recv().map_err(std::io::Error::other)?;
        self.file.flush()
    }
}

#[tokio::test]
async fn event_fence_waits_for_the_real_writer_flush() {
    // Arrange: keep the real journal file, holding only its flush completion.
    let output = tempfile::tempdir().unwrap();
    let (entered, blocked) = tokio::sync::oneshot::channel();
    let (release, released) = mpsc::channel();
    let journal = FixtureEvents::with_writer(HeldFlush {
        file: absent_file(&output.path().join("journal")).unwrap(),
        entered: Some(entered),
        release: released,
    });
    let writer = journal.clone();
    let write = tokio::task::spawn_blocking(move || {
        writer.write(json!({"kind":"grouping-invocation-settled"}))
    });
    blocked.await.unwrap();
    let mut fence =
        std::pin::pin!(journal.await_event(0, FixtureEventKind::GroupingInvocationSettled));
    // Act / Assert: a written but unflushed event cannot satisfy the fence.
    assert!(futures_util::poll!(fence.as_mut()).is_pending());
    release.send(()).unwrap();
    write.await.unwrap().unwrap();
    assert_eq!(
        fence.await,
        FixtureEventFenceResult::Observed {
            ordinal: 1,
            event_kind: FixtureEventKind::GroupingInvocationSettled,
        }
    );
}

#[tokio::test]
async fn event_fence_requires_a_newer_ordinal_of_the_requested_kind() {
    let (output, journal) = events();
    journal
        .write(json!({"kind":"grouping-invocation-started"}))
        .unwrap();
    let mut fence =
        std::pin::pin!(journal.await_event(1, FixtureEventKind::GroupingInvocationStarted));
    assert!(futures_util::poll!(fence.as_mut()).is_pending());
    journal
        .write(json!({"kind":"grouping-invocation-settled"}))
        .unwrap();
    assert!(futures_util::poll!(fence.as_mut()).is_pending());
    journal
        .write(json!({"kind":"grouping-invocation-started"}))
        .unwrap();
    assert_eq!(
        fence.await,
        FixtureEventFenceResult::Observed {
            ordinal: 3,
            event_kind: FixtureEventKind::GroupingInvocationStarted,
        }
    );
    assert_eq!(
        std::fs::read_to_string(output.path().join("events.jsonl"))
            .unwrap()
            .lines()
            .count(),
        3
    );
}

#[tokio::test]
async fn event_fence_ends_without_inventing_an_event_and_keeps_already_flushed_facts() {
    let (output, journal) = events();
    let mut pending =
        std::pin::pin!(journal.await_event(0, FixtureEventKind::GroupingInvocationStarted));
    assert!(futures_util::poll!(pending.as_mut()).is_pending());
    journal
        .write(json!({"kind":"grouping-invocation-settled"}))
        .unwrap();
    journal.finish();
    assert_eq!(pending.await, FixtureEventFenceResult::EndedBeforeEvent);
    assert_eq!(
        journal
            .await_event(0, FixtureEventKind::GroupingInvocationSettled)
            .await,
        FixtureEventFenceResult::Observed {
            ordinal: 1,
            event_kind: FixtureEventKind::GroupingInvocationSettled
        }
    );
    assert_eq!(
        std::fs::read_to_string(output.path().join("events.jsonl"))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[tokio::test]
async fn private_socket_fence_replies_are_read_only_and_typed() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let (output, journal) = events();
    let state = owned_state::OwnedQuotaState::create(Scenario::CapturedActions)
        .await
        .unwrap();
    let socket = output.path().join("control.sock");
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = FixtureControlServer {
        listener: tokio::net::UnixListener::bind(&socket).unwrap(),
        config: FixtureConfiguration {
            scenario: Scenario::CapturedActions,
            output_directory: output.path().to_owned(),
            events: output.path().join("events.jsonl"),
            receipt: output.path().join("receipt.json"),
            control_socket: socket.clone(),
            dispatch_receipt: output.path().join("dispatch.jsonl"),
            login_receipt: None,
            login_gate_socket: None,
            login_standin: None,
        },
        controller: QuotaFixtureController::default(),
        root: state.root().to_owned(),
        target: state.target.clone(),
        accounts: AccountEffects::default(),
        refresh: None,
        events: journal.clone(),
        mutations: Arc::new(Mutex::new(Vec::new())),
    };
    let serving = tokio::spawn(server.serve(stopped));
    let (reader, mut writer) = tokio::net::UnixStream::connect(socket)
        .await
        .unwrap()
        .into_split();
    let mut reader = BufReader::new(reader);
    writer.write_all(b"{\"operation\":\"await_event\",\"requestId\":1,\"after_ordinal\":0,\"kind\":\"invented-event\"}\n").await.unwrap();
    let mut line = String::new();
    assert!(reader.read_line(&mut line).await.unwrap() > 0);
    let refusal: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(refusal["kind"], "fixture-control-error");
    assert_eq!(refusal["requestId"], 1);
    assert!(
        refusal["message"]
            .as_str()
            .unwrap()
            .contains("unknown variant")
    );
    assert!(
        std::fs::read(output.path().join("events.jsonl"))
            .unwrap()
            .is_empty()
    );
    journal
        .write(json!({"kind":"grouping-invocation-settled"}))
        .unwrap();
    writer.write_all(b"{\"operation\":\"await_event\",\"requestId\":2,\"after_ordinal\":0,\"kind\":\"grouping-invocation-settled\"}\n").await.unwrap();
    line.clear();
    assert!(reader.read_line(&mut line).await.unwrap() > 0);
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({"kind":"fixture-event-fence","requestId":2,"result":{"kind":"observed","ordinal":1,"eventKind":"grouping-invocation-settled"}})
    );
    journal.finish();
    writer.write_all(b"{\"operation\":\"await_event\",\"requestId\":3,\"after_ordinal\":1,\"kind\":\"grouping-invocation-started\"}\n").await.unwrap();
    line.clear();
    assert!(reader.read_line(&mut line).await.unwrap() > 0);
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap(),
        json!({"kind":"fixture-event-fence","requestId":3,"result":{"kind":"ended-before-event"}})
    );
    stop.send(()).unwrap();
    serving.await.unwrap().unwrap();
    let records = std::fs::read_to_string(output.path().join("events.jsonl")).unwrap();
    assert_eq!(records.lines().count(), 1);
    assert_eq!(
        serde_json::from_str::<Value>(records.lines().next().unwrap()).unwrap(),
        json!({"sequence":1,"event":{"kind":"grouping-invocation-settled"}})
    );
}

#[tokio::test]
async fn published_refresh_kind_uses_the_existing_closed_post_flush_fence() {
    let kind: super::super::event_fence::FixtureEventKind =
        serde_json::from_value(serde_json::json!("refresh-result-published")).unwrap();
    let (_directory, events) = super::events();
    events.write(serde_json::json!({"kind":"refresh-result-published","mount":1,"publicationFrame":2,"result":{"kind":"failed","failed":[],"overall":"setup failed"}})).unwrap();
    assert_eq!(
        events.await_event(0, kind).await,
        super::super::event_fence::FixtureEventFenceResult::Observed {
            ordinal: 1,
            event_kind: kind
        }
    );
}
