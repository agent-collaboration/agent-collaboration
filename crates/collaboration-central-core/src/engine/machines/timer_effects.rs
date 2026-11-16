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

pub(crate) fn queue_message(
    context: &mut Context<'_>,
    target: workspace_model::SessionAddress,
    body: mail::MessageBody,
) -> Result<mail::MessageRecord, TransitionError<TimerRefusal>> {
    let id = context.ids.next_id();
    if id.get_version_num() != 7 {
        return Err(TransitionError::Domain(TimerRefusal::InvalidIdentifier));
    }
    Ok(mail::MessageRecord {
        message_id: id
            .to_string()
            .try_into()
            .map_err(|_| TransitionError::Domain(TimerRefusal::InvalidIdentifier))?,
        sender: mail::MessageSender::Central,
        target,
        body,
        state: mail::MessageState::Queued,
        source: None,
        created_at: context.now,
        updated_at: context.now,
    })
}
