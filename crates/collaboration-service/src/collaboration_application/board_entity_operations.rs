//! Named operations constrain the shared project request to one semantic operation.
use super::board_paging::fingerprint;
use super::{BoardOperations, admission_error};
use message_board::public_replies::WriteReply;
use message_board::*;

struct EntityListSelection<'operation> {
    kind: ProjectReadKind,
    operation: &'operation str,
    filter: String,
    include_removed: bool,
}
macro_rules! project_operation {
    ($name:ident,$selector:ident) => {
        pub async fn $name(&self, request: ProjectRequest) -> Result<WriteReply, BoardError> {
            if !ProjectTool::$selector.accepts(&request.operation) {
                return Err(BoardError::invalid_field(
                    "operation",
                    "must match the named tool",
                ));
            }
            self.project_write(request).await
        }
    };
}
macro_rules! entity_read_operation {
    (show $name:ident, $kind:ident) => {
        pub async fn $name(&self, request: ProjectRecordShowRequest) -> Result<Record, BoardError> {
            self.entity_show(request, RecordKind::$kind).await
        }
    };
    (list $name:ident, $kind:ident) => {
        pub async fn $name(
            &self,
            request: ProjectRecordListRequest,
        ) -> Result<RecordListResult, BoardError> {
            self.entity_list(request, ProjectReadKind::$kind, stringify!($name))
                .await
        }
    };
}
impl BoardOperations<'_> {
    entity_read_operation!(show workstream_show, Workstream);
    entity_read_operation!(list workstream_list, Workstream);
    entity_read_operation!(show task_show, Task);
    entity_read_operation!(list task_list, Task);
    entity_read_operation!(show subtask_show, Subtask);
    entity_read_operation!(show role_show, RoleAssignment);
    entity_read_operation!(list role_list, RoleAssignment);
    entity_read_operation!(show fact_show, Fact);
    entity_read_operation!(list fact_list, Fact);
    entity_read_operation!(show intent_show, NotifyIntent);
    entity_read_operation!(list intent_list, NotifyIntent);
    project_operation!(workstream_create, WorkstreamCreate);
    project_operation!(workstream_rename, WorkstreamRename);
    project_operation!(workstream_close, WorkstreamClose);
    project_operation!(task_create, TaskCreate);
    project_operation!(task_update, TaskUpdate);
    project_operation!(task_move_workstream, TaskMoveWorkstream);
    project_operation!(task_set_status, TaskSetStatus);
    project_operation!(task_link, TaskLink);
    project_operation!(task_unlink, TaskUnlink);
    project_operation!(task_duplicate, TaskDuplicate);
    project_operation!(task_replace, TaskReplace);
    project_operation!(subtask_add, SubtaskAdd);
    project_operation!(subtask_check, SubtaskCheck);
    project_operation!(subtask_uncheck, SubtaskUncheck);
    project_operation!(subtask_move, SubtaskMove);
    project_operation!(subtask_remove, SubtaskRemove);
    project_operation!(work_trail_add, WorkTrailAdd);
    project_operation!(work_trail_redact, WorkTrailRedact);
    project_operation!(role_grant, RoleGrant);
    project_operation!(role_release, RoleRelease);
    project_operation!(owner_grant_record, OwnerGrantRecord);
    project_operation!(role_replace_lead, RoleReplaceLead);
    project_operation!(role_acknowledge_current, RoleAcknowledgeCurrent);
    project_operation!(project_publish_plan, ProjectPublishPlan);
    project_operation!(project_close, ProjectClose);
    project_operation!(project_describe, ProjectDescribe);
    project_operation!(fact_record, FactRecord);
    pub async fn entity_show(
        &self,
        request: ProjectRecordShowRequest,
        kind: RecordKind,
    ) -> Result<Record, BoardError> {
        if request.record.kind != kind {
            return Err(BoardError::invalid_field(
                "record.kind",
                "must match the named tool",
            ));
        }
        let store = self
            .admission()?
            .registry
            .read_project(&request.project_id)
            .await
            .map_err(|error| admission_error(error.into()))?
            .ok_or_else(BoardError::board_unavailable)?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let record = read
            .record(&request.record)
            .await
            .map_err(|error| admission_error(error.into()))?;
        read.finish()
            .await
            .map_err(|error| admission_error(error.into()))?;
        record.ok_or_else(|| BoardError::invalid_field("record", "was not found in this project"))
    }
    pub async fn entity_list(
        &self,
        request: ProjectRecordListRequest,
        kind: ProjectReadKind,
        operation: &str,
    ) -> Result<RecordListResult, BoardError> {
        let filter = fingerprint(&request)?;
        self.entity_list_filtered(
            request,
            EntityListSelection {
                kind,
                operation,
                filter,
                include_removed: false,
            },
        )
        .await
    }
    pub async fn subtask_list(
        &self,
        request: SubtaskListRequest,
    ) -> Result<RecordListResult, BoardError> {
        let filter = fingerprint(&request)?;
        self.entity_list_filtered(
            ProjectRecordListRequest {
                project_id: request.project_id,
                page: request.page,
            },
            EntityListSelection {
                kind: ProjectReadKind::Subtask,
                operation: "subtask_list",
                filter,
                include_removed: request.include_removed,
            },
        )
        .await
    }
    async fn entity_list_filtered(
        &self,
        request: ProjectRecordListRequest,
        selection: EntityListSelection<'_>,
    ) -> Result<RecordListResult, BoardError> {
        let EntityListSelection {
            kind,
            operation,
            filter,
            include_removed,
        } = selection;
        let after = self
            .metadata_after(&request.page, operation, &filter)
            .await?;
        let limit = usize::try_from(request.page.limit.get())
            .map_err(|_| BoardError::board_unavailable())?;
        let key_limit = u64::from(request.page.limit.get()) + 1;
        let store = self
            .admission()?
            .registry
            .read_project(&request.project_id)
            .await
            .map_err(|error| admission_error(error.into()))?
            .ok_or_else(BoardError::board_unavailable)?;
        let mut read = store
            .snapshot()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let mut keys = if kind == ProjectReadKind::Subtask && !include_removed {
            read.live_subtask_page_keys(&after, key_limit).await
        } else {
            read.record_keys(kind.into(), &after, key_limit).await
        }
        .map_err(|error| admission_error(error.into()))?;
        let mut has_more = keys.len() > limit;
        keys.truncate(limit);
        let stored = read
            .record_batch(&keys)
            .await
            .map_err(|error| admission_error(error.into()))?;
        if stored.len() != keys.len() {
            return Err(BoardError::board_unavailable());
        }
        let mut records = Vec::new();
        let mut bytes = 2_usize;
        for stored in stored {
            let record = stored
                .decode()
                .map_err(|error| admission_error(error.into()))?;
            let size = serde_json::to_vec(&record)
                .map_err(|_| BoardError::board_unavailable())?
                .len();
            if bytes.saturating_add(size).saturating_add(1) > 900 * 1024 {
                if records.is_empty() {
                    return Err(BoardError::board_unavailable());
                }
                has_more = true;
                break;
            }
            bytes = bytes.saturating_add(size).saturating_add(1);
            records.push(record);
        }
        read.finish()
            .await
            .map_err(|error| admission_error(error.into()))?;
        let page = self
            .selected_metadata_page(
                records,
                request.page,
                operation,
                filter,
                has_more,
                |record| record.reference().id.as_str().to_owned(),
            )
            .await?;
        Ok(RecordListResult {
            records: page.records,
            next_cursor: page.next_cursor,
        })
    }
}
