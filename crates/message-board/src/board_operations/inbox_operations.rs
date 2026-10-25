use crate::{
    ActingForIdentity, Identity, InboxPage, InboxReadMode, InboxScope, Page, PageRequest,
    ReadScopeRef, WorkspaceUnreadSummary,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! contract {
    ($name:ident { $($(#[$meta:meta])* $field:ident : $ty:ty),* $(,)? }) => {
        #[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        pub struct $name { $($(#[$meta])* pub $field: $ty),* }
    };
}

contract!(ReadInitializationEntry {
    project_id: crate::ProjectId,
    request_id: crate::RequestId,
});
contract!(ReadInitializationOutcome {
    project_id: crate::ProjectId,
    request_id: crate::RequestId,
    outcome: ProjectWriteCallResult,
});
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InboxFetchRequestProps {
    pub protocol_version: crate::ProtocolVersion,
    pub acting_for: Option<ActingForIdentity>,
    pub reader: Identity,
    pub scope: InboxScope,
    #[serde(default)]
    pub read_mode: InboxReadMode,
    pub initializations: Vec<ReadInitializationEntry>,
    pub page: PageRequest,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(try_from = "InboxFetchRequestProps", into = "InboxFetchRequestProps")]
pub struct InboxFetchRequest {
    pub protocol_version: crate::ProtocolVersion,
    pub acting_for: Option<ActingForIdentity>,
    pub reader: Identity,
    pub scope: InboxScope,
    pub read_mode: InboxReadMode,
    initializations: Vec<ReadInitializationEntry>,
    pub page: PageRequest,
}
impl InboxFetchRequest {
    pub fn new(props: InboxFetchRequestProps) -> Result<Self, crate::TextValidationError> {
        if props.initializations.len()
            > usize::try_from(crate::MAX_PAGE_LIMIT).map_err(|_| crate::TextValidationError {
                field: "initializations",
                requirement: "must fit the page bound",
            })?
            || props.read_mode == InboxReadMode::Latest && !props.initializations.is_empty()
            || props
                .initializations
                .iter()
                .enumerate()
                .any(|(index, entry)| {
                    props
                        .initializations
                        .iter()
                        .take(index)
                        .any(|prior| prior.project_id == entry.project_id)
                })
        {
            return Err(crate::TextValidationError {
                field: "initializations",
                requirement: "must be distinct projects within the page bound and only used for unread fetch",
            });
        }
        Ok(Self {
            protocol_version: props.protocol_version,
            acting_for: props.acting_for,
            reader: props.reader,
            scope: props.scope,
            read_mode: props.read_mode,
            initializations: props.initializations,
            page: props.page,
        })
    }
    #[must_use]
    pub fn initializations(&self) -> &[ReadInitializationEntry] {
        &self.initializations
    }
}
impl TryFrom<InboxFetchRequestProps> for InboxFetchRequest {
    type Error = crate::TextValidationError;
    fn try_from(props: InboxFetchRequestProps) -> Result<Self, Self::Error> {
        Self::new(props)
    }
}
impl From<InboxFetchRequest> for InboxFetchRequestProps {
    fn from(request: InboxFetchRequest) -> Self {
        Self {
            protocol_version: request.protocol_version,
            acting_for: request.acting_for,
            reader: request.reader,
            scope: request.scope,
            read_mode: request.read_mode,
            initializations: request.initializations,
            page: request.page,
        }
    }
}
contract!(InboxFetchResult { page: InboxPage });
contract!(AcknowledgementEntry {
    request_id: crate::RequestId,
    scope: ReadScopeRef,
    through: crate::FeedCursor,
});

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InboxAcknowledgeRequestProps {
    pub protocol_version: crate::ProtocolVersion,
    pub actor: Identity,
    pub acting_for: Option<ActingForIdentity>,
    #[schemars(length(min = 1, max = crate::MAX_PAGE_LIMIT))]
    pub entries: Vec<AcknowledgementEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    try_from = "InboxAcknowledgeRequestProps",
    into = "InboxAcknowledgeRequestProps"
)]
pub struct InboxAcknowledgeRequest {
    pub protocol_version: crate::ProtocolVersion,
    pub actor: Identity,
    pub acting_for: Option<ActingForIdentity>,
    entries: Vec<AcknowledgementEntry>,
}

impl InboxAcknowledgeRequest {
    pub fn new(props: InboxAcknowledgeRequestProps) -> Result<Self, crate::TextValidationError> {
        let count = u32::try_from(props.entries.len()).map_err(|_| crate::TextValidationError {
            field: "entries",
            requirement: "must not exceed the aggregate page bound",
        })?;
        if count == 0
            || count > crate::MAX_PAGE_LIMIT
            || props.entries.iter().enumerate().any(|(index, entry)| {
                props
                    .entries
                    .iter()
                    .take(index)
                    .any(|previous| previous.scope.stored_scope() == entry.scope.stored_scope())
            })
        {
            return Err(crate::TextValidationError {
                field: "entries",
                requirement: "must contain at least one entry, at most the page bound, with distinct scopes",
            });
        }
        Ok(Self {
            protocol_version: props.protocol_version,
            actor: props.actor,
            acting_for: props.acting_for,
            entries: props.entries,
        })
    }
    #[must_use]
    pub fn entries(&self) -> &[AcknowledgementEntry] {
        &self.entries
    }
}
impl TryFrom<InboxAcknowledgeRequestProps> for InboxAcknowledgeRequest {
    type Error = crate::TextValidationError;
    fn try_from(props: InboxAcknowledgeRequestProps) -> Result<Self, Self::Error> {
        Self::new(props)
    }
}
impl From<InboxAcknowledgeRequest> for InboxAcknowledgeRequestProps {
    fn from(request: InboxAcknowledgeRequest) -> Self {
        Self {
            protocol_version: request.protocol_version,
            actor: request.actor,
            acting_for: request.acting_for,
            entries: request.entries,
        }
    }
}
contract!(AcknowledgementOutcome {
    request_id: crate::RequestId,
    outcome: ProjectWriteCallResult
});
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProjectWriteCallResult {
    Replied {
        reply: crate::public_replies::WriteReply,
    },
    /// No answer from the project: the write may or may not have applied.
    /// Retry with the same request_id to learn its outcome.
    Unanswered { reason: crate::BoardError },
}
contract!(InboxAcknowledgeReply { outcomes:Vec<AcknowledgementOutcome> });

contract!(InboxWorkspacesRequest {
    reader: Identity,
    #[serde(default)]
    unread_only: bool,
    page: PageRequest
});
contract!(InboxWorkspacesResult { page: Page<WorkspaceUnreadSummary> });
