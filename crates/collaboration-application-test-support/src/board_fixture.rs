//! Shared board test helpers that need no central.
use message_board::ServiceId;
use serde_json::{Value, json};

pub use crate::native_mail_owner;
pub use crate::native_registry;
pub type TestError = Box<dyn std::error::Error + Send + Sync>;

pub fn fixture_peer() -> Result<fleet_sync_protocol::PeerId, TestError> {
    native_registry::fixture_peer()
}
// The shared claim fixtures supply a trusted enrollment, not a directory implementation.
pub fn intent_handoff_input() -> Result<Option<collaboration_service::IntentHandoffInput>, TestError>
{
    Ok(Some(collaboration_service::IntentHandoffInput {
        item: fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7())?,
        enrollment: collaboration_service::IntentRecipientEnrollment {
            service: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            peer: fixture_peer()?,
        },
    }))
}
pub fn accepted(value: &Value) {
    assert_eq!(
        value.pointer("/result/result/decision/kind"),
        Some(&json!("accepted")),
        "{value}"
    )
}
pub fn refusal_kind(value: &Value) -> Option<&str> {
    let reason = value.pointer("/result/result/decision/reasons/0")?;
    match reason.get("kind").and_then(Value::as_str)? {
        "task" | "workstream" | "trail" | "role" | "thread" => {
            reason.pointer("/reason/kind").and_then(Value::as_str)
        }
        kind => Some(kind),
    }
}
