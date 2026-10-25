use collaboration_client::board::{RequestDecision, RequestResult};
use collaboration_client::{BoardClientError, CollaborationClient};
use collaboration_protocol::{
    SubscriptionWaitBatch, ThreadSubscribeRequest, ThreadSubscriptionWaitFilter,
    ThreadSubscriptionWaitOutcome, ThreadSubscriptionWaitRequest, ThreadSubscriptionsRequest,
    ThreadUnsubscribeRequest,
};
use message_board::{Identity, ProtocolVersion, RequestId, ResourceIdentity, SubscriptionTarget};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::oneshot;

#[path = "support/scripted_api.rs"]
mod scripted_api;
use scripted_api::{ScriptedApi, ScriptedCalls};

type TestError = Box<dyn std::error::Error + Send + Sync>;
type TestResult = Result<(), TestError>;

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn actor() -> Value {
    json!({"kind":"human","humanId":"subscription-client"})
}

fn scope() -> Value {
    json!({"kind":"thread","thread":{"thread":"01900000-0000-7000-8000-000000000001"}})
}

#[allow(clippy::expect_used)] // Static test JSON construction.
fn mutation(fields: Value) -> Value {
    let mut value = json!({"requestId":RequestId::generate(),"protocolVersion":1,"actor":actor(),"actingFor":null});
    value
        .as_object_mut()
        .expect("fixture object")
        .extend(fields.as_object().expect("fixture fields").clone());
    value
}
fn write_reply() -> Value {
    json!({"result":{"kind":"decided","decision":{"kind":"accepted",
        "effects":{"entries":[],"totalCount":0,
            "digest":message_board::DecisionEffectsHeader::from_full(&[]).digest,"next":null},
        "change":"alreadySo"}},"reader":null})
}
fn view() -> Value {
    json!({"projectId":"01900000-0000-7000-8000-000000000002","scope":{"kind":"thread","threadId":"01900000-0000-7000-8000-000000000001"},"policy":{"mode":"poll","whenIdle":"hold",
        "timing":{"quietSeconds":120,"capSeconds":600},"lifetime":86400},
        "state":"active","expiresAt":"2026-10-02T00:00:00Z","pendingCount":2,
        "presence":{"kind":"unreachable","reason":"human reader"}})
}

