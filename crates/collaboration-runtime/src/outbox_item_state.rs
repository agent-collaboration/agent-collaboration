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
    /// Ended at its deadline (outbox_deadline) with its text gone: a direct
    /// message that never left the Mac within 15 minutes.
    Cancelled,
    /// A direct message that left the Mac but was not confirmed within 15
    /// minutes; it may have been delivered.
    Unconfirmed,
    /// Any other write that never left the Mac within 24 hours.
    GaveUpUnsent,
    /// Any other write that left the Mac but was not confirmed within 24
    /// hours; it may have been applied.
    GaveUpUnconfirmed,
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
            Self::Cancelled => "cancelled",
            Self::Unconfirmed => "unconfirmed",
            Self::GaveUpUnsent => "gaveUpUnsent",
            Self::GaveUpUnconfirmed => "gaveUpUnconfirmed",
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
            "cancelled" => Some(Self::Cancelled),
            "unconfirmed" => Some(Self::Unconfirmed),
            "gaveUpUnsent" => Some(Self::GaveUpUnsent),
            "gaveUpUnconfirmed" => Some(Self::GaveUpUnconfirmed),
            _ => None,
        }
    }

    /// The stored state for a write that ended at its deadline.
    pub(crate) const fn ended(reason: OutboxCancellation) -> Self {
        match reason {
            OutboxCancellation::NotDeliveredWithin15Minutes => Self::Cancelled,
            OutboxCancellation::NotConfirmedWithin15Minutes => Self::Unconfirmed,
            OutboxCancellation::NotSentWithin24Hours => Self::GaveUpUnsent,
            OutboxCancellation::NotConfirmedWithin24Hours => Self::GaveUpUnconfirmed,
        }
    }

    /// Why a write in this ended state ended; None for any other state.
    pub(crate) const fn end_reason(self) -> Option<OutboxCancellation> {
        match self {
            Self::Cancelled => Some(OutboxCancellation::NotDeliveredWithin15Minutes),
            Self::Unconfirmed => Some(OutboxCancellation::NotConfirmedWithin15Minutes),
            Self::GaveUpUnsent => Some(OutboxCancellation::NotSentWithin24Hours),
            Self::GaveUpUnconfirmed => Some(OutboxCancellation::NotConfirmedWithin24Hours),
            Self::InOutbox
            | Self::SentAnswerLost
            | Self::Sent
            | Self::Resent
            | Self::Answered
            | Self::Refused => None,
        }
    }
}
