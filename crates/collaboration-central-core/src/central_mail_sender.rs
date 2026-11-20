//! Resolve display identity from current organization rows when delivering mail.
use crate::WorkspaceRuntime;
use central_storage::{Statement, StoreFailure};
use fleet_runtime_ports::{CentralStore, WorkspaceStores};
use fleet_runtime_ports::{
    guarded_statements::text, stored_values::string, workspace_decode_error::corrupt,
};
use workspace_model::{
    Identity, ResolvedSender,
    central_mail::{MessageBody, MessageRecord, MessageSender},
};

impl<Host: CentralStore, Stores: WorkspaceStores> WorkspaceRuntime<Host, Stores> {
    pub(crate) async fn resolve_message_sender(
        &self,
        message: &MessageRecord,
    ) -> Result<Option<ResolvedSender>, StoreFailure> {
        let central_authored = !matches!(message.body, MessageBody::Direct { .. });
        let identity = match &message.sender {
            MessageSender::Central => None,
            MessageSender::Actor(actor) => self.read_actor_identity(actor).await?,
        };
        let sender = match identity {
            Some(Identity::AgentSession { session }) => {
                let rows = self
                    .execution
                    .statement_host()
                    .read(Statement {
                        sql: "SELECT label FROM login_machines WHERE machine_id=?",
                        args: vec![text(&session.machine_id)],
                    })
                    .await?;
                let machine_label = match rows.rows.as_slice() {
                    [] => None,
                    [row] => Some(
                        string(&rows, row, "label")?
                            .to_owned()
                            .try_into()
                            .map_err(|_| corrupt("login_machines.label"))?,
                    ),
                    _ => return Err(corrupt("login_machines.machine_id")),
                };
                Some(ResolvedSender::Session {
                    session,
                    display_name: None,
                    machine_label,
                })
            }
            Some(Identity::Person { person_id }) => Some(ResolvedSender::Person {
                person_id,
                display_name: None,
            }),
            None | Some(Identity::RouterComponent { .. }) => {
                central_authored.then_some(ResolvedSender::Central {})
            }
        };
        Ok(sender)
    }
}
