#[path = "../../collaboration-service/tests/support/native_registry.rs"]
#[allow(dead_code)]
mod native_registry;
use collaboration_service as mail_service;
#[path = "../../collaboration-service/tests/support/native_mail_owner.rs"]
mod native_mail_owner;
use collaboration_client::CollaborationClient;
use collaboration_client::board::*;
use collaboration_contracts::MachineIdentity;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::TargetPresenceProbe;
use collaboration_runtime::SessionDeliveryRouter;
use collaboration_service::BoardAvailability;
use collaboration_service::ServiceIdentity;
use collaboration_service::SubscriptionDeliveryService;
use collaboration_service::SubscriptionDeliveryServiceProps;
use collaboration_service::SystemSubscriptionClock;
use collaboration_service::{ProjectAdmission, ProjectAdmissionProps};
use serde_json::Value;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

mod fake_api_support;
#[path = "thread_subscription_cli/wait_uncertainty.rs"]
mod wait_uncertainty;

const SERVICE_ID: &str = "00000000-0000-4000-8000-000000000101";
const SERVICE_EPOCH: &str = "00000000-0000-4000-8000-000000000102";
const SESSION_ID: &str = "thread-subscription-cli";
const NO_WATCH_SESSION_ID: &str = "thread-subscription-no-watch";
type TestError = Box<dyn std::error::Error + Send + Sync>;
type TestResult = Result<(), TestError>;

