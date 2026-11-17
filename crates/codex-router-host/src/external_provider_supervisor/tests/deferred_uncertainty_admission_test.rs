//! The uncertainty controls of D1 V22 at the API prompt's two Deferred
//! dispatch points: a fresh send (`send_api_prompt`, admitted with
//! `DispatchMark::Deferred` through `still_blocking`) and a queued entry's
//! dispatch (`dispatch_queued_api_prompt`). Each control is a state the
//! public API can't force on demand: an unresolved row in the current binding
//! generation with no live entry, a terminal Unknown row whose published
//! entry is still retained, and the moment between a terminal Unknown row
//! being stored and its result being published. The rows and live entries are
//! seeded directly; the agent is the real Cursor fixture, whose receipt
//! records every request it reads.

use super::*;
use crate::provider_acp_message_fifo::ProviderAcpMessageFifo;
use collaboration_runtime::ProviderSessionRecord;

/// The text of the send each control refuses; no agent frame may carry it.
const CONTROL_TEXT: &str = "control";

/// The current-generation uncertainty a control seeds for the session.
#[derive(Clone, Copy, Debug)]
enum SeededUncertainty {
    /// MayHaveDispatched, effect Unknown, no live entry: a row this process
    /// is not running, such as one left by a crash in this generation.
    NonLiveUnknownRow,
    /// Terminal Unknown, its result published and the entry still retained
    /// in `live_operations`, as right after a same-process unknown failure.
    PublishedUnknownRetained,
    /// Terminal Unknown and its live entry not yet published: the moment
    /// between `finish` storing the row and `publish_result`.
    StoredBeforePublished,
}

type HeldTurn = tokio::task::JoinHandle<
    Result<crate::ExternalProviderPromptOutcome, crate::ExternalProviderRuntimeError>,
>;

/// One supervisor with its session line, over the real Cursor fixture, with
/// a turn running on the session that no operation row owns.
struct ControlFixture {
    root: tempfile::TempDir,
    target: SessionRef,
    store: Arc<Mutex<ProviderOperationStore>>,
    supervisor: Arc<ExternalProviderSupervisor>,
    queue: Arc<ProviderAcpMessageFifo>,
    held: HeldTurn,
}

impl ControlFixture {
    fn receipt(&self) -> PathBuf {
        self.root.path().join("wire.json")
    }

