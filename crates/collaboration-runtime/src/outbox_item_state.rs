//! The outbox row's `item_state`, validated in Rust (no SQL enum constraint):
//! where a write stands, and for an ended write why it ended.
use central_link_client::OutboxCancellation;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutboxItemState {
    InOutbox,
    /// Went out in a batch whose answer was lost; back in the outbox to be
    /// resent with the same id, and may already be in central.
    SentAnswerLost,
    /// On the wire for the first time: claimed from InOutbox. If central
    /// says it did not apply this attempt, it never reached central.
    Sent,
    /// On the wire again: claimed from SentAnswerLost, so an earlier attempt
    /// may already be in central whatever this attempt's answer (Lead A,
    /// 2026-11-18).
    Resent,
    Answered,
    Refused,
    /// Ended at its deadline (outbox_deadline) with its text gone: it never
    /// left the Mac within 15 minutes.
    NotSent,
    /// Ended at its deadline with its text gone: it left the Mac but was not
    /// confirmed within 15 minutes, so it may have been applied.
    Unconfirmed,
}

impl OutboxItemState {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::InOutbox => "inOutbox",
            Self::SentAnswerLost => "sentAnswerLost",
            Self::Sent => "sent",
            Self::Resent => "resent",
            Self::Answered => "answered",
            Self::Refused => "refused",
            Self::NotSent => "notSent",
            Self::Unconfirmed => "unconfirmed",
        }
    }

    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "inOutbox" => Some(Self::InOutbox),
            "sentAnswerLost" => Some(Self::SentAnswerLost),
            "sent" => Some(Self::Sent),
            "resent" => Some(Self::Resent),
            "answered" => Some(Self::Answered),
            "refused" => Some(Self::Refused),
            "notSent" => Some(Self::NotSent),
            "unconfirmed" => Some(Self::Unconfirmed),
            _ => None,
        }
    }

    /// The stored state for a write that ended at its deadline.
    pub(crate) const fn ended(reason: OutboxCancellation) -> Self {
        match reason {
            OutboxCancellation::NotSentWithin15Minutes => Self::NotSent,
            OutboxCancellation::NotConfirmedWithin15Minutes => Self::Unconfirmed,
        }
    }

    /// Why a write in this ended state ended; None for any other state.
    pub(crate) const fn end_reason(self) -> Option<OutboxCancellation> {
        match self {
            Self::NotSent => Some(OutboxCancellation::NotSentWithin15Minutes),
            Self::Unconfirmed => Some(OutboxCancellation::NotConfirmedWithin15Minutes),
            Self::InOutbox
            | Self::SentAnswerLost
            | Self::Sent
            | Self::Resent
            | Self::Answered
            | Self::Refused => None,
        }
    }
}