#[tokio::test]
async fn subscribe_join_wait_and_cancel_use_the_real_api_and_sqlite_paths() -> TestResult {
    let directory = tempfile::tempdir_in("/tmp")?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let service_directory = directory.path();
    let registry = native_registry::new_registry(service_directory)?;
    let origin = message_board::ActorOrigin {
        installation: message_board::ServiceId::try_from(SERVICE_ID.to_owned())?,
        machine: message_board::MachineLabel::try_from("CLI subscription proof".to_owned())?,
    };
    let (fleet, fleet_runtime) = board_central_test_support::fleet_client(
        service_directory,
        Arc::new(native_registry::PendingRegistration),
        &origin,
    )
    .await?;
    let fleet = Arc::new(fleet);
    let admission = Arc::new(ProjectAdmission::new(ProjectAdmissionProps {
        registry: registry.clone(),
        fleet: fleet.clone(),
        identity: MachineIdentity::new(
            collaboration_protocol::UuidIdentity::try_from(SERVICE_ID.to_owned())?,
            Some("CLI subscription proof"),
        )?,
    })?);
    let push_store = Arc::new(tokio::sync::Mutex::new(
        automation_storage::AutomationStore::open(&service_directory.join("automation.sqlite"))
            .await?,
    ));
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(service_directory.join("interaction.sqlite"))
            .await?,
    );
    let store = Arc::new(tokio::sync::Mutex::new(
        admission.subscription_window_store(interactions.clone()),
    ));
    let router = Arc::new(SessionDeliveryRouter::new(Vec::new()));
    let delivery: Arc<dyn SessionMessageDelivery> = router.clone();
    let presence: Arc<dyn TargetPresenceProbe> = router.clone();
    let machine_identity = MachineIdentity::new(
        collaboration_protocol::UuidIdentity::try_from(SERVICE_ID.to_owned())?,
        None,
    )?;
    let subscription_delivery =
        SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
            board_availability: BoardAvailability::Available(Arc::clone(&store)),
            push_store: interactions.clone(),
            delivery,
            presence: Arc::clone(&presence),
            machine_identity,
            clock: Arc::new(SystemSubscriptionClock),
        });
    let identity = ServiceIdentity::new(SERVICE_ID, SERVICE_EPOCH)
        .map_err(std::io::Error::other)?
        .with_project_admission(admission.clone())
        .with_automation_store(push_store.clone())
        .with_session_delivery(router)
        .with_subscription_delivery_service(subscription_delivery.clone(), presence.clone());
    let (identity, native) = native_mail_owner::NativeMailOwner::start(
        identity,
        native_registry::fixture_peer()?,
        collaboration_service::MailPushDeliveryProps {
            interactions: interactions.clone(),
            actors: subscription_delivery.clone(),
            presence,
            machine: MachineIdentity::new(SERVICE_ID.to_owned().try_into()?, None)?,
            clock: Arc::new(SystemSubscriptionClock),
        },
    )?;
    let publisher = Arc::new(collaboration_service::NotificationPublisher::new(
        admission,
        interactions.clone(),
        Arc::downgrade(&native.runtime_handle()),
    ));
    let identity = identity.with_notification_publisher(publisher.clone());
    subscription_delivery.start().await?;
    let served = collaboration_mcp::test_support::ServedCollaborationApi::start(
        service_directory,
        collaboration_service::CollaborationApplication::new(identity),
    )
    .await?;
    let client =
        CollaborationClient::connect(service_directory, "thread-subscription-cli-test", "1")
            .await?;

    let owner = human_actor("owner")?;
    let reader = session_actor(SESSION_ID)?;
    let project_id = ProjectId::generate();
    let board_id = BoardId::generate();
    let workspace_id = WorkspaceId::generate();
    accepted(
        &client
            .workspace_create(request(
                &owner,
                json!({"workspaceId":workspace_id,"name":"Workspace","description":""}),
            )?)
            .await?,
    );
    accepted(&client.board_create(request(&owner, json!({"workspaceId":workspace_id,"boardId":board_id,"name":"Board","description":"","kind":{"kind":"area"}}))?).await?);
    let creation: ProjectCreateRequest = request(
        &owner,
        json!({"boardId":board_id,"projectId":project_id,"title":"Project"}),
    )?;
    native_registry::prepare_registered_creation(
        &fleet,
        &registry,
        &creation,
        &ActorOrigin {
            installation: ServiceId::try_from(SERVICE_ID.to_owned())?,
            machine: MachineLabel::try_from("CLI subscription proof".to_owned())?,
        },
    )
    .await?;
    accepted(&client.project_create(creation).await?);
    accepted(&client.role_grant(request(&owner, json!({"projectId":project_id,"operation":{"kind":"roleGrant","subject":{"kind":"project","subject":project_id},"role":"lead","holder":owner,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}}))?).await?);
    let first_root = post_root(&client, &owner, &project_id, "first root").await?;

    let join = run_cli(
        service_directory,
        &[
            "thread",
            "join",
            "--thread-id",
            first_root.as_str(),
            "--actor",
            "self",
            "--role",
            "advisor",
            "--mode",
            "poll",
            "--when-idle",
            "drop",
        ],
    )
    .await?;
    ensure_cli_success(&join)?;
    let first_watch = client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (first_root.clone()).into(),
            reader: ReadActor::Identified {
                reader: reader.clone(),
            },
        })
        .await?
        .watch_status
        .ok_or("joined session Watch status was omitted")?;
    ensure(
        first_watch.watching,
        "join without --watch must watch by default",
    )?;
    let joined_subscriptions = client
        .thread_subscriptions(collaboration_client::protocol::ThreadSubscriptionsRequest {
            actor: reader.clone(),
        })
        .await?;
    ensure(
        joined_subscriptions.subscriptions.len() == 1,
        "joining did not create exactly one subscription",
    )?;
    let joined_subscription = joined_subscriptions
        .subscriptions
        .first()
        .ok_or("joining did not create a subscription")?;
    ensure(
        joined_subscription.policy.mode() == SubscriptionMode::Poll,
        "join --mode was not applied",
    )?;
    ensure(
        joined_subscription.policy.when_idle() == WhenIdle::Drop,
        "join --when-idle was not applied",
    )?;

    let no_watch_actor = serde_json::to_string(&session_actor(NO_WATCH_SESSION_ID)?)?;
    let no_watch_join = run_cli(
        service_directory,
        &[
            "thread",
            "join",
            "--thread-id",
            first_root.as_str(),
            "--actor",
            no_watch_actor.as_str(),
            "--role",
            "participant",
            "--no-watch",
        ],
    )
    .await?;
    ensure_cli_success(&no_watch_join)?;
    let no_watch_status = client
        .thread_show(ThreadShowRequest {
            include_redacted: false,
            thread: (first_root.clone()).into(),
            reader: ReadActor::Identified {
                reader: session_actor(NO_WATCH_SESSION_ID)?,
            },
        })
        .await?
        .watch_status
        .ok_or("joined session Watch status was omitted")?;
    ensure(
        !no_watch_status.watching,
        "--no-watch must opt out of the Watch",
    )?;
    ensure(
        client
            .thread_subscriptions(collaboration_client::protocol::ThreadSubscriptionsRequest {
                actor: session_actor(NO_WATCH_SESSION_ID)?,
            })
            .await?
            .subscriptions
            .is_empty(),
        "--no-watch join created a subscription",
    )?;

    let thread_subscribe = run_cli(
        service_directory,
        &[
            "thread",
            "subscribe",
            "--thread-id",
            first_root.as_str(),
            "--actor",
            "self",
            "--mode",
            "deliver",
            "--when-idle",
            "hold",
            "--quiet",
            "0s",
            "--cap",
            "5m",
            "--for",
            "1h",
        ],
    )
    .await?;
    accepted_value(&parse_cli_result(&thread_subscribe)?);
    let thread_view = subscription_view(
        &client,
        &reader,
        &SubscriptionScope::thread(first_root.clone()),
    )
    .await?;
    ensure(
        thread_view["scope"]["kind"] == "thread",
        "thread subscribe returned a different scope kind",
    )?;
    ensure(
        thread_view["scope"]["threadId"] == first_root.as_str(),
        "thread subscribe returned a different root",
    )?;
    ensure(
        thread_view["policy"]["mode"] == "deliver",
        "thread subscribe mode was not applied",
    )?;
    ensure(
        thread_view["policy"]["whenIdle"] == "hold",
        "thread subscribe when-idle policy was not applied",
    )?;
    ensure(
        thread_view["policy"]["timing"]["quietSeconds"] == 0,
        "thread subscribe quiet period was not applied",
    )?;
    ensure(
        thread_view["policy"]["timing"]["capSeconds"] == 300,
        "thread subscribe cap was not applied",
    )?;
    ensure(
        thread_view["policy"]["lifetime"] == 3600,
        "thread subscribe lifetime was not applied",
    )?;

    let project_subscribe = run_cli(
        service_directory,
        &[
            "thread",
            "subscribe",
            "--project-id",
            project_id.as_str(),
            "--actor",
            "self",
            "--mode",
            "off",
        ],
    )
    .await?;
    accepted_value(&parse_cli_result(&project_subscribe)?);
    let project_view = subscription_view(
        &client,
        &reader,
        &SubscriptionScope::project(project_id.clone()),
    )
    .await?;
    ensure(
        project_view["scope"]["kind"] == "project",
        "project subscribe returned a different scope kind",
    )?;
    ensure(
        project_view["scope"]["projectId"] == project_id.as_str(),
        "project subscribe returned a different topic",
    )?;

    let list = run_cli(
        service_directory,
        &["thread", "subscriptions", "--actor", "self"],
    )
    .await?;
    let list_result = parse_cli_result(&list)?;
    ensure(
        list_result.get("unavailable") == Some(&json!([])),
        "subscription output omits availability",
    )?;
    let subscriptions = list_result["subscriptions"]
        .as_array()
        .ok_or("subscriptions command omitted its subscription list")?;
    ensure(
        subscriptions.len() == 2,
        "subscriptions command returned the wrong number of records",
    )?;
    ensure(
        subscriptions.iter().any(|subscription| {
            subscription["scope"]["kind"] == "thread"
                && subscription["scope"]["threadId"] == first_root.as_str()
        }),
        "subscriptions command omitted the Thread scope",
    )?;
    ensure(
        subscriptions.iter().any(|subscription| {
            subscription["scope"]["kind"] == "project"
                && subscription["scope"]["projectId"] == project_id.as_str()
        }),
        "subscriptions command omitted the Project scope",
    )?;

    let non_poll_wait = run_cli(
        service_directory,
        &[
            "thread",
            "wait",
            "--thread-id",
            first_root.as_str(),
            "--actor",
            "self",
            "--max-wait",
            "1s",
        ],
    )
    .await?;
    let non_poll_result = parse_cli_result(&non_poll_wait)?;
    ensure(
        non_poll_result["outcome"]["kind"] == "refused",
        "wait against deliver mode must return its closed refusal",
    )?;
    let non_poll_message = non_poll_result["outcome"]["reason"]["message"]
        .as_str()
        .ok_or("non-poll refusal message")?;
    ensure(
        non_poll_message.contains("wait requires a poll subscription")
            && non_poll_message.contains("thread subscribe --mode poll"),
        "non-poll refusal omitted poll-mode correction",
    )?;

    let poll_subscription = run_cli(
        service_directory,
        &[
            "thread",
            "subscribe",
            "--thread-id",
            first_root.as_str(),
            "--actor",
            "self",
            "--mode",
            "poll",
            "--quiet",
            "0s",
            "--cap",
            "0s",
        ],
    )
    .await?;
    parse_cli_result(&poll_subscription)?;

    let timeout_wait = run_cli(
        service_directory,
        &[
            "thread",
            "wait",
            "--thread-id",
            first_root.as_str(),
            "--actor",
            "self",
            "--max-wait",
            "0s",
        ],
    )
    .await?;
    let timeout_result = parse_cli_result(&timeout_wait)?;
    ensure(
        timeout_result["outcome"]["kind"] == "timedOut",
        "wait with no pending activity did not return an empty batch",
    )?;

    accepted(&client.thread_post(request(&session_actor(NO_WATCH_SESSION_ID)?, json!({"postId":PostId::generate(),"placement":{"kind":"thread","thread":{"thread":first_root}},"text":"poll wait due","references":[],"addressees":[]}))?).await?);
    let due_wait = run_cli(
        service_directory,
        &[
            "thread",
            "wait",
            "--thread-id",
            first_root.as_str(),
            "--actor",
            "self",
            "--max-wait",
            "5s",
        ],
    )
    .await?;
    let due_result = parse_cli_result(&due_wait)?;
    ensure(
        due_result["outcome"]["batch"]["kind"] == "notice",
        "due poll wait did not return the session Notice variant",
    )?;
    let due_roots = due_result["outcome"]["batch"]["roots"]
        .as_array()
        .ok_or("due poll Notice omitted root ranges")?;
    ensure(
        due_roots.len() == 1,
        "due poll wait returned the wrong number of root ranges",
    )?;
    ensure(
        due_roots[0]["threadId"] == first_root.as_str(),
        "due poll wait returned a different root range",
    )?;
    let repeated_wait = run_cli(
        service_directory,
        &[
            "thread",
            "wait",
            "--thread-id",
            first_root.as_str(),
            "--actor",
            "self",
            "--max-wait",
            "0s",
        ],
    )
    .await?;
    ensure(
        parse_cli_result(&repeated_wait)?["outcome"]["kind"] == "timedOut",
        "the handed-off poll batch was returned a second time",
    )?;

    for (scope_flag, scope_id) in [
        ("--thread-id", first_root.as_str()),
        ("--project-id", project_id.as_str()),
    ] {
        let unsubscribe = run_cli(
            service_directory,
            &[
                "thread",
                "unsubscribe",
                scope_flag,
                scope_id,
                "--actor",
                "self",
            ],
        )
        .await?;
        accepted_value(&parse_cli_result(&unsubscribe)?);
        let scope = if scope_flag == "--thread-id" {
            SubscriptionScope::thread(first_root.clone())
        } else {
            SubscriptionScope::project(project_id.clone())
        };
        let project_store = registry
            .existing_project(&project_id)
            .await?
            .ok_or("native store")?;
        let mut read = project_store.snapshot().await?;
        let ended = read
            .membership(&reader, &scope)
            .await?
            .ok_or("ended subscription")?;
        ensure(
            matches!(
                ended.state_at(chrono::Utc::now(), None, false),
                SubscriptionState::Ended {
                    reason: EndReason::Cancelled
                }
            ),
            "unsubscribe must end subscription with Cancelled",
        )?;
        read.finish().await?;
    }
    ensure(
        client
            .thread_subscriptions(collaboration_client::protocol::ThreadSubscriptionsRequest {
                actor: reader,
            })
            .await?
            .subscriptions
            .is_empty(),
        "subscriptions remained after cancelling every scope",
    )?;

    served.stop().await?;
    subscription_delivery.shutdown().await;
    publisher.shutdown().await;
    drop(subscription_delivery);
    drop(publisher);
    drop(store);
    drop(push_store);
    drop(interactions);
    native.close().await?;
    fleet_runtime.shutdown().await?;
    Ok(())
}

