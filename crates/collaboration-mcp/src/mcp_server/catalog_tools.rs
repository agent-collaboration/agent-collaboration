//! The catalog tools whose request and result types come straight from the domain crates:
//! automation (wakes, schedules, runs, instructions, configuration and inspection) and the
//! board. Each decodes its typed request, calls one application operation and presents the
//! typed result or rejection.
use super::board_argument_classification::decode_board_arguments;
use super::*;
use collaboration_protocol::{
    ThreadSubscribeRequest, ThreadSubscriptionsRequest, ThreadSubscriptionsResult,
    ThreadUnsubscribeRequest,
};

/// Registers one tool: `$name` takes `$request` and answers `$result`; `$call` runs with the
/// application bound to `$application` and the decoded request to `$input`.
macro_rules! application_tool {
    ($router:expr, $name:literal, $request:ty => $result:ty, |$application:ident, $input:ident| $call:expr) => {
        $router.add_route(ToolRoute::new_dyn(
            Tool::new(
                $name,
                operation_description($name),
                rmcp::handler::server::tool::schema_for_type::<$request>(),
            )
            .with_raw_output_schema(rmcp::handler::server::tool::schema_for_type::<
                McpToolOutput<$result>,
            >()),
            |context: ToolCallContext<'_, CollaborationMcpServer>| {
                Box::pin(async move {
                    let $input = match serde_json::from_value::<$request>(
                        serde_json::Value::Object(context.arguments.unwrap_or_default()),
                    ) {
                        Ok(value) => value,
                        Err(error) => {
                            return Ok(CallToolResponse::Complete(validation_failure(
                                &error.to_string(),
                            )));
                        }
                    };
                    let $application = &context.service.application;
                    Ok(CallToolResponse::Complete($call))
                })
            },
        ));
    };
}

/// A domain operation on one application family: `mutation` decides the effect a rejection
/// the family does not type reports.
macro_rules! domain_tool {
    ($router:expr, $name:literal, $request:ty => $result:ty, $family:ident . $operation:ident, $mutation:literal) => {
        application_tool!($router, $name, $request => $result, |application, request| {
            domain_result(application.$family().$operation(request).await, $mutation)
        })
    };
    ($router:expr, $name:literal, $request:ty => $result:ty, $family:ident . $operation:ident(budget), $mutation:literal) => {
        application_tool!($router, $name, $request => $result, |application, request| {
            domain_result(
                application.$family().$operation(request, API_RESULT_BUDGET).await,
                $mutation,
            )
        })
    };
}

/// A board operation. Arguments that do not decode are refused with the board's typed
/// rejection naming the field, never with the decoder's message.
macro_rules! board_tool {
    ($router:expr, $name:literal, $request:ty => $result:ty, $operation:ident, $mutation:literal) => {
        $router.add_route(ToolRoute::new_dyn(
            Tool::new(
                $name,
                operation_description($name),
                super::project_tool_schemas::board_input_schema::<$request>($name),
            )
            .with_raw_output_schema(rmcp::handler::server::tool::schema_for_type::<
                McpToolOutput<$result>,
            >()),
            |context: ToolCallContext<'_, CollaborationMcpServer>| {
                Box::pin(async move {
                    let request = match decode_board_arguments::<$request>(
                        $name,
                        context.arguments.unwrap_or_default(),
                    ) {
                        Ok(request) => request,
                        Err(rejection) => {
                            return Ok(CallToolResponse::Complete(domain_result::<(), _>(
                                Err(rejection),
                                $mutation,
                            )));
                        }
                    };
                    let board = context.service.application.board();
                    Ok(CallToolResponse::Complete(domain_result(
                        board.$operation(request).await,
                        $mutation,
                    )))
                })
            },
        ));
    };
}

