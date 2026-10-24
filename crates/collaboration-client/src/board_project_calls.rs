//! Typed project board calls; uncertain writes are never retransmitted.
use super::{BoardClientError, CollaborationClient};
use message_board::public_replies::WriteReply;
use message_board::*;
impl CollaborationClient {
    pub async fn move_check(
        &self,
        request: ProjectMoveCheckRequest,
    ) -> Result<MoveVerdict, BoardClientError> {
        self.board_call("move_check", request).await
    }
    pub async fn workstream_create(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("workstream_create", request, resource)
            .await
    }
    pub async fn workstream_rename(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("workstream_rename", request, resource)
            .await
    }
    pub async fn workstream_close(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("workstream_close", request, resource)
            .await
    }
    pub async fn task_create(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("task_create", request, resource)
            .await
    }
    pub async fn task_update(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("task_update", request, resource)
            .await
    }
    pub async fn task_move_workstream(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("task_move_workstream", request, resource)
            .await
    }
    pub async fn task_set_status(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("task_set_status", request, resource)
            .await
    }
    pub async fn task_link(&self, request: ProjectRequest) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("task_link", request, resource)
            .await
    }
    pub async fn task_unlink(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("task_unlink", request, resource)
            .await
    }
    pub async fn task_duplicate(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("task_duplicate", request, resource)
            .await
    }
    pub async fn task_replace(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("task_replace", request, resource)
            .await
    }
    pub async fn subtask_add(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("subtask_add", request, resource)
            .await
    }
    pub async fn subtask_check(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("subtask_check", request, resource)
            .await
    }
    pub async fn subtask_uncheck(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("subtask_uncheck", request, resource)
            .await
    }
    pub async fn subtask_move(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("subtask_move", request, resource)
            .await
    }
    pub async fn subtask_remove(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("subtask_remove", request, resource)
            .await
    }
    pub async fn work_trail_add(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("work_trail_add", request, resource)
            .await
    }
    pub async fn work_trail_redact(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("work_trail_redact", request, resource)
            .await
    }
    pub async fn role_grant(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("role_grant", request, resource)
            .await
    }
    pub async fn role_release(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("role_release", request, resource)
            .await
    }
    pub async fn owner_grant_record(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("owner_grant_record", request, resource)
            .await
    }
    pub async fn role_replace_lead(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("role_replace_lead", request, resource)
            .await
    }
    pub async fn role_acknowledge_current(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("role_acknowledge_current", request, resource)
            .await
    }
    pub async fn project_pin_definition(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("project_pin_definition", request, resource)
            .await
    }
    pub async fn project_publish_plan(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("project_publish_plan", request, resource)
            .await
    }
    pub async fn project_describe(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("project_describe", request, resource)
            .await
    }
    pub async fn project_close(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("project_close", request, resource)
            .await
    }
    pub async fn project_notification_cancel(
        &self,
        request: ProjectRequest,
    ) -> Result<collaboration_protocol::RequestedCancelResult, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("project_notification_cancel", request, resource)
            .await
    }
    pub async fn move_request(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("move_request", request, resource)
            .await
    }
    pub async fn fact_record(
        &self,
        request: ProjectRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("fact_record", request, resource)
            .await
    }
    pub async fn workstream_show(
        &self,
        request: ProjectRecordShowRequest,
    ) -> Result<Record, BoardClientError> {
        self.board_call("workstream_show", request).await
    }
    pub async fn workstream_list(
        &self,
        request: ProjectRecordListRequest,
    ) -> Result<RecordListResult, BoardClientError> {
        self.board_call("workstream_list", request).await
    }
    pub async fn task_show(
        &self,
        request: ProjectRecordShowRequest,
    ) -> Result<Record, BoardClientError> {
        self.board_call("task_show", request).await
    }
    pub async fn task_list(
        &self,
        request: ProjectRecordListRequest,
    ) -> Result<RecordListResult, BoardClientError> {
        self.board_call("task_list", request).await
    }
    pub async fn subtask_show(
        &self,
        request: ProjectRecordShowRequest,
    ) -> Result<Record, BoardClientError> {
        self.board_call("subtask_show", request).await
    }
    pub async fn subtask_list(
        &self,
        request: SubtaskListRequest,
    ) -> Result<RecordListResult, BoardClientError> {
        self.board_call("subtask_list", request).await
    }
    pub async fn role_show(
        &self,
        request: ProjectRecordShowRequest,
    ) -> Result<Record, BoardClientError> {
        self.board_call("role_show", request).await
    }
    pub async fn role_list(
        &self,
        request: ProjectRecordListRequest,
    ) -> Result<RecordListResult, BoardClientError> {
        self.board_call("role_list", request).await
    }
    pub async fn move_show(
        &self,
        request: ProjectRecordShowRequest,
    ) -> Result<Record, BoardClientError> {
        self.board_call("move_show", request).await
    }
    pub async fn move_list(
        &self,
        request: ProjectRecordListRequest,
    ) -> Result<RecordListResult, BoardClientError> {
        self.board_call("move_list", request).await
    }
    pub async fn fact_show(
        &self,
        request: ProjectRecordShowRequest,
    ) -> Result<Record, BoardClientError> {
        self.board_call("fact_show", request).await
    }
    pub async fn fact_list(
        &self,
        request: ProjectRecordListRequest,
    ) -> Result<RecordListResult, BoardClientError> {
        self.board_call("fact_list", request).await
    }
    pub async fn intent_show(
        &self,
        request: ProjectRecordShowRequest,
    ) -> Result<Record, BoardClientError> {
        self.board_call("intent_show", request).await
    }
    pub async fn intent_list(
        &self,
        request: ProjectRecordListRequest,
    ) -> Result<RecordListResult, BoardClientError> {
        self.board_call("intent_list", request).await
    }
    pub async fn feed_read(
        &self,
        request: FeedReadRequest,
    ) -> Result<BoardReadReply<FeedRead>, BoardClientError> {
        self.board_call("feed_read", request).await
    }
    pub async fn work_trail_show(
        &self,
        request: WorkTrailShowRequest,
    ) -> Result<BoardReadReply<WorkTrailReadResult>, BoardClientError> {
        self.board_call("work_trail_show", request).await
    }
    pub async fn project_watch(
        &self,
        request: ProjectWatchRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("project_watch", request, resource)
            .await
    }
    pub async fn project_unwatch(
        &self,
        request: ProjectWatchRequest,
    ) -> Result<WriteReply, BoardClientError> {
        let resource = ResourceIdentity::Project {
            project_id: request.project_id.clone(),
        };
        self.board_mutation_call("project_unwatch", request, resource)
            .await
    }
}
