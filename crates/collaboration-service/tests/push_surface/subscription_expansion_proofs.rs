#![expect(
    clippy::indexing_slicing,
    reason = "Test-only JSON fixture construction; behavior assertions remain strict."
)]
//! Control router/show expands a stored subscription range without acknowledging it.
use super::*;
use chrono::Utc;
use collaboration_service::{
    MachineIdentity, ProjectAdmission, ProjectAdmissionProps, ProjectStoreRegistry,
};
use message_board::{
    FeedCursor, Identity, InboxActivity, Post, PostId, ProjectId, ServiceId, SessionEndpointRef,
    SessionId as BoardSessionId, SessionRef as BoardSessionRef, ThreadId,
};
struct SubscriptionExpansionFixture {
    _fleet_runtime: board_central_test_support::FleetTestRuntime,
    _directory: tempfile::TempDir,
    registry: Arc<ProjectStoreRegistry>,
    pushes: Arc<interaction_storage::InteractionStore>,
    control: ControlHarness,
    project_id: ProjectId,
    thread_id: ThreadId,
    target: SessionRef,
    board_reader: Identity,
    initialization: message_board::RequestId,
}
fn header(actor: &Identity) -> Value {
    json!({"requestId":message_board::RequestId::generate(),"protocolVersion":1,"actor":actor,"actingFor":null})
}
impl SubscriptionExpansionFixture {
    async fn start() -> Self {
        let directory = tempfile::tempdir().expect("native databases");
        let registry = board_fixture::new_registry(directory.path()).expect("fixture enrollment");
        let origin = message_board::ActorOrigin {
            installation: message_board::ServiceId::try_from(SERVICE_ID.to_owned())
                .expect("fixture service"),
            machine: message_board::MachineLabel::try_from("range proof".to_owned())
                .expect("fixture machine"),
        };
        let (fleet, fleet_runtime) = board_central_test_support::fleet_client(
            directory.path(),
            Arc::new(board_fixture::PendingRegistration),
            &origin,
        )
        .await
        .expect("real central fixture");
        let fleet = Arc::new(fleet);
        let admission = Arc::new(
            ProjectAdmission::new(ProjectAdmissionProps {
                registry: registry.clone(),
                fleet: fleet.clone(),
                identity: MachineIdentity::new(
                    UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("service"),
                    Some("range proof"),
                )
                .expect("identity"),
            })
            .expect("admission"),
        );
        let pushes = Arc::new(
            interaction_storage::InteractionStore::open(
                directory.path().join("interaction.sqlite"),
            )
            .await
            .expect("native messages"),
        );
        let identity = ApplicationFixture::new(SERVICE_ID, SERVICE_ID)
            .expect("identity")
            .with_project_admission(admission)
            .with_interaction_store(pushes.clone());
        let control = ControlHarness::start(identity);
        let target = session("claude-local", "subscription-range-reader");
        let board_reader = board_session_identity(&target);
        let writer = Identity::Human {
            human_id: "subscription-range-writer"
                .to_owned()
                .try_into()
                .expect("writer"),
        };
        let workspace = message_board::WorkspaceId::generate();
        let board = message_board::BoardId::generate();
        let project_id = ProjectId::generate();
        let post = PostId::generate();
        let thread_id = ThreadId::from(post.clone());
        for (tool, fields) in [
            (
                "workspace_create",
                json!({"workspaceId":workspace,"name":"workspace","description":""}),
            ),
            (
                "board_create",
                json!({"workspaceId":workspace,"boardId":board,"name":"board","description":"","kind":{"kind":"area"}}),
            ),
            (
                "project_create",
                json!({"projectId":project_id,"boardId":board,"title":"project"}),
            ),
            (
                "role_grant",
                json!({"projectId":project_id,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project_id},"role":"lead","holder":writer,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}}),
            ),
            (
                "thread_create",
                json!({"postId":post,"projectId":project_id,"about":{"kind":"project","subject":{"line":"Subscription range"}},"text":"Root title outside activity range","references":[],"addressees":[],"role":null,"watch":false}),
            ),
        ] {
            let mut input = header(&writer);
            input
                .as_object_mut()
                .expect("header")
                .extend(fields.as_object().expect("fields").clone());
            if tool == "project_create" {
                board_fixture::prepare_registered_creation(
                    &fleet,
                    &registry,
                    &serde_json::from_value(input.clone()).expect("typed creation input"),
                    &origin,
                    &runtime_primitives::SystemClock,
                )
                .await
                .expect("registered project fixture");
            }
            let response = control
                .served
                .call(tool, input)
                .await
                .expect("native setup");
            assert_eq!(
                response.pointer("/result/result/decision/kind"),
                Some(&json!("accepted")),
                "{tool}: {response:?}"
            );
        }
        let mut join = header(&board_reader);
        join.as_object_mut().expect("header").remove("actingFor");
        join.as_object_mut().expect("header").extend(json!({"thread":{"thread":thread_id},"role":"participant","watch":true,"mode":"deliver","whenIdle":"hold","replace":null,"note":null}).as_object().expect("join").clone());
        assert_eq!(
            control
                .served
                .call("thread_join", join)
                .await
                .expect("join")
                .pointer("/result/result/decision/kind"),
            Some(&json!("accepted"))
        );
        Self {
            _fleet_runtime: fleet_runtime,
            _directory: directory,
            registry,
            pushes,
            control,
            project_id,
            thread_id,
            target,
            board_reader,
            initialization: message_board::RequestId::generate(),
        }
    }
    async fn close(self) {
        self.control.close().await;
        self._fleet_runtime
            .shutdown()
            .await
            .expect("stop real central fixture");
        Arc::try_unwrap(self.pushes)
            .ok()
            .expect("release native push owner")
            .close()
            .await
            .expect("close");
    }
    async fn post_thread_message(&self, body: &str) -> Post {
        let writer = Identity::Human {
            human_id: "subscription-range-writer"
                .to_owned()
                .try_into()
                .expect("writer"),
        };
        let post_id = PostId::generate();
        let mut input = header(&writer);
        input.as_object_mut().expect("header").extend(json!({"postId":post_id,"placement":{"kind":"thread","thread":{"thread":self.thread_id}},"text":body,"references":[],"addressees":[]}).as_object().expect("post").clone());
        assert_eq!(
            self.control
                .served
                .call("thread_post", input)
                .await
                .expect("post")
                .pointer("/result/result/decision/kind"),
            Some(&json!("accepted"))
        );
        let store = self
            .registry
            .existing_project(&self.project_id)
            .await
            .expect("registry")
            .expect("store");
        let mut read = store.snapshot().await.expect("snapshot");
        let record = read
            .record(&post_id.into())
            .await
            .expect("record")
            .expect("post");
        read.finish().await.expect("finish");
        match record {
            message_board::Record::Post(post) => Some(post),
            _ => None,
        }
        .expect("stored post")
    }
    async fn unread_project_activity(&self) -> Vec<InboxActivity> {
        let result=self.control.served.call("inbox_fetch",json!({"protocolVersion":1,"actingFor":null,"scope":{"kind":"project","projectId":self.project_id},"reader":self.board_reader,"readMode":"unread","initializations":[{"projectId":self.project_id,"requestId":self.initialization}],"page":{}})).await.expect("inbox");
        serde_json::from_value(result["result"]["page"]["records"].clone()).expect("activity")
    }
    async fn insert_subscription_push(
        &self,
        push_id: PushId,
        from: FeedCursor,
        through: FeedCursor,
        message_count: u64,
    ) {
        let origin = RouterOriginRef::SubscriptionActivity {
            target: self.target.clone(),
            batch_id: message_board::BatchId::generate(),
        };
        self.pushes
            .insert_push_record(PushRecordDraft {
                push_id,
                kind: PushKind::SubscriptionActivity,
                origin: PushOrigin::Router(PushKind::SubscriptionActivity),
                origin_router_ref: Some(origin.canonical_string().expect("origin")),
                target: self.target.clone(),
                reply_to_push_id: None,
                header_facts: PushHeaderFacts::SubscriptionActivity {
                    root_count: 1,
                    message_count,
                    held_since: None,
                    thread_resolved: false,
                },
                body: None,
                activity: Some(PushActivitySnapshot {
                    ranges: vec![PushActivityRange {
                        thread_id: self.thread_id.clone(),
                        project_id: self.project_id.clone(),
                        from,
                        through,
                    }],
                    held: false,
                    draining: false,
                }),
                mode: None,
                guard: None,
                created_at: Utc::now(),
            })
            .await
            .expect("push");
    }
}

