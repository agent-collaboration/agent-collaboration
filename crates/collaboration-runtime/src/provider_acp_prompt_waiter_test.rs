use super::*;
use crate::owner_inbox::{
    HubHistoryAvailability, HubPublication, HubPublicationBaseline, HubPublicationFact,
    HubPublicationRefusal, HubPublicationStamp, HubSessionInbox,
};
use crate::{ProviderOperationStore, ProviderSessionEventHub};
use codex_acp_adapter::bounded_acp_output;
use std::num::NonZeroUsize;
use uuid::Uuid;

const TURN: &str = "held-turn";

fn commit(incarnation: Uuid, sequence: u64, event: SessionEvent) -> HubPublication {
    HubPublication::Commit {
        stamp: HubPublicationStamp {
            incarnation,
            sequence,
        },
        facts: vec![HubPublicationFact::Event(event)],
        observed_at_seconds: 0,
    }
}

fn rebase(incarnation: Uuid, sequence: u64, state: SessionState) -> HubPublication {
    HubPublication::Rebase {
        stamp: HubPublicationStamp {
            incarnation,
            sequence,
        },
        baseline: Box::new(HubPublicationBaseline {
            turn_running: state == SessionState::Running,
            state,
            capabilities: Some(CapabilityReport::default()),
            settings: None,
            skills: session_event_model::SkillView::Unknown,
            history: HubHistoryAvailability::Unavailable,
        }),
    }
}

async fn apply(inbox: &HubSessionInbox, publication: HubPublication) {
    inbox
        .try_offer(publication)
        .expect("enqueue batch")
        .await
        .expect("batch receipt")
        .expect("apply batch");
}

fn terminal() -> SessionEvent {
    SessionEvent::TurnEnded {
        turn_id: TURN.to_owned(),
        outcome: TurnOutcome::Ended {
            stop_reason: StopReason::MaxTokens,
            local_cause: None,
        },
    }
}

async fn busy_prompt_rebase_reply(retains_terminal: bool) -> Value {
    let root = tempfile::tempdir().expect("private root");
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("store");
    // The retained-terminal case loses the marker to receiver lag. The other
    // case receives the marker from its original attachment.
    let subscriber_capacity = if retains_terminal { 1 } else { 256 };
    let hub = Arc::new(ProviderSessionEventHub::with_capacity(
        Arc::new(tokio::sync::Mutex::new(store)),
        subscriber_capacity,
    ));
    let session: SessionRef = serde_json::from_value(json!({
        "endpoint": {"serviceId": Uuid::now_v7(), "endpointId": "cursor-local"},
        "sessionId": Uuid::now_v7(),
    }))
    .expect("session");
    let incarnation = Uuid::now_v7();
    let stop = CancellationToken::new();
    let (inbox, worker) = hub.owner_inbox(session.clone(), NonZeroUsize::MIN);
    inbox
        .install_incarnation(None, incarnation)
        .expect("install owner");
    let worker = tokio::spawn(worker.expect("inbox worker").run(stop.clone()));
    apply(&inbox, rebase(incarnation, 1, SessionState::Idle)).await;
    let (output, mut replies) = bounded_acp_output(stop.clone());
    let mut observers = ProviderSessionObservers::new(hub.clone(), output.clone(), false);
    observers
        .attach(session.clone(), false, false)
        .await
        .expect("connection observer");
    apply(
        &inbox,
        commit(
            incarnation,
            2,
            SessionEvent::TurnStarted {
                turn_id: TURN.to_owned(),
                input_id: session_event_model::InputId::generate(),
            },
        ),
    )
    .await;
    let attachment = hub
        .attach(session.clone())
        .await
        .expect("busy prompt subscription");
    let observed = observers
        .observed_events
        .get(&session)
        .expect("connection observer")
        .clone();
    let mut waiter = std::pin::pin!(stream_prompt(
        output,
        json!(4),
        TURN.to_owned(),
        hub.as_ref(),
        session.clone(),
        attachment,
        observed
    ));
    assert!(
        futures_util::poll!(&mut waiter).is_pending(),
        "busy prompt waits on its turn"
    );

    let held = hub.hold_history_for_test(session.clone()).await;
    let buffered = inbox
        .try_offer(commit(
            incarnation,
            3,
            SessionEvent::StateChanged {
                state: SessionState::Running,
            },
        ))
        .expect("fill the one-slot inbox while history is held");
    assert!(matches!(
        inbox.try_offer(commit(incarnation, 4, terminal())),
        Err(HubPublicationRefusal::Full)
    ));
    drop(held);
    buffered
        .await
        .expect("buffered receipt")
        .expect("buffered batch applied");
    let state = if retains_terminal {
        SessionState::Running
    } else {
        SessionState::Idle
    };
    apply(&inbox, rebase(incarnation, 5, state)).await;
    if retains_terminal {
        apply(&inbox, commit(incarnation, 6, terminal())).await;
    }

    waiter.await;
    let reply = replies.recv().await.expect("ACP response");
    assert_eq!(reply.get("id"), Some(&json!(4)));
    assert!(replies.try_recv().is_err(), "prompt answered exactly once");
    stop.cancel();
    worker.await.expect("inbox shutdown");
    (*reply).clone()
}

#[tokio::test(flavor = "current_thread")]
async fn busy_prompt_recovers_retained_terminal_after_full_inbox_rebase() {
    let reply = busy_prompt_rebase_reply(true).await;
    assert_eq!(
        reply.pointer("/result/stopReason"),
        Some(&json!("max_tokens")),
        "{reply}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn busy_prompt_reports_unknown_when_full_inbox_rebase_loses_terminal() {
    let reply = busy_prompt_rebase_reply(false).await;
    assert_eq!(
        reply.pointer("/error/message"),
        Some(&json!(
            "the turn's outcome couldn't be read from the event stream; check the session before sending again"
        )),
        "{reply}"
    );
}
