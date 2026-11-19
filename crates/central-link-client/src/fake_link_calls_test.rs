use super::*;
use crate::flush_marks::FLUSH_INTERVAL_MILLIS;
use router_clock::ManualClock;
use serde_json::json;
use std::time::Duration;
use workspace_model::{WorkspaceReply, WorkspaceResult, central_mail::CentralMailResult};

const MARK: i64 = 1_763_380_800_000; // on a minute mark

fn uuid_v7(seed: u8) -> String {
    uuid::Builder::from_unix_timestamp_millis(1_763_380_000_000 + u64::from(seed), &[seed; 10])
        .into_uuid()
        .to_string()
}

fn session_caller(seed: u8) -> Caller {
    serde_json::from_value(json!({
        "kind": "session",
        "value": {"machineId": uuid_v7(200), "sessionId": format!("session-{seed}")},
    }))
    .unwrap()
}

fn status_query(request: &str) -> central_link_protocol::RequestStatusQuery {
    central_link_protocol::RequestStatusQuery {
        request_id: request_id(request),
        scope: central_link_protocol::RequestScope::Workspace(uuid_v7(201).parse().unwrap()),
    }
}

fn thread_post(request: &str, body: &str) -> RequestPayload {
    serde_json::from_value(json!({
        "kind": "workspace",
        "request": {
            "requestId": request,
            "workspaceId": uuid_v7(201),
            "operation": {"kind": "threadPost", "threadId": uuid_v7(202), "body": body, "addressees": []},
        },
    }))
    .unwrap()
}

fn create_thread(request: &str) -> RequestPayload {
    serde_json::from_value(json!({
        "kind": "workspace",
        "request": {
            "requestId": request,
            "workspaceId": uuid_v7(201),
            "operation": {"kind": "createThread", "areaId": uuid_v7(203), "title": "a thread", "body": "opening", "addressees": []},
        },
    }))
    .unwrap()
}

fn list_areas() -> RequestPayload {
    serde_json::from_value(json!({
        "kind": "workspace",
        "request": {
            "requestId": null,
            "workspaceId": uuid_v7(201),
            "operation": {"kind": "list", "query": {"kind": "areas", "includeArchived": false}, "cursor": {"order": "oldestFirst", "position": null}, "limit": 5},
        },
    }))
    .unwrap()
}

fn conflict_reply() -> ReplyPayload {
    ReplyPayload::Workspace(WorkspaceReply {
        result: WorkspaceResult::Conflict,
        actors: Vec::new(),
    })
}

fn fixture(start_millis: i64) -> (Arc<ManualClock>, FakeLinkCalls) {
    let clock =
        Arc::new(ManualClock::starting_at(SyncTime::from_unix_millis(start_millis)).unwrap());
    let responder: FakeResponder = Arc::new(|_, payload| match payload {
        RequestPayload::Workspace(_) => conflict_reply(),
        RequestPayload::Mail(_) => ReplyPayload::Mail(CentralMailResult::Conflict),
    });
    let calls = FakeLinkCalls::new(clock.clone(), responder);
    (clock, calls)
}

fn request_id(text: &str) -> RequestId {
    text.parse().unwrap()
}

#[tokio::test]
async fn a_thread_post_is_in_the_outbox_until_its_mark_then_answered() {
    let (clock, calls) = fixture(MARK - 20_000);
    let id = uuid_v7(1);
    let caller = session_caller(1);

    let submission = calls
        .submit(caller.clone(), thread_post(&id, "hello"))
        .await;

    let Submission::InOutbox(receipt) = submission else {
        panic!("thread posts go to the outbox: {submission:?}");
    };
    assert_eq!(receipt.id, request_id(&id));
    assert_eq!(receipt.flush_at, SyncTime::from_unix_millis(MARK));
    assert_eq!(
        calls
            .request_status(caller.clone(), status_query(&id))
            .await,
        RequestStatus::InOutbox {
            flush_at: SyncTime::from_unix_millis(MARK)
        }
    );
    assert_eq!(calls.flush_on_mark(), 0, "nothing is sent before its mark");

    clock
        .advance(Duration::from_millis(20_000 + 2_000))
        .unwrap();
    assert_eq!(calls.flush_on_mark(), 1);
    assert_eq!(
        calls.request_status(caller, status_query(&id)).await,
        RequestStatus::Answered(conflict_reply())
    );
}

#[tokio::test]
async fn another_callers_request_id_answers_not_found() {
    let (_clock, calls) = fixture(MARK - 20_000);
    let id = uuid_v7(2);
    calls
        .submit(session_caller(1), thread_post(&id, "mine"))
        .await;

    let status = calls
        .request_status(session_caller(2), status_query(&id))
        .await;

    assert_eq!(status, RequestStatus::NotFound);
}

