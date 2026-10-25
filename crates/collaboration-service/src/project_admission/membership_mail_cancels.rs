//! A membership's first end or generation change freezes its cancel cause in the same transaction.
use super::{ProjectAdmissionError, ProjectWriteTransaction};
use message_board::{
    EndReason, MailItemRef, Record, RecordKind, RecordRef, SubscriptionMembership,
};
use message_board_storage::{MailCancelGroup, MailCancelGroupCause, MailHandoffItem};

pub(super) async fn record_membership_cancel(
    write: &mut ProjectWriteTransaction,
    updated: &SubscriptionMembership,
) -> Result<(), ProjectAdmissionError> {
    let reference = RecordRef {
        kind: RecordKind::SubscriptionMembership,
        id: updated.envelope.id.clone(),
    };
    let Some(Record::SubscriptionMembership(previous)) = write.record(&reference).await? else {
        return Ok(());
    };
    if previous.ended().is_some() {
        return Ok(());
    }
    let cause = if let Some(end) = updated.ended() {
        // Resolution retains the reader's interest through its drain (decision 823).
        if end.reason == EndReason::Resolved {
            return Ok(());
        }
        MailCancelGroupCause::MembershipEnded { reason: end.reason }
    } else if previous.generation() != updated.generation() {
        MailCancelGroupCause::GenerationChanged
    } else {
        return Ok(());
    };
    let invalid = || ProjectAdmissionError::InvalidRecord("membership cancel group".into());
    let group = MailCancelGroup {
        id: MailItemRef::try_from(uuid::Uuid::now_v7().to_string()).map_err(|_| invalid())?,
        membership: previous
            .envelope
            .id
            .as_str()
            .to_owned()
            .try_into()
            .map_err(|_| invalid())?,
        activated_at: previous.activated_at(),
        generation: previous.generation(),
        cause,
    };
    write
        .append_mail_handoff(&MailHandoffItem::CancelGroup(group))
        .await?;
    Ok(())
}
