use agent_automation::{DurableMessage, ExpiryRule, OperationId, TimingRule, WakeState};
use automation_storage::{AutomationStore, WakeAction, WakeCreate, WakeMutation};
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Message {
    target: String,
    text: String,
}
impl DurableMessage for Message {
    type Target = String;
    type Content = String;
    type Generation = String;
    fn target(&self) -> &String {
        &self.target
    }
    fn content(&self) -> &String {
        &self.text
    }
    fn generation_guard(&self) -> Option<&String> {
        None
    }
    fn delivery_mode(&self) -> &'static str {
        "auto"
    }
}
// U66 / MS-10: valid bounded timing; original interleavings and assertions remain.
#[tokio::test]
async fn attached_wait_observes_pause_even_after_resume() -> Result<(), Box<dyn std::error::Error>>
{
    let path = std::env::temp_dir().join(format!(
        "wake-transition-order-{}.sqlite",
        OperationId::generate().as_str()
    ));
    let mut store = AutomationStore::open(&path).await?;
    let wake = store
        .create_wakeup(&WakeCreate {
            requested_by: collaboration_protocol::WakeCreator::Owner {},
            replace: false,
            operation_id: OperationId::generate(),
            message: Message {
                target: "B".into(),
                text: "Check".into(),
            },
            timing: TimingRule::Interval { seconds: 300 },
            expiry: ExpiryRule::After { seconds: 7200 },
            now_ms: 0,
        })
        .await?;
    for (action, at) in [(WakeAction::Pause, 5000), (WakeAction::Resume, 10000)] {
        store
            .mutate_wakeup::<Message>(&WakeMutation {
                operation_id: OperationId::generate(),
                wakeup_id: wake.definition.wakeup_id.clone(),
                action,
                now_ms: at,
            })
            .await?;
    }
    let current = store
        .read_wakeup::<Message>(&wake.definition.wakeup_id)
        .await?;
    if current.state != WakeState::Active {
        return Err("fixture did not resume".into());
    }
    let transitions = store
        .wake_transitions_after(&wake.definition.wakeup_id, wake.latest_event_sequence)
        .await?;
    let kinds = transitions
        .iter()
        .map(|event| event.kind.as_str())
        .collect::<Vec<_>>();
    if kinds != vec!["paused", "resumed"] {
        return Err("wait lost committed pause after resume".into());
    }
    if !transitions.windows(2).all(|pair| {
        pair.first()
            .zip(pair.last())
            .is_some_and(|(a, b)| a.sequence < b.sequence)
    }) {
        return Err("transition ordering regressed".into());
    }
    store.close().await?;
    std::fs::remove_file(path)?;
    Ok(())
}
