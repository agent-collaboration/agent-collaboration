use super::*;
#[path = "auto_direct_message_tests/uncertainty_push_tests.rs"]
mod uncertainty_push_tests;
use collaboration_contracts::MachineIdentity;
use collaboration_protocol::{
    PushDeliveryState, PushHeaderFacts, PushKind, PushOrigin, PushRecordDraft,
};
use collaboration_service::BoardAvailability;
use collaboration_service::SubscriptionDeliveryService;
use collaboration_service::SubscriptionDeliveryServiceProps;
use collaboration_service::SystemSubscriptionClock;
use interaction_storage::InteractionStore;

fn loadable_provider_fixture(capture: &Path, prompt_event: &Path) -> ExternalProviderLaunch {
    let script = format!(
        r#"
import json,socket,sys
request=json.loads(sys.stdin.readline())
assert request['method']=='initialize'
print(json.dumps({{'jsonrpc':'2.0','id':request['id'],'result':{{'protocolVersion':1,'agentCapabilities':{{'loadSession':True}},'agentInfo':{{'name':'saved-session-fixture','version':'1'}}}}}}),flush=True)
for line in sys.stdin:
 request=json.loads(line)
 assert request['method'] in ['session/load','session/prompt'], request['method']
 assert request['params']['sessionId']=='fixture-session'
 with open({:?},'a') as output: output.write(json.dumps(request)+'\n')
 result={{}} if request['method']=='session/load' else {{'stopReason':'end_turn'}}
 print(json.dumps({{'jsonrpc':'2.0','id':request['id'],'result':result}}),flush=True)
 if request['method']=='session/prompt':
  with socket.socket(socket.AF_UNIX,socket.SOCK_STREAM) as event:
   event.connect({:?})
   event.sendall(b'observed')
"#,
        capture.display().to_string(),
        prompt_event.display().to_string()
    );
    ExternalProviderLaunch {
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
        executable: PathBuf::from("/usr/bin/python3"),
        arguments: vec!["-c".to_owned(), script],
        environment: Vec::new(),
    }
}

async fn save_direct_message(
    store: &InteractionStore,
    target: &SessionRef,
    mode: MessageDelivery,
) -> PushId {
    let push_id = PushId::try_from(uuid::Uuid::now_v7().to_string()).expect("UUIDv7 push");
    store
        .insert_push_record(PushRecordDraft {
            push_id: push_id.clone(),
            kind: PushKind::DirectMessage,
            origin: PushOrigin::Session(target.clone()),
            origin_router_ref: None,
            target: target.clone(),
            mode: Some(mode),
            guard: None,
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::DirectMessage {
                sender_display_name: None,
            },
            body: Some("saved ACP direct message".to_owned()),
            activity: None,
            created_at: chrono::Utc::now(),
        })
        .await
        .expect("stored pending DM");
    push_id
}

#[tokio::test]
async fn new_auto_loads_saved_unloaded_acp_session_once_while_queue_stays_held() {
    // The real owner, stores, route, supervisor and ACP carrier stay in the path.
    let root = tempfile::tempdir().expect("provider root");
    let capture = root.path().join("provider-requests.jsonl");
    let event_socket = root.path().join("prompt.sock");
    let listener = tokio::net::UnixListener::bind(&event_socket).expect("prompt observer");
    let target = target();
    let binding = provider_binding(&target);
    let runtime =
        ExternalProviderRuntime::initialize(loadable_provider_fixture(&capture, &event_socket))
            .await
            .expect("provider initialized without loading a session");
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("provider store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: target.clone().into(),
            approver: target.clone().into(),
            updated_at_ms: 1,
        })
        .await
        .expect("saved unloaded provider session");
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: binding.clone(),
                runtime,
            }],
            store.clone(),
        )
        .expect("supervisor"),
    );
    let route = Arc::new(ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        available_directory(&target, &binding),
        supervisor.clone(),
        store,
        Arc::new(NoLivePeer),
    ));
    assert_eq!(
        route
            .presence(&target)
            .await
            .expect("saved session presence"),
        RoutePresence::Wakeable
    );
    let router = Arc::new(SessionDeliveryRouter::new(vec![route.clone()]));
    let pushes = Arc::new(
        InteractionStore::open(&root.path().join("interaction.sqlite"))
            .await
            .expect("push store"),
    );
    let owner = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Unavailable,
        push_store: pushes.clone(),
        delivery: router.clone(),
        presence: router,
        machine_identity: MachineIdentity::new(
            target.endpoint.service_id.clone(),
            Some("ACP direct-message proof"),
        )
        .expect("machine"),
        clock: Arc::new(SystemSubscriptionClock),
    });
    owner.start().await.expect("owner starts");

    let queued_id = save_direct_message(&pushes, &target, MessageDelivery::Queue).await;
    let queued = tokio::time::timeout(
        Duration::from_secs(5),
        owner.deliver_direct_message(target.clone(), queued_id),
    )
    .await
    .expect("Queue settles without loading")
    .expect("Queue record");
    assert_eq!(queued.delivery_state, PushDeliveryState::Held);
    assert!(!capture.exists(), "Queue must send neither load nor prompt");
    assert_eq!(
        route.presence(&target).await.expect("still unloaded"),
        RoutePresence::Wakeable
    );

    let auto_id = save_direct_message(&pushes, &target, MessageDelivery::Auto).await;
    let delivered = tokio::time::timeout(
        Duration::from_secs(5),
        owner.deliver_direct_message(target.clone(), auto_id.clone()),
    )
    .await
    .expect("Auto delivery deadline")
    .expect("Auto record");
    assert_eq!(delivered.delivery_state, PushDeliveryState::Delivered);
    let (mut event, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .expect("provider prompt deadline")
        .expect("provider observed prompt");
    let mut observed = Vec::new();
    event
        .read_to_end(&mut observed)
        .await
        .expect("provider observation");
    assert_eq!(observed, b"observed");
    let readback = tokio::time::timeout(
        Duration::from_secs(5),
        owner.deliver_direct_message(target.clone(), auto_id.clone()),
    )
    .await
    .expect("terminal readback deadline")
    .expect("terminal readback");
    assert_eq!(readback.delivery_state, PushDeliveryState::Delivered);
    owner.shutdown().await;
    drop(owner);
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("provider shutdown");
    Arc::try_unwrap(pushes)
        .unwrap_or_else(|_| panic!("push owner retained"))
        .close()
        .await
        .expect("push owner closed");

    let requests = std::fs::read_to_string(capture)
        .expect("actual ACP requests")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("ACP request JSON"))
        .collect::<Vec<_>>();
    assert_eq!(
        requests.len(),
        2,
        "one load, one prompt, no replacement session or replay"
    );
    assert_eq!(requests[0]["method"], "session/load");
    assert_eq!(requests[1]["method"], "session/prompt");
    let blocks = requests[1]["params"]["prompt"]
        .as_array()
        .expect("prompt blocks");
    assert_eq!(blocks.len(), 1);
    let text = blocks[0]["text"].as_str().expect("prepared notice");
    assert!(text.contains("saved ACP direct message"));
    assert!(text.ends_with(&format!(
        "router://{}/push/{}",
        String::from(target.endpoint.service_id),
        auto_id.as_str()
    )));
}
