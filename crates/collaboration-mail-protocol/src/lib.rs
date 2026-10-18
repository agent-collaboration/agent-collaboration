//! Portable mail contracts shared by the Router facade and central custody.
mod mail_delivery_mode;
pub use mail_delivery_mode::MailDeliveryMode;
mod generation_guard;
pub use generation_guard::{GenerationGuard, GenerationGuardError};
mod mail_identity;
pub use mail_identity::{
    Excerpt, MailDigest, MailItemId, MailSeq, MailValidationError, MembershipId, MessageText,
    ReportSeq, StatusVersion,
};
mod mail_content;
pub use mail_content::{
    CancelBody, CancelCause, CancelTarget, FollowDeliveryPolicy, InstallationId, MailBody,
    MailKind, MailSender, MessageBody, NotificationBody, NotificationSource, RecordPointer,
    RootRange,
};
mod mail_item;
pub use mail_item::{MailItem, MailItemContent};
mod mail_outcome;
pub use mail_outcome::{
    CancelResult, CarrierAcceptance, MailRefusal, MailStatus, RecipientOutcome, TooLateAt,
};