fn request<TRequest: serde::de::DeserializeOwned>(
    actor: &Identity,
    fields: Value,
) -> Result<TRequest, TestError> {
    let mut value = json!({"requestId":RequestId::generate(),"protocolVersion":1,"actor":actor,"actingFor":null});
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
fn accepted_value(reply: &Value) {
    assert_eq!(
        reply.pointer("/result/decision/kind"),
        Some(&json!("accepted")),
        "{reply}"
    );
}
async fn subscription_view(
    client: &CollaborationClient,
    reader: &Identity,
    scope: &SubscriptionScope,
) -> Result<Value, TestError> {
    let row = client
        .thread_subscriptions(collaboration_client::protocol::ThreadSubscriptionsRequest {
            actor: reader.clone(),
        })
        .await?
        .subscriptions
        .into_iter()
        .find(|row| row.scope == *scope)
        .ok_or("subscription omitted")?;
    Ok(serde_json::to_value(row)?)
}
async fn post_root(
    client: &CollaborationClient,
    owner: &Identity,
    project_id: &ProjectId,
    text: &str,
) -> Result<ThreadId, TestError> {
    let post = PostId::generate();
    accepted(&client.thread_create(request(owner, json!({"postId":post,"projectId":project_id,"about":{"kind":"project","subject":{"line":"Subscription fixture discussion"}},"text":text,"references":[],"addressees":[],"role":null,"watch":false}))?).await?);
    Ok(ThreadId::from(post))
}

async fn run_cli(
    service_directory: &Path,
    arguments: &[&str],
) -> Result<std::process::Output, TestError> {
    Ok(tokio::time::timeout(
        Duration::from_secs(10),
        tokio::process::Command::new(env!("CARGO_BIN_EXE_agent-collaboration"))
            .args(arguments)
            .arg("--service-directory")
            .arg(service_directory)
            .arg("--json")
            .env("CODEX_THREAD_ID", SESSION_ID)
            .env_remove("CLAUDE_CODE_SESSION_ID")
            .env_remove("CURSOR_CONVERSATION_ID")
            .kill_on_drop(true)
            .output(),
    )
    .await??)
}

fn ensure(condition: bool, message: &'static str) -> TestResult {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

fn ensure_cli_success(output: &std::process::Output) -> TestResult {
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "CLI failed with status {:?}\nstdout: {}\nstderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
        .into())
    }
}

fn parse_cli_result(output: &std::process::Output) -> Result<Value, TestError> {
    ensure_cli_success(output)?;
    let envelope: Value = serde_json::from_slice(&output.stdout)?;
    let result = envelope
        .get("result")
        .cloned()
        .ok_or_else(|| "CLI result envelope omitted result".to_owned())?;
    Ok(result.get("record").cloned().unwrap_or(result))
}

fn human_actor(value: &str) -> Result<Identity, IdentityValidationError> {
    Ok(Identity::Human {
        human_id: HumanId::try_from(value.to_owned())?,
    })
}

fn session_actor(value: &str) -> Result<Identity, IdentityValidationError> {
    Ok(Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: ServiceId::try_from(SERVICE_ID.to_owned())?,
                endpoint_id: EndpointId::try_from("codex-local".to_owned())?,
            },
            session_id: SessionId::try_from(value.to_owned())?,
        },
    })
}