pub(super) fn register_automation_mutation_tools(router: &mut ToolRouter<CollaborationMcpServer>) {
    use collaboration_protocol::*;
    domain_tool!(router, "instruction_create", InstructionCreateParams => InstructionSnapshot, automation.instruction_create, true);
    domain_tool!(router, "instruction_update", InstructionUpdateParams => InstructionSnapshot, automation.instruction_update, true);
    domain_tool!(router, "instruction_show", InstructionShowParams => InstructionSnapshot, automation.instruction_show, false);
    domain_tool!(router, "wake_send", WakeSendRequest => WakeSnapshot, wakes.wake_send, true);
    domain_tool!(router, "wake_show", WakeShowRequest => WakeSnapshot, wakes.wake_show, false);
    domain_tool!(router, "wake_pause", WakeMutationRequest => WakeMutationResult, wakes.wake_pause, true);
    domain_tool!(router, "wake_resume", WakeMutationRequest => WakeMutationResult, wakes.wake_resume, true);
    domain_tool!(router, "wake_cancel", WakeMutationRequest => WakeMutationResult, wakes.wake_cancel, true);
    domain_tool!(router, "delivery_show", DeliveryShowRequest => DeliveryInspection, wakes.delivery_show, false);
    domain_tool!(router, "wake_list", AutomationPageRequest => AutomationPage<WakeSnapshot>, wakes.wake_list(budget), false);
    domain_tool!(router, "schedule_import", ScheduleImportRequest => ScheduleSnapshot, automation.schedule_import, true);
    domain_tool!(router, "schedule_export", ScheduleShowRequest => ScheduleExportResult, automation.schedule_export(budget), false);
    domain_tool!(router, "schedule_create", ScheduleCreateRequest => ScheduleSnapshot, automation.schedule_create, true);
    domain_tool!(router, "schedule_update", ScheduleUpdateRequest => ScheduleSnapshot, automation.schedule_update, true);
    domain_tool!(router, "schedule_show", ScheduleShowRequest => ScheduleSnapshot, automation.schedule_show, false);
    domain_tool!(router, "schedule_enable", ScheduleEnableRequest => ScheduleSnapshot, automation.schedule_enable, true);
    domain_tool!(router, "schedule_disable", ScheduleEnableRequest => ScheduleSnapshot, automation.schedule_disable, true);
    domain_tool!(router, "schedule_prepare", SchedulePrepareRequest => ScheduleSnapshot, automation.schedule_prepare, true);
    domain_tool!(router, "run_show", RunShowRequest => RunSnapshot, automation.run_show, false);
    domain_tool!(router, "run_summary_retry", RunRecoveryRequest => RunSnapshot, automation.run_summary_retry, true);
    domain_tool!(router, "run_summary_skip", RunRecoveryRequest => RunSnapshot, automation.run_summary_skip, true);
    domain_tool!(router, "automation_configure", AutomationConfigureRequest => AutomationConfiguration, automation.automation_configure, true);
}

pub(super) fn register_automation_inspection_tools(
    router: &mut ToolRouter<CollaborationMcpServer>,
) {
    use collaboration_protocol::*;
    domain_tool!(router, "run_reconcile", RunShowRequest => RunSnapshot, automation.run_reconcile(budget), false);
    domain_tool!(router, "delivery_reconcile", DeliveryShowRequest => DeliveryInspection, automation.delivery_reconcile(budget), false);
    domain_tool!(router, "operation_show", OperationShowRequest => OperationSnapshot, automation.operation_show(budget), false);
    domain_tool!(router, "operation_reconcile", OperationShowRequest => OperationSnapshot, automation.operation_reconcile(budget), false);
    domain_tool!(router, "automation_events", AutomationEventsRequest => AutomationEventsPage, automation.automation_events(budget), false);
    domain_tool!(router, "delivery_attempts", DeliveryAttemptsRequest => AttemptHistoryPage<AttemptInspection>, automation.delivery_attempts(budget), false);
    domain_tool!(router, "run_summaries", RunSummariesRequest => AttemptHistoryPage<SummaryInspection>, automation.run_summaries(budget), false);
    domain_tool!(router, "instruction_list", AutomationPageRequest => AutomationPage<InstructionSnapshot>, automation.instruction_list(budget), false);
    domain_tool!(router, "schedule_list", AutomationPageRequest => AutomationPage<ScheduleSnapshot>, automation.schedule_list(budget), false);
    domain_tool!(router, "run_list", RunListRequest => AutomationPage<RunSnapshot>, automation.run_list(budget), false);
    domain_tool!(router, "delivery_list", DeliveryListRequest => AutomationPage<DeliveryInspection>, automation.delivery_list(budget), false);
    domain_tool!(router, "revision_list", RevisionListRequest => AutomationPage<RevisionRecord>, automation.revision_list(budget), false);
}

