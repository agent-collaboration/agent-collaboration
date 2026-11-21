//! Shared values for mailbox delivery over a machine link.
use workspace_ids::MessageId;
use workspace_model::{ResolvedSender, Timestamp, central_mail::MessageRecord};

/// The result of applying an Ack to the central message row.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailAckSettlement {
    Applied,
    Ignored(MailAckIgnored),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MailAckIgnored {
    UnknownMessage,
    WrongMachine,
    AlreadySettled,
    MachineAuthorityLapsed,
}

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

/// The row and its current sender at one authorized push to the addressed machine.
pub struct ResolvedMailDelivery {
    pub message: Box<MessageRecord>,
    pub sender: ResolvedSender,
    pub expires_at: Timestamp,
}

pub enum WindowedDeliveryDecision {
    Ready(ResolvedMailDelivery),
    Ineligible,
    /// Central terminally settled the message without offering a frame.
    SenderUnresolved,
    AuthorityLapsed,
}
