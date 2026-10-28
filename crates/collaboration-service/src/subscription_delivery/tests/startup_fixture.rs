//! Native startup proof fixtures and actual completion observations.
use super::super::SubscriptionClock;
use super::owner_fixture::*;
use message_board::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::Notify;
use tracing_subscriber::Layer;

#[derive(Default)]
pub(in crate::subscription_delivery) struct StartupEvidence {
    pub timeout: AtomicBool,
    pub states: Mutex<Vec<String>>,
    changed: Notify,
}
impl StartupEvidence {
    pub async fn state(&self, expected: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let changed = self.changed.notified();
                if self
                    .states
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|state| state == expected)
                {
                    return;
                }
                changed.await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "missing startup transition {expected}; observed {:?}",
                self.states.lock().unwrap()
            )
        });
    }
}
pub(in crate::subscription_delivery) struct StartupTrace(pub Arc<StartupEvidence>);
impl<TSubscriber: tracing::Subscriber> Layer<TSubscriber> for StartupTrace {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _: tracing_subscriber::layer::Context<'_, TSubscriber>,
    ) {
        struct Fields<'event>(&'event StartupEvidence);
        impl tracing::field::Visit for Fields<'_> {
            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if field.name() == "startup_recovery" {
                    self.0.states.lock().unwrap().push(value.to_owned());
                    self.0.changed.notify_one();
                }
            }
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if field.name() == "error"
                    && format!("{value:?}").contains(&sqlx::Error::PoolTimedOut.to_string())
                {
                    self.0.timeout.store(true, Ordering::SeqCst);
                }
            }
        }
        event.record(&mut Fields(&self.0));
    }
}
pub(in crate::subscription_delivery) fn clock(fixture: &OwnerFixture) -> Arc<RuntimeTimerClock> {
    Arc::new(RuntimeTimerClock::new(fixture.clock.clone()))
}
pub(in crate::subscription_delivery) async fn activity(
    fixture: &OwnerFixture,
    runtime: &OwnerRuntime,
) -> collaboration_protocol::PushActivitySnapshot {
    let request = tokio::time::timeout(Duration::from_secs(5), async {
        runtime.requests.lock().await.recv().await.unwrap()
    })
    .await
    .expect("autonomous startup delivery");
    fixture
        .interactions
        .get_push_record(&request.payload.push_id)
        .await
        .unwrap()
        .unwrap()
        .activity
        .unwrap()
}
pub(in crate::subscription_delivery) fn expected_range(
    project: ProjectId,
    thread: ThreadId,
    cursor: FeedCursor,
) -> collaboration_protocol::PushActivityRange {
    collaboration_protocol::PushActivityRange {
        project_id: project,
        thread_id: thread,
        from: cursor,
        through: cursor,
    }
}

// Retain each real native reader until another acquisition actually times out.
// The caller resets timeout evidence afterward so only its own operation can satisfy it.
pub(in crate::subscription_delivery) async fn exhaust_project_readers(
    project: &collaboration_runtime::NativeProjectStore,
) -> Vec<message_board_storage::ProjectSnapshot> {
    let mut snapshots = Vec::new();
    loop {
        match project.snapshot().await {
            Ok(snapshot) => snapshots.push(snapshot),
            Err(message_board_storage::ProjectStorageError::Database(
                sqlx::Error::PoolTimedOut,
            )) => {
                assert!(
                    !snapshots.is_empty(),
                    "native reader pool had no available snapshot"
                );
                return snapshots;
            }
            Err(error) => panic!("native reader-pool exhaustion failed: {error}"),
        }
    }
}

pub(in crate::subscription_delivery) async fn release_project_readers(
    snapshots: Vec<message_board_storage::ProjectSnapshot>,
) {
    for snapshot in snapshots {
        snapshot.finish().await.unwrap();
    }
}

