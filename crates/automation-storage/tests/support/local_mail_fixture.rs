//! Shared valid Local custody fixtures; no admission policy is reproduced.
use automation_storage::LocalMessageRequest;
use chrono::Utc;
use collaboration_mail_protocol::MailItemId;
use collaboration_protocol::{
    DeliveryOutcome, DeliveryReceipt, MessageDelivery, PushHeaderFacts, PushId, PushKind,
    PushOrigin, PushRecordDraft, SessionRef,
};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::path::PathBuf;
pub(super) struct Fixture {
    pub(super) path: PathBuf,
}
impl Fixture {
    pub(super) fn new() -> Self {
        Self {
            path: std::env::temp_dir().join(format!("local-mail-{}.sqlite", uuid::Uuid::now_v7())),
        }
    }
    pub(super) async fn observer(&self) -> Result<SqliteConnection, sqlx::Error> {
        SqliteConnection::connect_with(&SqliteConnectOptions::new().filename(&self.path)).await
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
pub(super) fn request(
    created_at: chrono::DateTime<Utc>,
) -> Result<LocalMessageRequest, Box<dyn std::error::Error>> {
    let target: SessionRef = serde_json::from_value(
        serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"local-mail-target"}),
    )?;
    Ok(LocalMessageRequest {
        draft: PushRecordDraft {
            push_id: PushId::try_from(uuid::Uuid::now_v7().to_string())?,
            kind: PushKind::DirectMessage,
            origin: PushOrigin::OwnerUnverified,
            origin_router_ref: None,
            target,
            reply_to_push_id: None,
            header_facts: PushHeaderFacts::DirectMessage {
                sender_display_name: None,
            },
            body: Some("Full content beyond the preview: ".repeat(30)),
            activity: None,
            mode: Some(MessageDelivery::Auto),
            guard: None,
            created_at,
        },
        target_identity: "original target label".into(),
    })
}
pub(super) fn accepted() -> DeliveryReceipt {
    DeliveryReceipt {
        outcome: DeliveryOutcome::PeerMessageWritten,
        reachability: None,
        client: None,
    }
}
pub(super) fn mail_id(
    request: &LocalMessageRequest,
) -> Result<MailItemId, Box<dyn std::error::Error>> {
    Ok(request.draft.push_id.clone().try_into()?)
}