    async fn start(behaviours: &[&str]) -> Self {
        let root = tempfile::tempdir().expect("fixture root");
        let receipt = root.path().join("wire.json");
        let target = SessionRef {
            endpoint: endpoint(),
            session_id: SessionId::try_from("reactive-session-1".to_owned()).expect("session"),
        };
        let mut arguments = vec!["reactive".to_owned(), receipt.display().to_string()];
        arguments.extend(behaviours.iter().map(|behaviour| (*behaviour).to_owned()));
        let runtime = ExternalProviderRuntime::initialize(
            crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
                "cursor-settings-agent.ts",
                arguments,
            ),
            Arc::new(acp_provider_adapters::StandardAcpBehavior),
        )
        .await
        .expect("fixture provider");
        runtime
            .create_session(root.path().to_owned())
            .await
            .expect("session/new");
        let store = Arc::new(Mutex::new(
            ProviderOperationStore::open(&root.path().join("operations.sqlite"))
                .await
                .expect("store"),
        ));
        store
            .lock()
            .await
            .record_session(&ProviderSessionRecord {
                target: target.clone(),
                working_directory: ProviderWorkingDirectory::try_from(
                    root.path().display().to_string(),
                )
                .expect("working directory"),
                requested_policy: ProviderRequestedPolicy {
                    access: RouterAccess::WriteRestricted,
                },
                created_by: target.clone().into(),
                approver: target.clone().into(),
                updated_at_ms: 1,
            })
            .await
            .expect("session record");
        let supervisor = Arc::new(
            ExternalProviderSupervisor::new(
                vec![ExternalProviderBinding {
                    identity: binding(),
                    runtime,
                }],
                Arc::clone(&store),
            )
            .expect("supervisor"),
        );
        let queue = Arc::new(ProviderAcpMessageFifo::new(
            Arc::clone(&supervisor),
            Arc::clone(&store),
            Arc::new(NoLivePeer),
            std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
        ));
        supervisor.attach_message_fifo(&queue);
        let runtime = supervisor.runtime_for(&target.endpoint).expect("runtime");
        let held_context = crate::ExternalProviderApprovalContext {
            requester: target.clone().into(),
            approver: target.clone().into(),
            target: target.clone(),
            operation_id: OperationId::generate(),
            binding_generation: binding().generation,
            binding_retirement: runtime.retirement(),
        };
        let held = tokio::spawn(async move {
            runtime
                .prompt_with_approval_context(
                    "reactive-session-1".to_owned(),
                    "first".to_owned(),
                    held_context,
                )
                .await
        });
        while recorded_frames(&receipt, "session/prompt", None) < 1 {
            tokio::task::yield_now().await;
        }
        Self {
            root,
            target,
            store,
            supervisor,
            queue,
            held,
        }
    }

    /// Stores the seeded uncertainty for this session in the current
    /// generation and sets its live entry; returns its operation ID.
    async fn seed(&self, seeded: SeededUncertainty) -> OperationId {
        let operation_id = OperationId::generate();
        {
            let mut store = self.store.lock().await;
            store
                .admit(ProviderOperationAdmission {
                    operation_id: operation_id.clone(),
                    operation_kind: ProviderOperationKind::ConversationPrompt,
                    binding: ConversationBindingIdentity::ExternalProvider { binding: binding() },
                    admitted_at_ms: 1,
                })
                .await
                .expect("seeded admission");
            store
                .record_target(&operation_id, &self.target, 2)
                .await
                .expect("seeded target");
            store
                .mark_may_have_dispatched(&operation_id, 3)
                .await
                .expect("seeded dispatch mark");
            if !matches!(seeded, SeededUncertainty::NonLiveUnknownRow) {
                store
                    .record_terminal(
                        &operation_id,
                        ProviderOperationEffect::Unknown,
                        ProviderReconciliationState::Unresolved,
                        None,
                        4,
                    )
                    .await
                    .expect("seeded terminal Unknown");
            }
        }
        match seeded {
            SeededUncertainty::NonLiveUnknownRow => {}
            SeededUncertainty::PublishedUnknownRetained => {
                self.supervisor.hold_live_operation(operation_id.clone());
                assert!(self.supervisor.publish_held_result(&operation_id).await);
            }
            SeededUncertainty::StoredBeforePublished => {
                self.supervisor.hold_live_operation(operation_id.clone());
            }
        }
        // The binding's uncertainty cell holds it, as the start-up seed (a
        // row with no live owner) or its live owner (an unknown failure)
        // records it there.
        self.supervisor
            .record_unknown_for_test(&operation_id, &binding(), Some(self.target.clone()))
            .await;
        operation_id
    }

    /// One API prompt from the session to itself, through the backend trait
    /// the API calls.
    #[expect(
        clippy::result_large_err,
        reason = "The backend trait answers with the full typed failure."
    )]
    async fn send_control(
        &self,
        operation_id: &OperationId,
    ) -> Result<ConversationOperationSubmission, ConversationOperationFailure> {
        self.supervisor
            .prompt(ConversationPromptRequest {
                delivery: collaboration_protocol::MessageDelivery::Queue,
                operation_id: operation_id.clone(),
                input_id: None,
                target: self.target.clone(),
                generation: None,
                requested_by: self.target.clone().into(),
                approver: self.target.clone().into(),
                prompt: collaboration_protocol::MessageContent::HumanUser {
                    text: MessageText::try_from(CONTROL_TEXT.to_owned()).expect("prompt"),
                },
            })
            .await
    }

    async fn show(&self, operation_id: &OperationId) -> Option<ConversationOperationSnapshot> {
        self.supervisor
            .show(ConversationOperationShowRequest {
                operation_id: operation_id.clone(),
            })
            .await
            .ok()
    }

    /// Ends the held turn, publishes a still-pending seeded entry so the
    /// drain can finish, and stops the line and the supervisor.
    async fn stop(self, seeded: &OperationId, seeded_state: SeededUncertainty) -> StoppedControl {
        node_test_fixture::release_fixture(&self.receipt(), ".release")
            .expect("release the held turn");
        let held = self.held.await.expect("held turn task");
        if matches!(seeded_state, SeededUncertainty::StoredBeforePublished) {
            assert!(self.supervisor.publish_held_result(seeded).await);
        }
        self.queue.shutdown().await;
        let stopped = self.supervisor.shutdown().await;
        StoppedControl {
            root: self.root,
            held_ended: held.is_ok(),
            stopped,
        }
    }
}

