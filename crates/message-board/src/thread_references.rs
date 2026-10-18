//! Command routing references are separate from IDs in persisted conversation records.
use crate::{ProjectId, ReadScope, SubscriptionScope, TextValidationError, Thread, ThreadId};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThreadRef {
    pub thread: ThreadId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectId>,
}
impl From<ThreadId> for ThreadRef {
    fn from(thread: ThreadId) -> Self {
        Self {
            thread,
            project: None,
        }
    }
}
impl std::str::FromStr for ThreadRef {
    type Err = TextValidationError;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let invalid = || TextValidationError {
            field: "thread",
            requirement: "must be a thread UUID or project UUID/thread UUID",
        };
        match value.split_once('/') {
            Some((project, thread)) => Ok(Self {
                thread: ThreadId::try_from(thread.to_owned()).map_err(|_| invalid())?,
                project: Some(ProjectId::try_from(project.to_owned()).map_err(|_| invalid())?),
            }),
            None => Ok(ThreadId::try_from(value.to_owned())
                .map_err(|_| invalid())?
                .into()),
        }
    }
}
impl std::fmt::Display for ThreadRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.project {
            Some(project) => write!(formatter, "{}/{}", project.as_str(), self.thread.as_str()),
            None => formatter.write_str(self.thread.as_str()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PostTarget {
    Thread { thread: ThreadRef },
}

#[derive(
    Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum SubscriptionTarget {
    Project { project_id: ProjectId },
    Thread { thread: ThreadRef },
}
impl SubscriptionTarget {
    #[must_use]
    pub fn stored_scope(&self) -> SubscriptionScope {
        match self {
            Self::Project { project_id } => SubscriptionScope::project(project_id.clone()),
            Self::Thread { thread } => SubscriptionScope::thread(thread.thread.clone()),
        }
    }
    #[must_use]
    pub fn kind_and_id(&self) -> (&'static str, &str) {
        match self {
            Self::Project { project_id } => ("project", project_id.as_str()),
            Self::Thread { thread } => ("thread", thread.thread.as_str()),
        }
    }
    #[must_use]
    pub fn qualified(mut self, project: ProjectId) -> Self {
        if let Self::Thread { thread } = &mut self {
            thread.project = Some(project);
        }
        self
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ReadScopeRef {
    Project { project_id: ProjectId },
    Thread { thread: ThreadRef },
}
impl ReadScopeRef {
    #[must_use]
    pub fn stored_scope(&self) -> ReadScope {
        match self {
            Self::Project { project_id } => ReadScope::Project {
                project_id: project_id.clone(),
            },
            Self::Thread { thread } => ReadScope::Thread {
                thread_id: thread.thread.clone(),
            },
        }
    }
}

/// Read results label the owning project without changing a stored Thread record.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReadRecord {
    pub project_id: ProjectId,
    #[serde(flatten)]
    pub thread: Thread,
    pub subject: crate::ThreadSubjectView,
}

impl From<ReadScope> for ReadScopeRef {
    fn from(scope: ReadScope) -> Self {
        match scope {
            ReadScope::Project { project_id } => Self::Project { project_id },
            ReadScope::Thread { thread_id } => Self::Thread {
                thread: thread_id.into(),
            },
        }
    }
}
impl From<SubscriptionScope> for SubscriptionTarget {
    fn from(scope: SubscriptionScope) -> Self {
        match scope {
            SubscriptionScope::Project { project_id } => Self::Project { project_id },
            SubscriptionScope::Thread { thread_id } => Self::Thread {
                thread: thread_id.into(),
            },
        }
    }
}
