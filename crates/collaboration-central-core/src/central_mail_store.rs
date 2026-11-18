//! Central-owned statement construction and validated mail rows.
pub(crate) use crate::central_mail_rows::{applied, encode, insert_message, timestamp};
use crate::guarded_statements::text;
use crate::{WorkspaceRuntime, workspace_decode_error::corrupt};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use central_storage::{AlarmUpdate, Statement, StoreFailure};
use workspace_model::central_mail as mail;
use workspace_model::{ActorIdentityId, SessionAddress, Timestamp};
pub(crate) fn alarm_update() -> AlarmUpdate {
    crate::engine::query_registry::alarm_update()
}
impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) fn prepare_message_record(
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