pub(in crate::subscription_delivery) async fn subscribed_project(
    fixture: &OwnerFixture,
    reader: Identity,
) -> (ProjectId, ThreadId, Identity, FeedCursor) {
    let original = fixture
        .registry
        .existing_project(&fixture.project)
        .await
        .unwrap()
        .unwrap();
    let mut read = original.snapshot().await.unwrap();
    let project_record = read
        .record(&RecordRef {
            kind: RecordKind::Project,
            id: RecordId::try_from(fixture.project.as_str().to_owned()).unwrap(),
        })
        .await
        .unwrap()
        .unwrap();
    let board = project_record.envelope().scope.board.clone();
    read.finish().await.unwrap();
    let project = ProjectId::generate();
    let request = serde_json::from_value(serde_json::json!({
        "requestId":RequestId::generate(),"protocolVersion":1,"actingFor":null,
        "actor":human("board-owner"),"boardId":board,"projectId":project,"title":"Healthy startup project"
    })).unwrap();
    Box::pin(fixture.prepare_additional_project(&request)).await;
    let reply = fixture.admission.create_project(&request).await.unwrap();
    assert!(
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{reply:?}"
    );
    let thread = ThreadId::generate();
    let write = async |actor: Identity, operation: BoardOperation| {
        let request = ProjectRequest {
            request_id: RequestId::generate(),
            project_id: project.clone(),
            actor,
            acting_for: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation,
        };
        let reply = fixture
            .admission
            .submit_at(&request, fixture.clock.now())
            .await
            .unwrap();
        assert!(
            matches!(
                reply.result,
                RequestResult::Decided(RequestDecision::Accepted { .. })
            ),
            "{reply:?}"
        );
        reply
    };
    write(
        human("healthy-root-author"),
        BoardOperation::ThreadCreate {
            post_id: PostId::try_from(thread.as_str().to_owned()).unwrap(),
            about: crate::test_subject_workstream::create(&fixture.registry, &project)
                .await
                .unwrap(),
            text: PostText::try_from("Healthy root".to_owned()).unwrap(),
            references: Vec::new(),
            participation: Some(ParticipantRole::Orchestrator),
            watch: false,
            addressees: Vec::new(),
        },
    )
    .await;
    write(
        reader.clone(),
        BoardOperation::ThreadJoin {
            thread_id: thread.clone(),
            role: ParticipantRole::Participant,
            watch: true,
            mode: None,
            when_idle: None,
            replace: None,
            note: None,
        },
    )
    .await;
    write(
        reader.clone(),
        BoardOperation::ThreadSubscribe {
            scope: SubscriptionScope::thread(thread.clone()),
            policy: SubscriptionPolicyPatch {
                mode: Some(SubscriptionMode::Deliver),
                when_idle: Some(WhenIdle::Hold),
                timing: SubscriptionTimingPatch {
                    quiet_seconds: Some(0),
                    cap_seconds: Some(0),
                },
                lifetime_seconds: None,
            },
        },
    )
    .await;
    // Establish local activation progress before the post, just as the fixture's first project does.
    fixture
        .store
        .lock()
        .await
        .rescan_missing_subscription_windows(&reader, fixture.clock.now())
        .await
        .unwrap();
    let posted = write(
        human("healthy-post-author"),
        BoardOperation::ThreadPost {
            post_id: PostId::generate(),
            thread_id: thread.clone(),
            text: PostText::try_from("Healthy owed range".to_owned()).unwrap(),
            references: Vec::new(),
            addressees: Vec::new(),
        },
    )
    .await;
    let RequestResult::Decided(RequestDecision::Accepted { effects, .. }) = posted.result else {
        panic!("accepted")
    };
    let Some(RecordPlacement::Project(cursor)) =
        effects.entries().first().map(|(_, placement)| placement)
    else {
        panic!("post position")
    };
    (project, thread, reader, cursor.cursor)
}

pub(in crate::subscription_delivery) async fn healthy_project(
    fixture: &OwnerFixture,
) -> (ProjectId, ThreadId, Identity, FeedCursor) {
    let Identity::Session { mut session } = fixture.reader.clone() else {
        panic!("session reader")
    };
    session.session_id = SessionId::try_from("healthy-startup-reader".to_owned()).unwrap();
    let reader = Identity::Session { session };
    Box::pin(subscribed_project(fixture, reader)).await
}
