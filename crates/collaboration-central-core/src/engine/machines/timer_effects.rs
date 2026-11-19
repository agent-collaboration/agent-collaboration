use central_engine_model::{Context, RefusalTag, TransitionError};
use workspace_model::central_mail as mail;

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) enum TimerRefusal {
    AlreadyFinished,
    InvalidTiming,
    InvalidIdentifier,
}
impl RefusalTag for TimerRefusal {
    fn refusal_tag(&self) -> &'static str {
        match self {
            Self::AlreadyFinished => "already_finished",
            Self::InvalidTiming => "invalid_timing",
            Self::InvalidIdentifier => "invalid_identifier",
        }
    }
}

/// The id source gave something other than a UUIDv7 for a new message.
#[derive(Debug)]
pub(crate) struct NotUuidV7;
impl From<NotUuidV7> for TransitionError<TimerRefusal> {
    fn from(_: NotUuidV7) -> Self {
        TransitionError::Domain(TimerRefusal::InvalidIdentifier)
    }
}

/// A queued message from central, minted with the next id from the context.
pub(crate) fn queue_message(
    context: &mut Context<'_>,
    target: workspace_ids::SessionRef,
    body: mail::MessageBody,
) -> Result<mail::MessageRecord, NotUuidV7> {
    let id = context.ids.next_id();
    if id.get_version_num() != 7 {
        return Err(NotUuidV7);
    }
    Ok(mail::MessageRecord {
        message_id: id.to_string().try_into().map_err(|_| NotUuidV7)?,
        sender: mail::MessageSender::Central,
        target,
        body,
        state: mail::MessageState::Queued,
        source: None,
        created_at: context.now,
        updated_at: context.now,
    })
}
