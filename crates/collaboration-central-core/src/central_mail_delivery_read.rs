//! Select a page of still-deliverable messages for one live machine link.
use crate::{
    MachineConnectionAttachment, ObjectHost, WorkspaceRuntime, WorkspaceStoreCalls,
    central_mail_records::decode_messages, central_mail_rows::timestamp, guarded_statements::text,
    workspace_decode_error::corrupt, workspace_row_codec::string,
};
use central_storage::{SqlValue, Statement, StoreFailure};
use workspace_ids::MessageId;
use workspace_model::{Timestamp, central_mail::MessageRecord};

pub const WELCOME_PAGE_SIZE: usize = central_engine_model::DueBatchPolicy::DEFAULT.per_kind_cap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WelcomeDeliveryCursor {
    pub created_at: Timestamp,
    pub message_id: MessageId,
}

pub struct WindowedDelivery {
    pub message: MessageRecord,
    pub expires_at: Timestamp,
}

impl WindowedDelivery {
    #[must_use]
    pub fn cursor(&self) -> WelcomeDeliveryCursor {
        WelcomeDeliveryCursor {
            created_at: self.message.created_at,
            message_id: self.message.message_id.clone(),
        }
    }
}

pub enum WelcomeDeliveryPage {
    Ready(Vec<WindowedDelivery>),
    AuthorityLapsed,
}

impl<Host: ObjectHost, Stores: WorkspaceStoreCalls> WorkspaceRuntime<Host, Stores> {
    /// Read the next page of direct and system messages with an open window.
    pub async fn windowed_welcome_page(
        &self,
        attachment: &MachineConnectionAttachment,
        cursor: Option<&WelcomeDeliveryCursor>,
    ) -> Result<WelcomeDeliveryPage, StoreFailure> {
        let _turn = self.take_central_turn().await;
        if self
            .restore_machine_connection_held(attachment)
            .await?
            .is_err()
        {
            return Ok(WelcomeDeliveryPage::AuthorityLapsed);
        }
        let now = self.now()?;
        let (created_at, message_id) = cursor.map_or((SqlValue::Null, SqlValue::Null), |cursor| {
            (text(timestamp(cursor.created_at)), text(&cursor.message_id))
        });
        let limit = i64::try_from(WELCOME_PAGE_SIZE)
            .ok()
            .and_then(|size| size.try_into().ok())
            .ok_or_else(|| corrupt("Welcome page size"))?;
        let rows = self.host.read(Statement {
            sql: "SELECT message_id,sender,target_machine_id,target_session,message_kind,body,state,failure_reason,source_change_id,recipient_actor,source_workspace_id,source_thread_id,created_at,updated_at,delivery_window_ends_at FROM mail_messages WHERE target_machine_id=?1 AND state='queued' AND message_kind IN ('direct','notice','deliveryOutcome') AND delivery_window_ends_at>?2 AND (?3 IS NULL OR (created_at,message_id)>(?3,?4)) ORDER BY created_at,message_id LIMIT ?5",
            args: vec![
                text(attachment.machine_id()), text(timestamp(now)), created_at, message_id,
                SqlValue::Integer(limit),
            ],
        }).await?;
        let page = decode_messages(&rows)?
            .into_iter()
            .zip(&rows.rows)
            .map(|(message, row)| {
                let expires_at: Timestamp = string(&rows, row, "delivery_window_ends_at")?
                    .parse()
                    .map_err(|_| corrupt("mail_messages.delivery_window_ends_at"))?;
                if expires_at <= now {
                    return Err(corrupt("mail_messages.delivery_window_ends_at"));
                }
                Ok(WindowedDelivery {
                    message,
                    expires_at,
                })
            })
            .collect::<Result<Vec<_>, StoreFailure>>()?;
        Ok(WelcomeDeliveryPage::Ready(page))
    }
}