#[tokio::test]
async fn subscription_methods_exchange_typed_requests_and_views() -> TestResult {
    let (_api, client, mut peer) = connected_client().await?;
    let server = tokio::spawn(async move {
        let subscribe = peer.next().await?;
        ensure(
            subscribe.tool == "thread_subscribe",
            "wrong subscribe method",
        )?;
        ensure(
            subscribe.arguments["actor"] == actor(),
            "subscribe actor changed",
        )?;
        ensure(
            subscribe.arguments["scope"] == scope(),
            "subscribe scope changed",
        )?;
        ensure(
            subscribe.arguments["policy"]["quietSeconds"] == 0,
            "policy units changed",
        )?;
        subscribe.succeed(write_reply());

        let list = peer.next().await?;
        ensure(
            list.tool == "thread_subscriptions",
            "wrong subscriptions method",
        )?;
        ensure(
            list.arguments == json!({"actor":actor()}),
            "list request changed",
        )?;
        list.succeed(json!({"subscriptions":[view()],"unavailable":["01900000-0000-7000-8000-000000000003"]}));

        let unsubscribe = peer.next().await?;
        ensure(
            unsubscribe.tool == "thread_unsubscribe",
            "wrong unsubscribe method",
        )?;
        ensure(
            unsubscribe.arguments["actor"] == actor() && unsubscribe.arguments["scope"] == scope(),
            "unsubscribe request changed",
        )?;
        unsubscribe.succeed(write_reply());
        Ok::<(), TestError>(())
    });

    let subscribed = client
        .thread_subscribe(serde_json::from_value::<ThreadSubscribeRequest>(mutation(
            json!({"scope":scope(),"policy":{"mode":"poll","quietSeconds":0}}),
        ))?)
        .await?;
    ensure(
        matches!(
            subscribed.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "subscribe write reply was not decoded",
    )?;
    let subscriptions = client
        .thread_subscriptions(ThreadSubscriptionsRequest {
            actor: serde_json::from_value::<Identity>(actor())?,
        })
        .await?;
    ensure(
        subscriptions.unavailable.len() == 1,
        "unavailable project omitted",
    )?;
    ensure(
        subscriptions.subscriptions.len() == 1,
        "subscription list was not decoded",
    )?;
    let ended = client
        .thread_unsubscribe(serde_json::from_value::<ThreadUnsubscribeRequest>(
            mutation(json!({"scope":scope()})),
        )?)
        .await?;
    ensure(
        matches!(
            ended.result,
            RequestResult::Decided(RequestDecision::Accepted { .. })
        ),
        "unsubscribe write reply was not decoded",
    )?;
    server.await??;
    Ok(())
}

#[tokio::test]
async fn invalid_mutation_results_preserve_thread_or_project_inspection_identity() -> TestResult {
    for subscription_scope in [
        scope(),
        json!({"kind":"project","projectId":"01900000-0000-7000-8000-000000000002"}),
    ] {
        let (_api, client, mut peer) = connected_client().await?;
        let server = tokio::spawn(async move {
            peer.next().await?.succeed(json!({"invalid":true}));
            Ok::<(), TestError>(())
        });
        let scope: SubscriptionTarget = serde_json::from_value(subscription_scope)?;
        let expected_resource = match &scope {
            SubscriptionTarget::Thread { thread } => ResourceIdentity::Thread {
                thread_id: thread.thread.clone(),
            },
            SubscriptionTarget::Project { project_id } => ResourceIdentity::Project {
                project_id: project_id.clone(),
            },
        };
        let outcome = client
            .thread_unsubscribe(ThreadUnsubscribeRequest {
                actor: serde_json::from_value(actor())?,
                scope,
                request_id: RequestId::generate(),
                protocol_version: ProtocolVersion::CURRENT,
                acting_for: None,
            })
            .await;
        match outcome {
            Err(BoardClientError::OutcomeUnknown { resource, .. }) => {
                ensure(
                    resource == expected_resource,
                    "mutation inspection identity changed",
                )?;
            }
            _ => return Err("invalid result did not classify uncertain mutation".into()),
        }
        server.await??;
    }
    Ok(())
}

#[tokio::test]
async fn subscription_wait_uses_the_rebound_method_and_decodes_notice_ranges() -> TestResult {
    let (_api, client, mut peer) = connected_client().await?;
    let root_id = "01900000-0000-7000-8000-000000000011";
    let project_id = "01900000-0000-7000-8000-000000000012";
    let request: ThreadSubscriptionWaitRequest = serde_json::from_value(json!({
        "actor": {
            "kind":"session",
            "session":{
                "endpoint":{
                    "serviceId":"00000000-0000-4000-8000-000000000001",
                    "endpointId":"codex-local"
                },
                "sessionId":"subscription-client"
            }
        },
        "filter":{"kind":"roots","threads":[{"thread":root_id}]},
        "protocolVersion":1,"actingFor":null,"renewals":[],"maxWaitSeconds":30
    }))?;
    let server = tokio::spawn(async move {
        let wait = peer.next().await?;
        ensure(wait.tool == "thread_wait", "wrong wait method")?;
        ensure(
            wait.arguments["filter"] == json!({"kind":"roots","threads":[{"thread":root_id}]}),
            "wait filter changed",
        )?;
        ensure(
            wait.arguments["maxWaitSeconds"] == 30,
            "wait duration changed",
        )?;
        wait.succeed(json!({
            "renewals":[],"outcome":{"kind":"batch","batch":{
                "kind":"notice",
                "pushId":"01900000-0000-7000-8000-000000000013",
                "line":"thread subscription notice",
                "held":false,
                "draining":false,
                "roots":[{
                    "threadId":root_id,
                    "projectId":project_id,
                    "from":{"epoch":1,"position":1},
                    "through":{"epoch":1,"position":2},
                    "postCount":2
                }]
            }}
        }));
        Ok::<(), TestError>(())
    });

    let result = client.thread_wait(request, Duration::from_secs(35)).await?;
    match result.outcome {
        ThreadSubscriptionWaitOutcome::Batch {
            batch: SubscriptionWaitBatch::Notice { roots, .. },
        } => {
            ensure(roots.len() == 1, "notice roots were not decoded")?;
        }
        ThreadSubscriptionWaitOutcome::Batch {
            batch: SubscriptionWaitBatch::Ranges { .. },
        } => {
            return Err("session notice was decoded as human ranges".into());
        }
        _ => return Err("wait response lost its notice batch".into()),
    }
    server.await??;
    Ok(())
}

#[tokio::test]
async fn subscription_wait_timeout_is_unknown_and_never_replayed() -> TestResult {
    let (_api, client, mut peer) = connected_client().await?;
    let request: ThreadSubscriptionWaitRequest = serde_json::from_value(json!({
        "actor":actor(),
        "filter":{"kind":"all"},
        "protocolVersion":1,"actingFor":null,"renewals":[],"maxWaitSeconds":1
    }))?;
    let expected_actor = request.actor.clone();
    let expected_filter = request.filter.clone();
    let (observed_request_sender, observed_request_receiver) = oneshot::channel();
    let (release_server_sender, release_server_receiver) = oneshot::channel();
    let server = tokio::spawn(async move {
        let request = peer.next().await?;
        observed_request_sender
            .send(())
            .map_err(|_| "wait request observer dropped")?;
        release_server_receiver
            .await
            .map_err(|_| "wait server release dropped")?;
        drop(request);
        // The timed-out wait is not resubmitted.
        ensure(
            peer.none_within(Duration::from_millis(200)).await,
            "timed-out wait was replayed",
        )
    });

    let timeout = client
        .thread_wait(request.clone(), Duration::from_millis(100))
        .await;
    match timeout {
        Err(BoardClientError::WaitOutcomeUnknown {
            actor,
            filter,
            renewals,
        }) => {
            ensure(renewals.is_empty(), "unexpected uncertain renewal")?;
            ensure(*actor == expected_actor, "timeout lost the Reader identity")?;
            ensure(filter == expected_filter, "timeout lost the wait filter")?;
        }
        _ => return Err("wait timeout was not preserved".into()),
    }
    observed_request_receiver
        .await
        .map_err(|_| "server did not observe the wait request")?;
    release_server_sender
        .send(())
        .map_err(|_| "wait server ended before release")?;
    server.await??;
    Ok(())
}

#[tokio::test]
async fn subscription_wait_rejects_invalid_request_before_transmission() -> TestResult {
    let (_api, client, mut peer) = connected_client().await?;
    let request = ThreadSubscriptionWaitRequest {
        actor: serde_json::from_value(actor())?,
        filter: ThreadSubscriptionWaitFilter::All {},
        max_wait_seconds: 1501,
        acting_for: None,
        protocol_version: ProtocolVersion::CURRENT,
        renewals: Vec::new(),
    };

    match client.thread_wait(request, Duration::from_secs(1)).await {
        Err(BoardClientError::Connection(collaboration_client::ClientError::InvalidRequest(
            "invalid Thread Wait request",
        ))) => {}
        _ => return Err("invalid wait request was not rejected before transmission".into()),
    }

    ensure(
        peer.none_within(Duration::from_millis(25)).await,
        "invalid wait request reached the API",
    )
}

#[tokio::test]
async fn subscription_wait_malformed_success_preserves_inspection_identity() -> TestResult {
    let (_api, client, mut peer) = connected_client().await?;
    let request: ThreadSubscriptionWaitRequest = serde_json::from_value(json!({
        "actor":actor(),
        "filter":{"kind":"all"},
        "protocolVersion":1,"actingFor":null,"renewals":[],"maxWaitSeconds":1
    }))?;
    let expected_actor = request.actor.clone();
    let expected_filter = request.filter.clone();
    let server = tokio::spawn(async move {
        let wait = peer.next().await?;
        ensure(wait.tool == "thread_wait", "wrong wait method")?;
        wait.succeed(json!({
            "renewals":[],"outcome":{"kind":"batch","batch":{
                "kind":"notice",
                "pushId":"01900000-0000-7000-8000-000000000013",
                "line":"thread subscription notice",
                "held":false,
                "draining":false
            }}
        }));
        Ok::<(), TestError>(())
    });

    match client.thread_wait(request, Duration::from_secs(5)).await {
        Err(BoardClientError::WaitOutcomeUnknown {
            actor,
            filter,
            renewals,
        }) => {
            ensure(renewals.is_empty(), "unexpected uncertain renewal")?;
            ensure(
                *actor == expected_actor,
                "malformed result lost the Reader identity",
            )?;
            ensure(
                filter == expected_filter,
                "malformed result lost the wait filter",
            )?;
        }
        _ => return Err("malformed wait success was not classified as unknown".into()),
    }
    server.await??;
    Ok(())
}

async fn connected_client() -> Result<(ScriptedApi, CollaborationClient, ScriptedCalls), TestError>
{
    let (api, calls) = ScriptedApi::start().await?;
    let client =
        CollaborationClient::connect(api.directory(), "subscription-client-test", "1").await?;
    Ok((api, client, calls))
}