#[tokio::test]
async fn an_immediate_write_whose_answer_is_lost_is_uncertain_and_shows_sent() {
    let (_clock, calls) = fixture(MARK - 20_000);
    calls.set_central(FakeCentral::AnswerLost);
    let id = uuid_v7(3);

    let expected_scope =
        central_link_protocol::RequestScope::Workspace(uuid_v7(201).parse().unwrap());

    let submission = calls.submit(session_caller(1), create_thread(&id)).await;

    assert!(
        matches!(&submission, Submission::Uncertain { request, scope } if *request == request_id(&id) && *scope == expected_scope),
        "{submission:?}"
    );
    assert_eq!(
        calls
            .request_status(session_caller(1), status_query(&id))
            .await,
        RequestStatus::Sent
    );
}

#[tokio::test]
async fn immediate_kinds_are_answered_or_refused_at_once_never_queued() {
    let (_clock, calls) = fixture(MARK - 20_000);

    let answered = calls.submit(session_caller(1), list_areas()).await;
    calls.set_central(FakeCentral::Unavailable);
    let refused = calls
        .submit(session_caller(1), create_thread(&uuid_v7(4)))
        .await;

    assert!(matches!(answered, Submission::Answered(_)), "{answered:?}");
    assert!(
        matches!(
            refused,
            Submission::Refused(SubmitRefusal::CentralUnavailable)
        ),
        "{refused:?}"
    );
}

#[tokio::test]
async fn outbox_writes_wait_out_an_unavailable_central_and_send_after() {
    let (clock, calls) = fixture(MARK - 20_000);
    calls.set_central(FakeCentral::Unavailable);
    let id = uuid_v7(5);
    calls
        .submit(session_caller(1), thread_post(&id, "later"))
        .await;

    clock.advance(Duration::from_millis(20_000)).unwrap();
    assert_eq!(calls.flush_on_mark(), 0);
    calls.set_central(FakeCentral::Answering);
    clock
        .advance(Duration::from_millis(FLUSH_INTERVAL_MILLIS.unsigned_abs()))
        .unwrap();

    assert_eq!(calls.flush_on_mark(), 1);
    assert_eq!(
        calls
            .request_status(session_caller(1), status_query(&id))
            .await,
        RequestStatus::Answered(conflict_reply())
    );
}

#[tokio::test]
async fn resubmitting_the_same_write_returns_its_receipt_and_new_content_is_refused() {
    let (_clock, calls) = fixture(MARK - 20_000);
    let id = uuid_v7(6);
    let first = calls
        .submit(session_caller(1), thread_post(&id, "same"))
        .await;

    let again = calls
        .submit(session_caller(1), thread_post(&id, "same"))
        .await;
    let changed = calls
        .submit(session_caller(1), thread_post(&id, "different"))
        .await;

    let (Submission::InOutbox(first), Submission::InOutbox(again)) = (first, again) else {
        panic!("both submissions are in the outbox");
    };
    assert_eq!(first, again);
    assert!(
        matches!(
            changed,
            Submission::Refused(SubmitRefusal::RequestIdConflict)
        ),
        "{changed:?}"
    );
}

#[tokio::test]
async fn an_unenrolled_router_refuses_every_kind() {
    let (_clock, calls) = fixture(MARK - 20_000);
    calls.set_central(FakeCentral::NotEnrolled);

    let post = calls
        .submit(session_caller(1), thread_post(&uuid_v7(7), "x"))
        .await;
    let read = calls.submit(session_caller(1), list_areas()).await;

    assert!(matches!(
        post,
        Submission::Refused(SubmitRefusal::NotEnrolled)
    ));
    assert!(matches!(
        read,
        Submission::Refused(SubmitRefusal::NotEnrolled)
    ));
}

#[tokio::test]
async fn a_flush_sends_at_most_64_and_the_rest_wait_for_the_next_mark() {
    let (clock, calls) = fixture(MARK - 20_000);
    let mut ids = Vec::new();
    for seed in 0..70_u8 {
        let id = uuid_v7(seed);
        calls
            .submit(session_caller(1), thread_post(&id, "bulk"))
            .await;
        ids.push(id);
    }

    clock.advance(Duration::from_millis(20_000)).unwrap();
    let first = calls.flush_on_mark();
    let repeat = calls.flush_on_mark();
    clock
        .advance(Duration::from_millis(FLUSH_INTERVAL_MILLIS.unsigned_abs()))
        .unwrap();
    let second = calls.flush_on_mark();

    assert_eq!((first, repeat, second), (64, 0, 6));
}

#[tokio::test]
async fn a_read_whose_answer_is_lost_is_read_answer_lost_and_records_nothing() {
    let (_clock, calls) = fixture(MARK - 20_000);
    calls.set_central(FakeCentral::AnswerLost);

    let read = calls.submit(session_caller(1), list_areas()).await;
    calls.set_central(FakeCentral::Answering);
    let again = calls.submit(session_caller(1), list_areas()).await;

    assert!(matches!(read, Submission::ReadAnswerLost), "{read:?}");
    assert!(matches!(again, Submission::Answered(_)), "{again:?}");
}
