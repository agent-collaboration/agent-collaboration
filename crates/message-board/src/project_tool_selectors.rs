//! Named project tools share their accepted operation with schema generation.

use crate::{BoardOperation, TaskAction};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TaskActionSelection {
    Duplicate,
    Replace,
}

impl TaskActionSelection {
    #[must_use]
    pub const fn kind(self) -> &'static str {
        match self {
            Self::Duplicate => "duplicate",
            Self::Replace => "replace",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProjectToolSelection {
    Operation { kind: &'static str },
    TaskAction { action: TaskActionSelection },
    SubtaskCheck { checked: bool },
}

impl ProjectToolSelection {
    #[must_use]
    pub const fn operation_kind(self) -> &'static str {
        match self {
            Self::Operation { kind } => kind,
            Self::TaskAction { .. } => "taskAction",
            Self::SubtaskCheck { .. } => "subtaskCheck",
        }
    }
}

macro_rules! project_tools {
    ($($variant:ident => ($name:literal, $selection:expr, $pattern:pat)),+ $(,)?) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub enum ProjectTool { $($variant),+ }

        impl ProjectTool {
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            #[must_use]
            pub const fn name(self) -> &'static str {
                match self { $(Self::$variant => $name),+ }
            }

            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$variant),)+ _ => None }
            }

            #[must_use]
            pub const fn selection(self) -> ProjectToolSelection {
                match self { $(Self::$variant => $selection),+ }
            }

            #[must_use]
            pub fn accepts(self, operation: &BoardOperation) -> bool {
                match self { $(Self::$variant => matches!(operation, $pattern)),+ }
            }
        }
    };
}

project_tools! {
    WorkstreamCreate => ("workstream_create", ProjectToolSelection::Operation { kind: "workstreamCreate" }, BoardOperation::WorkstreamCreate { .. }),
    WorkstreamRename => ("workstream_rename", ProjectToolSelection::Operation { kind: "workstreamRename" }, BoardOperation::WorkstreamRename { .. }),
    WorkstreamClose => ("workstream_close", ProjectToolSelection::Operation { kind: "workstreamClose" }, BoardOperation::WorkstreamClose { .. }),
    TaskCreate => ("task_create", ProjectToolSelection::Operation { kind: "taskCreate" }, BoardOperation::TaskCreate { .. }),
    TaskUpdate => ("task_update", ProjectToolSelection::Operation { kind: "taskUpdate" }, BoardOperation::TaskUpdate { .. }),
    TaskMoveWorkstream => ("task_move_workstream", ProjectToolSelection::Operation { kind: "taskMoveWorkstream" }, BoardOperation::TaskMoveWorkstream { .. }),
    TaskSetStatus => ("task_set_status", ProjectToolSelection::Operation { kind: "taskSetStatus" }, BoardOperation::TaskSetStatus { .. }),
    TaskLink => ("task_link", ProjectToolSelection::Operation { kind: "taskLink" }, BoardOperation::TaskLink { .. }),
    TaskUnlink => ("task_unlink", ProjectToolSelection::Operation { kind: "taskUnlink" }, BoardOperation::TaskUnlink { .. }),
    TaskDuplicate => ("task_duplicate", ProjectToolSelection::TaskAction { action: TaskActionSelection::Duplicate }, BoardOperation::TaskAction { action: TaskAction::Duplicate { .. }, .. }),
    TaskReplace => ("task_replace", ProjectToolSelection::TaskAction { action: TaskActionSelection::Replace }, BoardOperation::TaskAction { action: TaskAction::Replace { .. }, .. }),
    SubtaskAdd => ("subtask_add", ProjectToolSelection::Operation { kind: "subtaskAdd" }, BoardOperation::SubtaskAdd { .. }),
    SubtaskCheck => ("subtask_check", ProjectToolSelection::SubtaskCheck { checked: true }, BoardOperation::SubtaskCheck { checked: true, .. }),
    SubtaskUncheck => ("subtask_uncheck", ProjectToolSelection::SubtaskCheck { checked: false }, BoardOperation::SubtaskCheck { checked: false, .. }),
    SubtaskMove => ("subtask_move", ProjectToolSelection::Operation { kind: "subtaskMove" }, BoardOperation::SubtaskMove { .. }),
    SubtaskRemove => ("subtask_remove", ProjectToolSelection::Operation { kind: "subtaskRemove" }, BoardOperation::SubtaskRemove { .. }),
    WorkTrailAdd => ("work_trail_add", ProjectToolSelection::Operation { kind: "workTrailAdd" }, BoardOperation::WorkTrailAdd { .. }),
    WorkTrailRedact => ("work_trail_redact", ProjectToolSelection::Operation { kind: "workTrailRedact" }, BoardOperation::WorkTrailRedact { .. }),
    RoleGrant => ("role_grant", ProjectToolSelection::Operation { kind: "roleGrant" }, BoardOperation::RoleGrant { .. }),
    RoleRelease => ("role_release", ProjectToolSelection::Operation { kind: "roleRelease" }, BoardOperation::RoleRelease { .. }),
    OwnerGrantRecord => ("owner_grant_record", ProjectToolSelection::Operation { kind: "ownerGrantRecord" }, BoardOperation::OwnerGrantRecord { .. }),
    RoleReplaceLead => ("role_replace_lead", ProjectToolSelection::Operation { kind: "roleReplaceLead" }, BoardOperation::RoleReplaceLead { .. }),
    RoleAcknowledgeCurrent => ("role_acknowledge_current", ProjectToolSelection::Operation { kind: "roleAcknowledgeCurrent" }, BoardOperation::RoleAcknowledgeCurrent { .. }),
    ProjectPublishPlan => ("project_publish_plan", ProjectToolSelection::Operation { kind: "projectPublishPlan" }, BoardOperation::ProjectPublishPlan { .. }),
    ProjectClose => ("project_close", ProjectToolSelection::Operation { kind: "projectClose" }, BoardOperation::ProjectClose { .. }),
    ProjectDescribe => ("project_describe", ProjectToolSelection::Operation { kind: "projectDescribe" }, BoardOperation::ProjectDescribe { .. }),
    FactRecord => ("fact_record", ProjectToolSelection::Operation { kind: "factRecord" }, BoardOperation::FactRecord { .. }),
}