struct StoppedControl {
    root: tempfile::TempDir,
    held_ended: bool,
    stopped: Result<(), &'static str>,
}

impl StoppedControl {
    /// Requests with `method` the fixture read, and only those whose prompt
    /// text is `text` when one is named.
    fn recorded(&self, method: &str, text: Option<&str>) -> usize {
        recorded_frames(&self.root.path().join("wire.json"), method, text)
    }
}

fn recorded_frames(receipt: &std::path::Path, method: &str, text: Option<&str>) -> usize {
    std::fs::read(receipt)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Vec<serde_json::Value>>(&bytes).ok())
        .unwrap_or_default()
        .iter()
        .filter(|frame| frame.get("method").and_then(serde_json::Value::as_str) == Some(method))
        .filter(|frame| {
            text.is_none_or(|text| {
                frame
                    .pointer("/params/prompt/0/text")
                    .and_then(serde_json::Value::as_str)
                    == Some(text)
            })
        })
        .count()
}

/// A fresh API send while `seeded_state` holds: the Deferred admission
/// refuses it for the uncertainty, and no steer or prompt reaches the agent,
/// whose running turn would take a steer.
async fn assert_fresh_send_refused(seeded_state: SeededUncertainty) {
    // Arrange: a running turn the agent would steer into, and the seeded
    // uncertainty in the current generation.
    let fixture = ControlFixture::start(&["hold-first-prompt-open", "advertise-steering"]).await;
    let seeded = fixture.seed(seeded_state).await;

    // Act
    let requested = OperationId::generate();
    let refused = fixture.send_control(&requested).await;
    let requested_shown = fixture.show(&requested).await;
    let stopped = fixture.stop(&seeded, seeded_state).await;

    // Assert: refused for the uncertainty, and nothing reached the agent.
    let failure = match refused {
        Err(failure) => failure,
        Ok(submission) => panic!("{seeded_state:?}: the send is refused: {submission:?}"),
    };
    assert_eq!(
        failure.kind,
        ConversationOperationFailureKind::Busy,
        "{seeded_state:?}: {failure:?}"
    );
    assert_eq!(
        String::from(failure.message.clone()),
        format!(
            "an earlier send's outcome is unknown; reconcile {} first",
            seeded.as_str()
        ),
        "{seeded_state:?}: the refusal names the uncertain operation"
    );
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    assert!(
        requested_shown.is_none(),
        "{seeded_state:?}: a refused send leaves no row: {requested_shown:?}"
    );
    assert_eq!(
        stopped.recorded("_session/steering", None),
        0,
        "{seeded_state:?}: zero steer"
    );
    assert_eq!(
        stopped.recorded("session/prompt", Some(CONTROL_TEXT)),
        0,
        "{seeded_state:?}: zero prompt"
    );
    assert_eq!(
        stopped.recorded("session/prompt", None),
        1,
        "{seeded_state:?}: only the held turn's prompt"
    );
    assert!(stopped.held_ended, "{seeded_state:?}: the held turn ended");
    stopped
        .stopped
        .unwrap_or_else(|error| panic!("{seeded_state:?}: supervisor shutdown: {error}"));
}

