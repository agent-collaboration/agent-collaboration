//! E30 content is independent of custody, transport and local scheduling.
use crate::{
    Excerpt, GenerationGuard, MailDeliveryMode, MailItemId, MailValidationError, MembershipId,
    MessageText,
};
use message_board::{
    FeedCursor, Identity, NotifyIntentId, ProjectId, RecordRef, SessionRef, SubscriptionGeneration,
    ThreadId,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use message_board::ServiceId as InstallationId;
pub use message_board::SubscriptionPolicy as FollowDeliveryPolicy;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MailSender {
    Session(SessionRef),
    #[serde(rename_all = "camelCase")]
    HumanUser {
        installation: InstallationId,
    },
    #[serde(rename_all = "camelCase")]
    Project {
        project: ProjectId,
    },
}
impl MailSender {
    #[must_use]
    pub fn matches_submitting_installation(&self, installation: &InstallationId) -> bool {
        match self {
            Self::Session(session) => session.endpoint.service_id == *installation,
            Self::HumanUser {
                installation: sender,
            } => sender == installation,
            Self::Project { .. } => true,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum MailBody {
    Message(MessageBody),
    Notification(NotificationBody),
    Cancel(CancelBody),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MailKind {
    Message,
    Notification,
    Cancel,
}
impl MailBody {
    #[must_use]
    pub const fn kind(&self) -> MailKind {
        match self {
            Self::Message(_) => MailKind::Message,
            Self::Notification(_) => MailKind::Notification,
            Self::Cancel(_) => MailKind::Cancel,
        }
    }
    pub(crate) fn validate_sender(&self, sender: &MailSender) -> Result<(), MailValidationError> {
        let valid = matches!(
            (sender, self),
            (
                MailSender::Session(_) | MailSender::HumanUser { .. },
                Self::Message(_)
            ) | (MailSender::Project { .. }, Self::Notification(_))
                | (
                    MailSender::Session(_) | MailSender::HumanUser { .. },
                    Self::Cancel(CancelBody {
                        target: CancelTarget::Item(_),
                        cause: CancelCause::BySender {},
                    }),
                )
                | (
                    MailSender::Project { .. },
                    Self::Cancel(CancelBody {
                        cause: CancelCause::ByRequester { .. } | CancelCause::BoardDerived {},
                        ..
                    }),
                )
        );
        if valid {
            Ok(())
        } else {
            Err(MailValidationError { field: "sender" })
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageBody {
    pub text: MessageText,
    pub delivery: MailDeliveryMode,
    pub guard: Option<GenerationGuard>,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationBody {
    pub excerpt: Excerpt,
    pub pointer: RecordPointer,
    pub source: NotificationSource,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum NotificationSource {
    Intent {
        intent: NotifyIntentId,
    },
    #[serde(rename_all = "camelCase")]
    FollowBatch {
        membership: MembershipId,
        activated_at: FeedCursor,
        generation: SubscriptionGeneration,
        roots: Vec<RootRange>,
        policy: FollowDeliveryPolicy,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordPointer {
    pub project: ProjectId,
    pub subject: RecordRef,
    pub position: FeedCursor,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    rename_all = "camelCase",
    deny_unknown_fields,
    try_from = "RootRangeFields"
)]
pub struct RootRange {
    root: ThreadId,
    from: FeedCursor,
    through: FeedCursor,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RootRangeFields {
    root: ThreadId,
    from: FeedCursor,
    through: FeedCursor,
}
impl TryFrom<RootRangeFields> for RootRange {
    type Error = MailValidationError;
    fn try_from(fields: RootRangeFields) -> Result<Self, Self::Error> {
        Self::new(fields.root, fields.from, fields.through)
    }
}
impl RootRange {
    pub fn new(
        root: ThreadId,
        from: FeedCursor,
        through: FeedCursor,
    ) -> Result<Self, MailValidationError> {
        if from.epoch != through.epoch || from.position > through.position {
            return Err(MailValidationError { field: "roots" });
        }
        Ok(Self {
            root,
            from,
            through,
        })
    }
    #[must_use]
    pub fn root(&self) -> &ThreadId {
        &self.root
    }
    #[must_use]
    pub fn from(&self) -> FeedCursor {
        self.from
    }
    #[must_use]
    pub fn through(&self) -> FeedCursor {
        self.through
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CancelBody {
    pub target: CancelTarget,
    pub cause: CancelCause,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "target",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub enum CancelTarget {
    Item(MailItemId),
    #[serde(rename_all = "camelCase")]
    Membership {
        membership: MembershipId,
        activated_at: FeedCursor,
        generation: SubscriptionGeneration,
    },
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum CancelCause {
    BySender {},
    #[serde(rename_all = "camelCase")]
    ByRequester {
        by: Identity,
    },
    BoardDerived {},
}