pub(super) fn register_board_tools(router: &mut ToolRouter<CollaborationMcpServer>) {
    use message_board::public_replies::{RequestEffectsRead, RequestEffectsRequest, WriteReply};
    use message_board::*;
    board_tool!(router, "request_effects", RequestEffectsRequest => RequestEffectsRead, request_effects, false);
    board_tool!(router, "workstream_create", ProjectRequest => WriteReply, workstream_create, true);
    board_tool!(router, "workstream_rename", ProjectRequest => WriteReply, workstream_rename, true);
    board_tool!(router, "workstream_close", ProjectRequest => WriteReply, workstream_close, true);
    board_tool!(router, "task_create", ProjectRequest => WriteReply, task_create, true);
    board_tool!(router, "task_update", ProjectRequest => WriteReply, task_update, true);
    board_tool!(router, "task_move_workstream", ProjectRequest => WriteReply, task_move_workstream, true);
    board_tool!(router, "task_set_status", ProjectRequest => WriteReply, task_set_status, true);
    board_tool!(router, "task_link", ProjectRequest => WriteReply, task_link, true);
    board_tool!(router, "task_unlink", ProjectRequest => WriteReply, task_unlink, true);
    board_tool!(router, "task_duplicate", ProjectRequest => WriteReply, task_duplicate, true);
    board_tool!(router, "task_replace", ProjectRequest => WriteReply, task_replace, true);
    board_tool!(router, "subtask_add", ProjectRequest => WriteReply, subtask_add, true);
    board_tool!(router, "subtask_check", ProjectRequest => WriteReply, subtask_check, true);
    board_tool!(router, "subtask_uncheck", ProjectRequest => WriteReply, subtask_uncheck, true);
    board_tool!(router, "subtask_move", ProjectRequest => WriteReply, subtask_move, true);
    board_tool!(router, "subtask_remove", ProjectRequest => WriteReply, subtask_remove, true);
    board_tool!(router, "work_trail_add", ProjectRequest => WriteReply, work_trail_add, true);
    board_tool!(router, "work_trail_redact", ProjectRequest => WriteReply, work_trail_redact, true);
    board_tool!(router, "role_grant", ProjectRequest => WriteReply, role_grant, true);
    board_tool!(router, "role_release", ProjectRequest => WriteReply, role_release, true);
    board_tool!(router, "owner_grant_record", ProjectRequest => WriteReply, owner_grant_record, true);
    board_tool!(router, "role_replace_lead", ProjectRequest => WriteReply, role_replace_lead, true);
    board_tool!(router, "role_acknowledge_current", ProjectRequest => WriteReply, role_acknowledge_current, true);
    board_tool!(router, "project_publish_plan", ProjectRequest => WriteReply, project_publish_plan, true);
    board_tool!(router, "project_close", ProjectRequest => WriteReply, project_close, true);
    board_tool!(router, "project_describe", ProjectRequest => WriteReply, project_describe, true);
    board_tool!(router, "fact_record", ProjectRequest => WriteReply, fact_record, true);
    board_tool!(router, "thread_renew", ThreadRenewRequest => WriteReply, thread_renew, true);
    board_tool!(router, "workstream_show", ProjectRecordShowRequest => Record, workstream_show, false);
    board_tool!(router, "workstream_list", ProjectRecordListRequest => RecordListResult, workstream_list, false);
    board_tool!(router, "task_show", ProjectRecordShowRequest => Record, task_show, false);
    board_tool!(router, "task_list", ProjectRecordListRequest => RecordListResult, task_list, false);
    board_tool!(router, "subtask_show", ProjectRecordShowRequest => Record, subtask_show, false);
    board_tool!(router, "subtask_list", SubtaskListRequest => RecordListResult, subtask_list, false);
    board_tool!(router, "role_show", ProjectRecordShowRequest => Record, role_show, false);
    board_tool!(router, "role_list", ProjectRecordListRequest => RecordListResult, role_list, false);
    board_tool!(router, "fact_show", ProjectRecordShowRequest => Record, fact_show, false);
    board_tool!(router, "fact_list", ProjectRecordListRequest => RecordListResult, fact_list, false);
    board_tool!(router, "intent_show", ProjectRecordShowRequest => Record, intent_show, false);
    board_tool!(router, "intent_list", ProjectRecordListRequest => RecordListResult, intent_list, false);
    board_tool!(router,"adr_create",AdrCreateRequest => WriteReply,adr_create,true);
    board_tool!(router,"adr_transition",AdrTransitionRequest => WriteReply,adr_transition,true);
    board_tool!(router,"adr_show",FleetRecordShowRequest => Record,adr_show,false);
    board_tool!(router,"adr_list",FleetRecordListRequest => RecordListResult,adr_list,false);
    board_tool!(router, "feed_read", FeedReadRequest => BoardReadReply<FeedRead>, feed_read, true);
    board_tool!(router, "view_work", ProjectViewRequest => WorkView, view_work, false);
    board_tool!(router, "view_card", ContinuationCardRequest => ContinuationCard, view_card, false);
    board_tool!(router, "view_session_context", SessionContextRequest => SessionContext, view_session_context, false);
    board_tool!(router, "view_owner", OwnerViewRequest => OwnerView, view_owner, false);
    board_tool!(router, "view_lead_load", LeadLoadRequest => LeadLoad, view_lead_load, false);
    board_tool!(router, "work_trail_show", WorkTrailShowRequest => BoardReadReply<WorkTrailReadResult>, work_trail_show, true);
    board_tool!(router, "discovery_search", DiscoverySearchRequest => DiscoverySearchResult, discovery_search, false);
    board_tool!(router, "post_search", PostSearchRequest => PostSearchResult, post_search, false);
    board_tool!(router, "workspace_create", WorkspaceCreateRequest => WriteReply, workspace_create, true);
    board_tool!(router, "workspace_archive", WorkspaceArchiveRequest => WriteReply, workspace_archive, true);
    board_tool!(router, "workspace_update", WorkspaceUpdateRequest => WriteReply, workspace_update, true);
    board_tool!(router, "workspace_show", WorkspaceShowRequest => WorkspaceShowResult, workspace_show, false);
    board_tool!(router, "workspace_list", WorkspaceListRequest => WorkspaceListResult, workspace_list, false);
    board_tool!(router, "repository_attach", RepositoryAttachRequest => WriteReply, repository_attach, true);
    board_tool!(router, "repository_detach", RepositoryDetachRequest => WriteReply, repository_detach, true);
    board_tool!(router, "repository_list", RepositoryListRequest => RepositoryListResult, repository_list, false);
    board_tool!(router, "board_create", BoardCreateRequest => WriteReply, board_create, true);
    board_tool!(router, "board_update", BoardUpdateRequest => WriteReply, board_update, true);
    board_tool!(router, "board_show", BoardShowRequest => BoardShowResult, board_show, false);
    board_tool!(router, "board_list", BoardListRequest => BoardListResult, board_list, false);
    board_tool!(router, "board_close", BoardCloseRequest => WriteReply, board_close, true);
    board_tool!(router, "board_archive", BoardArchiveRequest => WriteReply, board_archive, true);
    board_tool!(router, "project_claim", board_sync_protocol::ProjectClaimRequest => board_sync_protocol::ProjectClaimReply, project_claim, true);
    board_tool!(router, "project_create", ProjectCreateRequest => WriteReply, project_create, true);
    board_tool!(router, "project_update", ProjectUpdateRequest => WriteReply, project_update, true);
    board_tool!(router, "project_notification_cancel", ProjectRequest => collaboration_protocol::RequestedCancelResult, project_notification_cancel, true);
    board_tool!(router, "project_list", ProjectListRequest => ProjectListResult, project_list, false);
    board_tool!(router, "project_show", ProjectShowRequest => ProjectShowResult, project_show, false);
    board_tool!(router, "thread_post", ThreadPostRequest => WriteReply, thread_post, true);
    board_tool!(router, "post_show", PostShowRequest => PostShowResult, post_show, false);
    board_tool!(router, "post_list", PostListRequest => PostListResult, post_list, false);
    board_tool!(router, "thread_show", ThreadShowRequest => ThreadShowResult, thread_show, false);
    board_tool!(router, "thread_resolve", ThreadResolveRequest => WriteReply, thread_resolve, true);
    board_tool!(router, "thread_unresolve", ThreadUnresolveRequest => WriteReply, thread_unresolve, true);
    board_tool!(router, "thread_watch", ThreadWatchRequest => WriteReply, thread_watch, true);
    board_tool!(router, "thread_unwatch", ThreadUnwatchRequest => WriteReply, thread_unwatch, true);
    board_tool!(router, "project_watch", ProjectWatchRequest => WriteReply, project_watch, true);
    board_tool!(router, "project_unwatch", ProjectWatchRequest => WriteReply, project_unwatch, true);
    board_tool!(router, "thread_list", ThreadListRequest => ThreadListResult, thread_list, false);
    board_tool!(router, "thread_create", ThreadCreateRequest => WriteReply, thread_create, true);
    board_tool!(router, "thread_refile", ThreadRefileRequest => WriteReply, thread_refile, true);
    board_tool!(router, "thread_join", ThreadJoinRequest => WriteReply, thread_join, true);
    board_tool!(router, "thread_leave", ThreadLeaveRequest => WriteReply, thread_leave, true);
    board_tool!(router, "thread_participant_list", ThreadParticipantListRequest => ThreadParticipantListResult, thread_participant_list, false);
    board_tool!(router, "thread_subscribe", ThreadSubscribeRequest => WriteReply, thread_subscribe, true);
    board_tool!(router, "thread_unsubscribe", ThreadUnsubscribeRequest => WriteReply, thread_unsubscribe, true);
    board_tool!(router, "thread_subscriptions", ThreadSubscriptionsRequest => ThreadSubscriptionsResult, thread_subscriptions, false);
    board_tool!(router, "inbox_fetch", InboxFetchRequest => InboxFetchResult, inbox_fetch, true);
    board_tool!(router, "inbox_acknowledge", InboxAcknowledgeRequest => InboxAcknowledgeReply, inbox_acknowledge, true);
    board_tool!(router, "inbox_workspaces", InboxWorkspacesRequest => InboxWorkspacesResult, inbox_workspaces, false);
}

#[derive(Debug, Default, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct EmptyToolInput {}
