//! Central-owned statement construction and validated mail rows.
use crate::guarded_statements::{any, text};
use crate::{ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls, workspace_decode_error::corrupt};
use central_storage::{
    AlarmUpdate, BatchOutcome, GuardedStatement, SqlValue, Statement, StoreFailure,
};
use collaboration_protocol::central_mail as mail;
use serde::Serialize;
use workspace_model::{ActorIdentityId, SessionAddress, Timestamp};
pub(crate) fn encode(value: &impl Serialize) -> Result<SqlValue, StoreFailure> {
    serde_json::to_string(value)
        .map(text)
        .map_err(|_| corrupt("mail JSON"))
}
pub(crate) fn timestamp(value: Timestamp) -> String {
    value.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
pub const EARLIEST_DUE: &str = "SELECT CASE WHEN COUNT(*) != COUNT(unixepoch(due_at)) THEN 'invalid_due_at' ELSE MIN(unixepoch(due_at)*1000+CAST(substr(strftime('%f',due_at),4,3) AS INTEGER)) END AS earliest_due FROM (SELECT due_at FROM wakes WHERE state='active' UNION ALL SELECT next_due_at FROM automations WHERE state='active' AND next_due_at IS NOT NULL UNION ALL SELECT due_at FROM notification_windows UNION ALL SELECT drain_due_at FROM workspace_drains WHERE drain_due_at IS NOT NULL) WHERE (SELECT COUNT(*) FROM organization_identity)=1";
pub(crate) fn alarm_update() -> AlarmUpdate {
    AlarmUpdate::SetFromQuery(Statement {
        sql: EARLIEST_DUE,
        args: vec![],
    })
}
pub(crate) fn applied(outcome: BatchOutcome) -> Result<(), StoreFailure> {
    match outcome {
        BatchOutcome::Applied { .. } => Ok(()),
        _ => Err(StoreFailure::Unavailable),
    }
}
pub(crate) fn insert_message(
    message: &mail::MessageRecord,
) -> Result<GuardedStatement, StoreFailure> {
    let kind = match message.body {
        mail::MessageBody::Direct { .. } => "direct",
        mail::MessageBody::Notice { .. } => "notice",
        mail::MessageBody::Wake { .. } => "wake",
        mail::MessageBody::Automation { .. } => "automation",
    };
    let (state, failure) = match &message.state {
        mail::MessageState::Queued => ("queued", SqlValue::Null),
        mail::MessageState::Delivered => ("delivered", SqlValue::Null),
        mail::MessageState::Cancelled => ("cancelled", SqlValue::Null),
        mail::MessageState::Failed { reason } => ("failed", text(reason)),
    };
    let source = message.source.as_ref();
    Ok(any(
        "INSERT INTO messages(message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?) ON CONFLICT(source_change_id,recipient_actor) WHERE source_change_id IS NOT NULL DO NOTHING",
        vec![
            text(&message.message_id),
            encode(&message.sender)?,
            text(&message.target.machine_id),
            encode(&message.target)?,
            text(kind),
            encode(&message.body)?,
            text(state),
            failure,
            source.map_or(SqlValue::Null, |source| text(&source.change_id)),
            source.map_or(SqlValue::Null, |source| text(&source.recipient_actor)),
            source.map_or(SqlValue::Null, |source| text(&source.workspace_id)),
            source
                .and_then(|source| source.thread_id.as_ref())
                .map_or(SqlValue::Null, text),
            text(timestamp(message.created_at)),
            text(timestamp(message.updated_at)),
        ],
    ))
}
impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    pub(crate) fn queued_message(
        &self,
        sender: mail::MessageSender,
        target: SessionAddress,
        body: mail::MessageBody,
        source: Option<mail::NoticeSource>,
        now: Timestamp,
    ) -> Result<mail::MessageRecord, StoreFailure> {
        Ok(mail::MessageRecord {
            message_id: self.mint()?,
            sender,
            target,
            body,
            state: mail::MessageState::Queued,
            source,
            created_at: now,
            updated_at: now,
        })
    }
    pub(crate) async fn known_session(
        &self,
        target: &SessionAddress,
    ) -> Result<bool, StoreFailure> {
        let identity =
            serde_json_canonicalizer::to_string(&workspace_model::Identity::AgentSession {
                session: target.clone(),
            })
            .map_err(|_| corrupt("session identity"))?;
        let rows=self.host.read(Statement{sql:"SELECT a.actor_identity_id FROM actor_identities a JOIN machines m ON m.machine_id=? WHERE a.identity_kind='agentSession' AND a.identity=? AND m.state='enrolled'",args:vec![text(&target.machine_id),text(identity)]}).await?;
        Ok(!rows.rows.is_empty())
    }
    pub(crate) async fn route_notice_actor(
        &self,
        actor: &ActorIdentityId,
    ) -> Result<Option<SessionAddress>, StoreFailure> {
        let actors = self.actor_views(std::slice::from_ref(actor)).await?;
        match actors.first().map(|actor| &actor.identity) {
            Some(workspace_model::Identity::AgentSession { session }) => Ok(Some(session.clone())),
            Some(
                workspace_model::Identity::Person { .. }
                | workspace_model::Identity::RouterComponent { .. },
            ) => Ok(None),
            None => Err(corrupt("notice recipient")),
        }
    }
}