/// An API prompt queued behind a running turn before any uncertainty
/// existed, dispatched once the turn ends while `seeded_state` holds: the
/// entry ends not sent, with no effect, and no steer or prompt reaches the
/// agent for it.
async fn assert_queued_entry_not_sent(seeded_state: SeededUncertainty) {
    // Arrange: a running turn the agent can't steer into, an API prompt
    // waiting in the line behind it, then the seeded uncertainty.
    let fixture = ControlFixture::start(&["hold-first-prompt-open"]).await;
    let queued = OperationId::generate();
    let admitted = fixture.send_control(&queued).await;
    let queued_before = fixture
        .show(&queued)
        .await
        .and_then(|snapshot| snapshot.queue_state);
    let seeded = fixture.seed(seeded_state).await;

    // Act: the turn ends and the line dispatches its entry.
    node_test_fixture::release_fixture(&fixture.receipt(), ".release")
        .expect("release the held turn");
    let settled = fixture
        .supervisor
        .wait(ConversationOperationWaitRequest {
            operation_id: queued.clone(),
            timeout_seconds: PositiveSeconds::try_from(10).expect("bound"),
        })
        .await;
    let queued_shown = fixture.show(&queued).await;
    let stopped = fixture.stop(&seeded, seeded_state).await;

    // Assert: the entry ends not sent, and nothing reached the agent.
    assert!(
        admitted.is_ok(),
        "{seeded_state:?}: the prompt was admitted to wait: {admitted:?}"
    );
    assert_eq!(
        queued_before,
        Some(collaboration_protocol::ConversationOperationQueueState::RouterQueued),
        "{seeded_state:?}: it waited in the session's line"
    );
    let failure = match settled {
        Err(failure) => failure,
        Ok(waited) => panic!("{seeded_state:?}: the entry ends not sent: {waited:?}"),
    };
    assert_eq!(
        String::from(failure.message.clone()),
        "not sent: an earlier outcome is unknown",
        "{seeded_state:?}: {failure:?}"
    );
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    let queued_shown = queued_shown.expect("the entry is on record");
    assert_eq!(queued_shown.stage, ProviderOperationStage::Terminal);
    assert_eq!(queued_shown.effect, ProviderOperationEffect::None);
    assert_eq!(
        stopped.recorded("_session/steering", None),
        0,
        "{seeded_state:?}: zero steer"
    );
    assert_eq!(
        stopped.recorded("session/prompt", Some(CONTROL_TEXT)),
        0,
        "{seeded_state:?}: zero prompt"
    );
    assert_eq!(
        stopped.recorded("session/prompt", None),
        1,
        "{seeded_state:?}: only the held turn's prompt"
    );
    assert!(stopped.held_ended, "{seeded_state:?}: the held turn ended");
    stopped
        .stopped
        .unwrap_or_else(|error| panic!("{seeded_state:?}: supervisor shutdown: {error}"));
}

/// Oracle: D1 V22 (R12, C2) — "Controls, each with zero steer and zero
/// prompt at the fixture and the uncertainty refusal: an unresolved, non-live
/// unknown row", at a fresh API send (Deferred admission).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_send_refused_for_a_non_live_unknown_row() {
    assert_fresh_send_refused(SeededUncertainty::NonLiveUnknownRow).await;
}

/// Oracle: D1 V22 (R12, C2) — "a same-process unknown failure whose completed
/// entry is still retained in live_operations, followed at once by a new
/// send", with zero steer, zero prompt and the uncertainty refusal, at a
/// fresh API send (Deferred admission).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_send_refused_for_a_retained_published_unknown() {
    assert_fresh_send_refused(SeededUncertainty::PublishedUnknownRetained).await;
}

/// Oracle: D1 V22 (R12, C2) — "the moment after a terminal unknown row is
/// stored and before its result is published", with zero steer, zero prompt
/// and the uncertainty refusal, at a fresh API send (Deferred admission).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn fresh_send_refused_between_unknown_stored_and_published() {
    assert_fresh_send_refused(SeededUncertainty::StoredBeforePublished).await;
}

/// Oracle: D1 V22 (R12, C2) — "an unresolved, non-live unknown row ... The
/// same three run at queued dispatch, where the entry ends not sent."
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_entry_not_sent_for_a_non_live_unknown_row() {
    assert_queued_entry_not_sent(SeededUncertainty::NonLiveUnknownRow).await;
}

/// Oracle: D1 V22 (R12, C2) — "a same-process unknown failure whose completed
/// entry is still retained in live_operations ... The same three run at
/// queued dispatch, where the entry ends not sent."
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_entry_not_sent_for_a_retained_published_unknown() {
    assert_queued_entry_not_sent(SeededUncertainty::PublishedUnknownRetained).await;
}

/// Oracle: D1 V22 (R12, C2) — "the moment after a terminal unknown row is
/// stored and before its result is published ... The same three run at
/// queued dispatch, where the entry ends not sent."
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn queued_entry_not_sent_between_unknown_stored_and_published() {
    assert_queued_entry_not_sent(SeededUncertainty::StoredBeforePublished).await;
}
