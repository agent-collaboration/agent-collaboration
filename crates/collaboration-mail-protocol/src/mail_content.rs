//! E30 content is independent of custody, transport and local scheduling.
use crate::{
    Excerpt, GenerationGuard, MailDeliveryMode, MailItemId, MailValidationError, MembershipId,
    MessageText,
};
use message_board::{
    FeedCursor, Identity, MAX_SUBSCRIPTION_NOTICE_ROOTS, NotifyIntentId, ProjectId, RecordRef,
    SessionRef, SubscriptionGeneration, ThreadId,
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
    Wake(crate::WakeBody),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum MailKind {
    Message,
    Notification,
    Cancel,
    Wake,
}
impl MailBody {
    #[must_use]
    pub const fn kind(&self) -> MailKind {
        match self {
            Self::Message(_) => MailKind::Message,
            Self::Notification(_) => MailKind::Notification,
            Self::Cancel(_) => MailKind::Cancel,
            Self::Wake(_) => MailKind::Wake,
        }
    }
    pub(crate) fn validate_sender(&self, sender: &MailSender) -> Result<(), MailValidationError> {
        match self {
            Self::Message(body) => body.validate_sender(sender),
            Self::Notification(body) => body.validate_sender(sender),
            Self::Wake(body) => body.validate_sender(sender),
            Self::Cancel(body) => {
                let valid = matches!(
                    (sender, body),
                    (
                        MailSender::Session(_) | MailSender::HumanUser { .. },
                        CancelBody {
                            target: CancelTarget::Item(_),
                            cause: CancelCause::BySender {},
                        },
                    ) | (
                        MailSender::Project { .. },
                        CancelBody {
                            cause: CancelCause::ByRequester { .. } | CancelCause::BoardDerived {},
                            ..
                        },
                    )
                );
                valid
                    .then_some(())
                    .ok_or(MailValidationError { field: "sender" })
            }
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MessageBody {
    pub text: MessageText,
    pub delivery: MailDeliveryMode,
    pub guard: Option<GenerationGuard>,
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(required)]
    pub sender_display_name: Option<crate::SessionDisplayName>,
    /// The answered mail item; an ordinary message has no reply link.
    #[serde(deserialize_with = "Option::deserialize")]
    #[schemars(required)]
    pub reply_to: Option<crate::MailItemId>,
}
impl MessageBody {
    pub fn validate_sender(&self, sender: &MailSender) -> Result<(), MailValidationError> {
        matches!(
            sender,
            MailSender::Session(_) | MailSender::HumanUser { .. }
        )
        .then_some(())
        .ok_or(MailValidationError { field: "sender" })
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NotificationBody {
    pub excerpt: Excerpt,
    pub pointer: RecordPointer,
    pub source: NotificationSource,
}
impl NotificationBody {
    pub fn validate_sender(&self, sender: &MailSender) -> Result<(), MailValidationError> {
        matches!(sender, MailSender::Project { .. })
            .then_some(())
            .ok_or(MailValidationError { field: "sender" })
    }
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
        #[serde(deserialize_with = "decode_follow_roots")]
        #[schemars(length(max = MAX_SUBSCRIPTION_NOTICE_ROOTS))]
        roots: Vec<RootRange>,
        policy: FollowDeliveryPolicy,
        draining: bool,
    },
}

impl NotificationSource {
    pub fn follow_batch(
        membership: MembershipId,
        activated_at: FeedCursor,
        generation: SubscriptionGeneration,
        policy: FollowDeliveryPolicy,
        batch: &message_board::SubscriptionBatch,
    ) -> Result<Self, MailValidationError> {
        validate_follow_root_count(batch.roots.len())?;
        Ok(Self::FollowBatch {
            membership,
            activated_at,
            generation,
            roots: batch
                .roots
                .iter()
                .map(RootRange::try_from)
                .collect::<Result<_, _>>()?,
            policy,
            draining: batch.draining,
        })
    }
}

fn validate_follow_root_count(count: usize) -> Result<(), MailValidationError> {
    if count > MAX_SUBSCRIPTION_NOTICE_ROOTS {
        return Err(MailValidationError { field: "roots" });
    }
    Ok(())
}

fn decode_follow_roots<'de, D>(decoder: D) -> Result<Vec<RootRange>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let roots = Vec::<RootRange>::deserialize(decoder)?;
    validate_follow_root_count(roots.len()).map_err(serde::de::Error::custom)?;
    Ok(roots)
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
    #[schemars(range(min = 1))]
    post_count: u64,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RootRangeFields {
    root: ThreadId,
    from: FeedCursor,
    through: FeedCursor,
    #[schemars(range(min = 1))]
    post_count: u64,
}
impl TryFrom<RootRangeFields> for RootRange {
    type Error = MailValidationError;
    fn try_from(fields: RootRangeFields) -> Result<Self, Self::Error> {
        Self::new(fields.root, fields.from, fields.through, fields.post_count)
    }
}
impl RootRange {
    pub fn new(
        root: ThreadId,
        from: FeedCursor,
        through: FeedCursor,
        post_count: u64,
    ) -> Result<Self, MailValidationError> {
        if from.epoch > through.epoch || from.position > through.position {
            return Err(MailValidationError { field: "roots" });
        }
        if post_count == 0 {
            return Err(MailValidationError { field: "postCount" });
        }
        Ok(Self {
            root,
            from,
            through,
            post_count,
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
    #[must_use]
    pub fn post_count(&self) -> u64 {
        self.post_count
    }
}

impl TryFrom<&message_board::PendingRootNotice> for RootRange {
    type Error = MailValidationError;
    fn try_from(root: &message_board::PendingRootNotice) -> Result<Self, Self::Error> {
        Self::new(
            root.thread_id.clone(),
            root.from,
            root.through,
            root.post_count,
        )
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