fn board_session_identity(target: &SessionRef) -> Identity {
    Identity::Session {
        session: BoardSessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from(String::from(target.endpoint.service_id.clone()))
                    .expect("Board service id"),
                endpoint_id: message_board::EndpointId::try_from(String::from(
                    target.endpoint.endpoint_id.clone(),
                ))
                .expect("Board endpoint id"),
            },
            session_id: BoardSessionId::try_from(String::from(target.session_id.clone()))
                .expect("Board session id"),
        },
    }
}

#[tokio::test]
async fn router_show_expands_only_stored_subscription_range_without_read_acknowledgement() {
    let mut fixture = SubscriptionExpansionFixture::start().await;
    let initial_unread = fixture.unread_project_activity().await;
    assert!(
        initial_unread.is_empty(),
        "establish the inbox baseline first"
    );

    let before_range = fixture
        .post_thread_message("outside-before: do not return")
        .await;
    let first_in_range = fixture
        .post_thread_message("inside-first: return this body")
        .await;
    let native = fixture
        .registry
        .existing_project(&fixture.project_id)
        .await
        .expect("native project")
        .expect("store");
    let mut read = native.snapshot().await.expect("snapshot");
    let template = read
        .record(&fixture.project_id.clone().into())
        .await
        .expect("project read")
        .expect("project");
    read.finish().await.expect("finish");
    let unrelated = message_board::WorkstreamId::generate();
    let mut write = native
        .begin_admission(message_board::ActorOrigin {
            installation: SERVICE_ID.to_owned().try_into().expect("service"),
            machine: "range proof".to_owned().try_into().expect("machine"),
        })
        .await
        .expect("native write");
    let unrelated_position = write
        .append_record(message_board::Record::Workstream(
            message_board::Workstream {
                envelope: message_board::RecordEnvelope {
                    id: unrelated.clone().into(),
                    request: None,
                    ..template.envelope().clone()
                },
                workstream_id: unrelated,
                project_id: fixture.project_id.clone(),
                name: "unrelated in-range work"
                    .to_owned()
                    .try_into()
                    .expect("name"),
                goal: String::new(),
                state: message_board::WorkstreamState::Open {},
                revision: message_board::SubjectRevision::INITIAL,
            },
        ))
        .await
        .expect("append unrelated workstream");
    write.commit().await.expect("commit");
    let second_in_range = fixture
        .post_thread_message("inside-second: return this body")
        .await;
    let after_range = fixture
        .post_thread_message("outside-after: do not return")
        .await;
    use sqlx::{ConnectOptions, Connection};
    let mut observer = sqlx_turso::TursoConnectOptions::new()
        .filename(native.path())
        .connect()
        .await
        .expect("observer");
    let changed = sqlx::query("UPDATE project_feed SET content = json_set(content, '$.record.record.name', '') WHERE position = ?")
        .bind(i64::try_from(unrelated_position.position.get()).expect("position"))
        .execute(&mut observer).await.expect("corrupt unrelated body");
    assert_eq!(changed.rows_affected(), 1);
    observer.close().await.expect("close");
    let mut read = native.snapshot().await.expect("snapshot");
    let before_unrelated = message_board::FeedCursor {
        epoch: unrelated_position.epoch,
        position: message_board::FeedPosition::try_from(unrelated_position.position.get() - 1)
            .expect("before position"),
    };
    assert!(
        read.feed_after(before_unrelated, 1, false).await.is_err(),
        "direct content still rejects the unrelated corrupt body"
    );
    read.finish().await.expect("finish");

    let push_id = PushId::try_from(uuid::Uuid::now_v7().to_string()).expect("UUIDv7 push id");
    fixture
        .insert_subscription_push(
            push_id.clone(),
            first_in_range
                .position
                .project_cursor()
                .expect("first project cursor"),
            second_in_range
                .position
                .project_cursor()
                .expect("second project cursor"),
            2,
        )
        .await;
    let link = collaboration_protocol::RouterLink::new(
        collaboration_protocol::MachineId::from(
            UuidIdentity::try_from(SERVICE_ID.to_owned()).expect("machine UUID"),
        ),
        push_id.clone(),
    )
    .to_string();

    let shown = fixture
        .control
        .call(
            "router/show",
            json!({"caller":fixture.target.clone(),"reference":link.clone()}),
        )
        .await;
    assert!(shown.get("result").is_some(), "real Control show: {shown}");
    assert_eq!(shown["result"]["link"], link);
    assert_eq!(shown["result"]["record"]["pushId"], push_id.as_str());
    assert_eq!(shown["result"]["record"]["kind"], "subscription-activity");
    assert_eq!(
        shown["result"]["activityRanges"].as_array().map(Vec::len),
        Some(1)
    );

    let range_read = &shown["result"]["activityRanges"][0];
    assert_eq!(
        range_read["range"],
        json!({
            "threadId":fixture.thread_id.clone(),
            "projectId":fixture.project_id.clone(),
            "from":first_in_range.position.project_cursor().expect("first project cursor"),
            "through":second_in_range.position.project_cursor().expect("second project cursor")
        })
    );
    let expanded_messages = range_read["posts"]
        .as_array()
        .expect("Control returned expanded Board messages");
    assert_eq!(expanded_messages.len(), 2);
    assert_eq!(
        expanded_messages
            .iter()
            .map(|message| message["postId"].as_str().expect("message id"))
            .collect::<Vec<_>>(),
        vec![
            first_in_range.post_id.as_str(),
            second_in_range.post_id.as_str()
        ]
    );
    assert_eq!(
        expanded_messages[0]["text"],
        "inside-first: return this body"
    );
    assert_eq!(
        expanded_messages[1]["text"],
        "inside-second: return this body"
    );
    assert!(
        expanded_messages
            .iter()
            .all(|message| message["postId"] != before_range.post_id.as_str()
                && message["postId"] != after_range.post_id.as_str()),
        "the stored activity range excludes surrounding Board messages"
    );

    assert!(
        shown["result"]["record"]["readAt"].is_null(),
        "showing a subscription push does not acknowledge it"
    );
    let stored_after_show = fixture
        .pushes
        .get_push_record(&push_id)
        .await
        .expect("read stored subscription push")
        .expect("subscription push remains stored");
    assert!(stored_after_show.read_at.is_none());
    assert_eq!(stored_after_show.delivery_state, PushDeliveryState::Pending);

    let unread_after_show = fixture.unread_project_activity().await;
    assert_eq!(unread_after_show.len(), 4);
    let unread_message_ids = unread_after_show
        .iter()
        .filter_map(|activity| match activity {
            InboxActivity::PostCreated { post: message, .. } => {
                Some(message.post_id.as_str().to_owned())
            }
            InboxActivity::ThreadStateChanged { .. } => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        unread_message_ids,
        vec![
            before_range.post_id.as_str().to_owned(),
            first_in_range.post_id.as_str().to_owned(),
            second_in_range.post_id.as_str().to_owned(),
            after_range.post_id.as_str().to_owned()
        ],
        "range expansion reads the Board but leaves the target inbox unacknowledged"
    );

    fixture.close().await;
}
