//! Retained subscription journey through the typed SDK, native membership and local progress.
use collaboration_client::CollaborationClient;
use collaboration_protocol::{
    SubscriptionWaitBatch, ThreadSubscriptionPresence, ThreadSubscriptionState,
    ThreadSubscriptionWaitFilter, ThreadSubscriptionWaitOutcome, ThreadSubscriptionWaitRequest,
    ThreadSubscriptionWaitResult, ThreadSubscriptionsRequest,
};
use message_board::public_replies::{RequestDecision, RequestResult, WriteReply};
use message_board::*;
use serde_json::json;
use std::time::Duration;
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError};
fn request<TRequest: serde::de::DeserializeOwned>(
    actor: &Identity,
    fields: serde_json::Value,
) -> Result<TRequest, TestError> {
    let mut value = json!({"requestId":RequestId::generate(),"protocolVersion":1,"actor":actor});
    value
        .as_object_mut()
        .ok_or("header")?
        .extend(fields.as_object().ok_or("fields")?.clone());
    Ok(serde_json::from_value(value)?)
}
fn accepted(reply: &WriteReply) {
    assert!(
        matches!(
            reply.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "{reply:?}"
    );
}
async fn wait(
    client: &CollaborationClient,
    reader: &Identity,
    filter: ThreadSubscriptionWaitFilter,
    seconds: u64,
) -> Result<ThreadSubscriptionWaitResult, TestError> {
    Ok(client
        .thread_wait(
            ThreadSubscriptionWaitRequest {
                actor: reader.clone(),
                acting_for: None,
                protocol_version: ProtocolVersion::CURRENT,
                renewals: Vec::new(),
                filter,
                max_wait_seconds: seconds,
            },
            Duration::from_secs(5),
        )
        .await?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn subscription_methods_roundtrip_through_the_api_and_sqlite() -> Result<(), TestError> {
    let fixture = BoardFixture::start_poll().await?;
    let (project, workstream) = fixture.project().await?;
    let client = fixture.api.client("subscription-control-test").await?;
    let reader = Identity::Human {
        human_id: HumanId::try_from("human-reader".to_owned())?,
    };
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let before = store.snapshot().await?;
    let boundary = before.high_water();
    before.finish().await?;
    let project_scope = SubscriptionScope::project(project.clone());
    accepted(
        &client
            .thread_subscribe(request(
                &reader,
                json!({"scope":project_scope,"policy":{}}),
            )?)
            .await?,
    );
    let listed = client
        .thread_subscriptions(ThreadSubscriptionsRequest {
            actor: reader.clone(),
        })
        .await?;
    assert_eq!(listed.subscriptions.len(), 1);
    let subscribed = listed.subscriptions.first().ok_or("subscription")?.clone();
    assert_eq!(subscribed.scope, project_scope);
    assert_eq!(subscribed.state, ThreadSubscriptionState::Active);
    assert_eq!(subscribed.policy.mode(), SubscriptionMode::Off);
    assert_eq!(
        subscribed.presence,
        ThreadSubscriptionPresence::Unreachable {
            reason: "no session target".to_owned()
        }
    );
    assert_eq!(
        client
            .thread_subscriptions(ThreadSubscriptionsRequest {
                actor: reader.clone()
            })
            .await?
            .subscriptions,
        vec![subscribed]
    );
    let non_poll = wait(
        &client,
        &reader,
        ThreadSubscriptionWaitFilter::Project {
            project_id: project.clone(),
        },
        0,
    )
    .await?;
    assert!(
        matches!(
            non_poll.outcome,
            ThreadSubscriptionWaitOutcome::Refused { .. }
        ),
        "wait must refuse off mode"
    );
    accepted(&client.thread_subscribe(request(&reader,json!({"scope":project_scope,"policy":{"mode":"poll","quietSeconds":0,"capSeconds":0}}))?).await?);
    let root_author = Identity::Human {
        human_id: HumanId::try_from("root-author".to_owned())?,
    };
    let post = PostId::generate();
    let thread = ThreadId::from(post.clone());
    let mut before_root = store.snapshot().await?;
    assert_eq!(
        before_root.project_watch(&reader).await?,
        Some(boundary),
        "policy changes retain the project-watch start"
    );
    let thread_watch_boundary = before_root.high_water();
    before_root.finish().await?;
    accepted(&client.thread_create(request(&root_author,json!({"postId":post,"projectId":project,"about":{"kind":"workstream","subject":workstream},"text":"Join policy","references":[],"addressees":[],"role":null,"watch":false}))?).await?);
    let shown = client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (thread.clone()).into(),
            reader: ReadActor::Identified {
                reader: reader.clone(),
            },
        })
        .await?;
    let watch = shown.watch_status.ok_or("first-root watch status")?;
    assert!(
        watch.watching,
        "empty project subscription must cover its first later root"
    );
    assert_eq!(
        watch.starts_after,
        Some(thread_watch_boundary),
        "the post materializes its thread watch at the pre-post feed head"
    );
    let project_subscription = client
        .thread_subscriptions(ThreadSubscriptionsRequest {
            actor: reader.clone(),
        })
        .await?
        .subscriptions
        .into_iter()
        .next()
        .ok_or("project subscription disappeared")?;
    assert!(
        project_subscription.pending_count > 0,
        "first root window must retain pending activity"
    );
    let first_root = wait(
        &client,
        &reader,
        ThreadSubscriptionWaitFilter::Roots {
            threads: vec![(thread.clone()).into()],
        },
        1,
    )
    .await?;
    let ThreadSubscriptionWaitOutcome::Batch {
        batch: SubscriptionWaitBatch::Ranges { roots, .. },
    } = first_root.outcome
    else {
        return Err("human reader did not receive due first-root ranges".into());
    };
    assert!(roots.iter().any(|root| root.thread_id == thread));
    assert!(!serde_json::to_string(&roots)?.contains("Join policy"));
    accepted(
        &client
            .thread_unsubscribe(request(&reader, json!({"scope":project_scope}))?)
            .await?,
    );
    let mut read = store.snapshot().await?;
    let cancelled = read
        .membership(&reader, &project_scope)
        .await?
        .ok_or("cancelled membership missing")?;
    assert_eq!(
        cancelled.ended().map(|end| end.reason),
        Some(EndReason::Cancelled)
    );
    assert_eq!(
        cancelled.state_at(chrono::Utc::now(), None, false),
        SubscriptionState::Ended {
            reason: EndReason::Cancelled
        }
    );
    read.finish().await?;
    assert!(
        client
            .thread_subscriptions(ThreadSubscriptionsRequest {
                actor: reader.clone()
            })
            .await?
            .subscriptions
            .is_empty()
    );

    let session_reader = Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
                endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
            },
            session_id: SessionId::try_from("00000000-0000-7000-8000-000000000003".to_owned())?,
        },
    };
    accepted(&client.thread_join(request(&session_reader,json!({"thread":{"thread":thread},"role":"participant","watch":true,"mode":"poll","whenIdle":"hold","replace":null,"note":null}))?).await?);
    let joined = client
        .thread_subscriptions(ThreadSubscriptionsRequest {
            actor: session_reader.clone(),
        })
        .await?
        .subscriptions
        .into_iter()
        .next()
        .ok_or("join did not create subscription")?;
    assert_eq!(joined.policy.mode(), SubscriptionMode::Poll);
    assert_eq!(joined.policy.when_idle(), WhenIdle::Hold);
    accepted(&client.thread_subscribe(request(&session_reader,json!({"scope":SubscriptionTarget::from(SubscriptionScope::thread(thread.clone())),"policy":{"mode":"poll","whenIdle":"hold","quietSeconds":0,"capSeconds":0}}))?).await?);
    assert!(matches!(
        wait(
            &client,
            &session_reader,
            ThreadSubscriptionWaitFilter::Roots {
                threads: vec![(thread.clone()).into()]
            },
            0
        )
        .await?
        .outcome,
        ThreadSubscriptionWaitOutcome::TimedOut {}
    ));
    let writer = Identity::Human {
        human_id: HumanId::try_from("thread-writer".to_owned())?,
    };
    accepted(&client.thread_join(request(&writer,json!({"thread":{"thread":thread},"role":"participant","watch":false,"mode":null,"whenIdle":null,"replace":null,"note":null}))?).await?);
    accepted(&client.thread_post(request(&writer,json!({"postId":PostId::generate(),"placement":{"kind":"thread","thread":{"thread":thread}},"text":"private reply body","references":[],"addressees":[]}))?).await?);
    let delivered = wait(
        &client,
        &session_reader,
        ThreadSubscriptionWaitFilter::Roots {
            threads: vec![(thread.clone()).into()],
        },
        1,
    )
    .await?;
    let ThreadSubscriptionWaitOutcome::Batch {
        batch: SubscriptionWaitBatch::Notice { roots, .. },
    } = delivered.outcome
    else {
        return Err("session reader did not receive due notice".into());
    };
    assert!(roots.iter().any(|root| root.thread_id == thread));
    assert!(!serde_json::to_string(&roots)?.contains("private reply body"));
    assert!(
        matches!(
            wait(
                &client,
                &session_reader,
                ThreadSubscriptionWaitFilter::Roots {
                    threads: vec![(thread).into()]
                },
                0
            )
            .await?
            .outcome,
            ThreadSubscriptionWaitOutcome::TimedOut {}
        ),
        "same activity was handed off twice"
    );
    accepted(
        &client
            .thread_unsubscribe(request(&session_reader, json!({"scope":SubscriptionTarget::from(joined.scope).qualified(joined.project_id)}))?)
            .await?,
    );
    assert!(
        client
            .thread_subscriptions(ThreadSubscriptionsRequest {
                actor: session_reader
            })
            .await?
            .subscriptions
            .is_empty()
    );
    fixture.stop().await
}
