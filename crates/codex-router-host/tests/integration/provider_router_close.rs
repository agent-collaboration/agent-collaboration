//! Router-only close settles on the owner and permits same-ID restore.

#![expect(
    clippy::expect_used,
    reason = "A malformed fixture must fail before asserting the close and restore."
)]
#![expect(
    clippy::result_large_err,
    reason = "The tests match the full public operation failures."
)]

use super::provider_acp_message_fifo_support::{provider_binding, recorded_requests, target};
use super::provider_fixture_launch;
use codex_router_host::{
    ExternalProviderBinding, ExternalProviderRuntime, ExternalProviderSupervisor,
};
use collaboration_contracts::ProviderConversationBackend;
use collaboration_protocol::{
    ConversationCloseRequest, ConversationLoadRequest, ConversationOperationFailure,
    ConversationOperationSettlement, ConversationOperationWaitOutput,
    ConversationOperationWaitRequest, ConversationPromptRequest, MessageContent, MessageDelivery,
    MessageText, OperationId, PositiveSeconds, ProviderRequestedPolicy, ProviderWorkingDirectory,
    RouterAccess, SessionId, SessionRef,
};
use collaboration_runtime::{ProviderOperationStore, ProviderSessionRecord};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

struct RouterCloseFixture {
    root: tempfile::TempDir,
    wire: PathBuf,
    target: SessionRef,
    supervisor: ExternalProviderSupervisor,
    cwd: ProviderWorkingDirectory,
    policy: ProviderRequestedPolicy,
}

impl RouterCloseFixture {
    async fn start(hold_prompt: bool) -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        let wire = root.path().join("wire.json");
        let mut target = target();
        target.session_id =
            SessionId::try_from("reactive-session-1".to_owned()).expect("session ID");
        let mut arguments = vec!["reactive".to_owned(), wire.display().to_string()];
        if hold_prompt {
            arguments.push("hold-first-prompt-open".to_owned());
        }
        let runtime = ExternalProviderRuntime::initialize(
            provider_fixture_launch::launch("cursor-settings-agent.ts", arguments),
            Arc::new(acp_provider_adapters::StandardAcpBehavior),
        )
        .await
        .expect("fixture provider");
        let store = Arc::new(Mutex::new(
            ProviderOperationStore::open(&root.path().join("operations.sqlite"))
                .await
                .expect("operation store"),
        ));
        let cwd = ProviderWorkingDirectory::try_from(root.path().display().to_string())
            .expect("fixture directory");
        let policy = ProviderRequestedPolicy {
            access: RouterAccess::WriteRestricted,
        };
        store
            .lock()
            .await
            .record_session(&ProviderSessionRecord {
                target: target.clone(),
                working_directory: cwd.clone(),
                requested_policy: policy.clone(),
                created_by: target.clone().into(),
                approver: target.clone().into(),
                updated_at_ms: 0,
                resume: Default::default(),
            })
            .await
            .expect("saved setup");
        let supervisor = ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: provider_binding(&target),
                runtime,
            }],
            store,
        )
        .expect("fixture Host");
        Self {
            root,
            wire,
            target,
            supervisor,
            cwd,
            policy,
        }
    }

    async fn load(&self) -> Result<ConversationOperationWaitOutput, ConversationOperationFailure> {
        let operation_id = OperationId::generate();
        self.supervisor
            .load(ConversationLoadRequest {
                operation_id: operation_id.clone(),
                target: self.target.clone(),
                generation: None,
                working_directory: self.cwd.clone(),
                requested_by: self.target.clone().into(),
                approver: self.target.clone().into(),
                requested_policy: self.policy.clone(),
            })
            .await?;
        self.wait(operation_id).await
    }

    async fn close(&self) -> Result<ConversationOperationWaitOutput, ConversationOperationFailure> {
        let operation_id = OperationId::generate();
        self.supervisor
            .close(ConversationCloseRequest {
                operation_id: operation_id.clone(),
                target: self.target.clone(),
                generation: None,
                requested_by: self.target.clone().into(),
                approver: self.target.clone().into(),
            })
            .await?;
        self.wait(operation_id).await
    }

    async fn wait(
        &self,
        operation_id: OperationId,
    ) -> Result<ConversationOperationWaitOutput, ConversationOperationFailure> {
        self.supervisor
            .wait(ConversationOperationWaitRequest {
                operation_id,
                timeout_seconds: PositiveSeconds::try_from(10).expect("bound"),
            })
            .await
            .map(|result| result.output)
    }

    async fn shutdown(&self) {
        self.supervisor.shutdown().await.expect("fixture stops");
    }
}

fn assert_router_close(output: ConversationOperationWaitOutput) {
    assert!(
        matches!(
            output,
            ConversationOperationWaitOutput::Available {
                settlement: ConversationOperationSettlement::Closed {
                    router_closed: true,
                    ..
                }
            }
        ),
        "expected Router-only close, got {output:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_released_session_loads_again_on_the_same_connection() {
    let fixture = RouterCloseFixture::start(false).await;
    fixture.load().await.expect("first load");
    assert_router_close(fixture.close().await.expect("Router releases session"));

    let reloaded = fixture.load().await;
    let loads = recorded_requests(&fixture.wire, "session/load");
    fixture.shutdown().await;

    assert!(reloaded.is_ok(), "same ID loads again: {reloaded:?}");
    assert_eq!(loads, 2, "the agent saw both session/loads");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_release_during_a_running_prompt_frees_the_session_for_a_new_load() {
    let fixture = RouterCloseFixture::start(true).await;
    let opened = fixture
        .supervisor
        .open_provider_fixture(&fixture.target.endpoint, fixture.root.path().to_owned())
        .await
        .expect("session/new");
    assert_eq!(opened, String::from(fixture.target.session_id.clone()));
    let held = OperationId::generate();
    fixture
        .supervisor
        .prompt(ConversationPromptRequest {
            delivery: MessageDelivery::Auto,
            operation_id: held.clone(),
            input_id: None,
            target: fixture.target.clone(),
            generation: None,
            requested_by: fixture.target.clone().into(),
            approver: fixture.target.clone().into(),
            prompt: MessageContent::HumanUser {
                text: MessageText::try_from("first".to_owned()).expect("prompt"),
            },
        })
        .await
        .expect("admit held prompt");
    while recorded_requests(&fixture.wire, "session/prompt") < 1 {
        tokio::task::yield_now().await;
    }

    let released = fixture.close().await;
    let held = fixture.wait(held).await;
    let reloaded = fixture.load().await;
    let cancels = recorded_requests(&fixture.wire, "session/cancel");
    let loads = recorded_requests(&fixture.wire, "session/load");
    fixture.shutdown().await;

    assert_router_close(released.expect("Router-only close completes"));
    assert!(held.is_ok(), "the interrupted prompt returns: {held:?}");
    assert_eq!(cancels, 1, "the running turn was interrupted once");
    assert!(
        reloaded.is_ok(),
        "the freed session loads again: {reloaded:?}"
    );
    assert_eq!(loads, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn releasing_a_session_that_is_not_loaded_answers_nothing_to_close() {
    let fixture = RouterCloseFixture::start(false).await;

    let released = fixture.close().await;
    let loads = recorded_requests(&fixture.wire, "session/load");
    fixture.shutdown().await;

    let failure = released.expect_err("nothing to close");
    assert_eq!(
        String::from(failure.message),
        "session is not loaded; nothing to close; load it or resume it"
    );
    assert_eq!(
        failure.effect,
        collaboration_protocol::ProviderOperationEffect::None
    );
    assert_eq!(loads, 0, "the agent saw no session/load");
}
