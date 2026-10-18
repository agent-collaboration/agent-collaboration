use super::board_preparation::{self, CommandContext, PreparedBoardCommand};
use collaboration_client::board::*;
use collaboration_client::protocol::ThreadSubscriptionWaitFilter;
use collaboration_client::{BoardClientError, ClientError, CollaborationClient};
use serde::Serialize;
use serde_json::{Value, json};
use std::io::{self, Write};

enum CommandExecutionError {
    ConnectionBeforeRequest(ClientError),
    Request {
        error: BoardClientError,
        thread_create_text_file: Option<std::path::PathBuf>,
    },
    InvalidUsage(String),
}

enum CommandExecutionResult {
    Value(Value),
}

pub(super) fn execute(command: PreparedBoardCommand, context: CommandContext) -> i32 {
    let directory = match crate::endpoint_commands::resolve_directory(context.service_directory) {
        Ok(directory) => directory,
        Err(message) => {
            return crate::endpoint_commands::report_failure(
                "invalidUsage",
                &message,
                2,
                context.json,
            );
        }
    };
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            return report_connection_failure(context.json);
        }
    };
    let result = runtime.block_on(async {
        let mut client = CollaborationClient::connect(
            &directory,
            "agent-collaboration-board",
            env!("CARGO_PKG_VERSION"),
        )
        .await
        .map_err(CommandExecutionError::ConnectionBeforeRequest)?;
        let mut command = command;
        board_preparation::finalize_command(&mut command, &client)
            .map_err(CommandExecutionError::InvalidUsage)?;
        let thread_create_text_file = match &command {
            PreparedBoardCommand::ThreadPost(pending) => pending.text_file.clone(),
            _ => None,
        };
        dispatch(&mut client, command)
            .await
            .map(CommandExecutionResult::Value)
            .map_err(|error| CommandExecutionError::Request {
                error,
                thread_create_text_file,
            })
    });
    report(result, context.json)
}

async fn dispatch(
    client: &mut CollaborationClient,
    command: PreparedBoardCommand,
) -> Result<Value, BoardClientError> {
    match command {
        PreparedBoardCommand::View(command) => {
            super::board_view_commands::dispatch(client, *command).await
        }
        PreparedBoardCommand::Entity(command) => {
            super::board_entity_execution::dispatch(client, *command).await
        }
        PreparedBoardCommand::DiscoverySearch(request) => {
            serialize_result(client.discovery_search(request).await?)
        }
        PreparedBoardCommand::PostSearch(request) => {
            serialize_result(client.post_search(request).await?)
        }
        PreparedBoardCommand::WorkspaceCreate(request) => {
            serialize_result(client.workspace_create(request).await?)
        }
        PreparedBoardCommand::WorkspaceUpdate(request) => {
            serialize_result(client.workspace_update(request).await?)
        }
        PreparedBoardCommand::WorkspaceArchive(request) => {
            serialize_result(client.workspace_archive(request).await?)
        }
        PreparedBoardCommand::WorkspaceShow(request) => {
            serialize_result(client.workspace_show(request).await?)
        }
        PreparedBoardCommand::WorkspaceList { repository, page } => {
            let repository = repository
                .map(|repository| repository.for_client(client))
                .transpose()?;
            serialize_result(
                client
                    .workspace_list(WorkspaceListRequest { repository, page })
                    .await?,
            )
        }
        PreparedBoardCommand::RepositoryAttach {
            request,
            workspace_id,
            repository,
            actor,
            acting_for,
        } => {
            let repository = repository.for_client(client)?;
            serialize_result(
                client
                    .repository_attach(RepositoryAttachRequest {
                        request_id: request
                            .request_id()
                            .map_err(|_| ClientError::InvalidRequest("invalid request metadata"))?,
                        protocol_version: ProtocolVersion::CURRENT,
                        resubmits: request
                            .resubmits()
                            .map_err(|_| ClientError::InvalidRequest("invalid resubmits"))?,
                        workspace_id,
                        repository,
                        actor,
                        acting_for,
                    })
                    .await?,
            )
        }
        PreparedBoardCommand::RepositoryDetach {
            request,
            workspace_id,
            repository,
            actor,
            acting_for,
        } => {
            let repository = repository.for_client(client)?;
            serialize_result(
                client
                    .repository_detach(RepositoryDetachRequest {
                        request_id: request
                            .request_id()
                            .map_err(|_| ClientError::InvalidRequest("invalid request metadata"))?,
                        protocol_version: ProtocolVersion::CURRENT,
                        resubmits: request
                            .resubmits()
                            .map_err(|_| ClientError::InvalidRequest("invalid resubmits"))?,
                        workspace_id,
                        repository,
                        actor,
                        acting_for,
                    })
                    .await?,
            )
        }
        PreparedBoardCommand::RepositoryList(request) => {
            serialize_result(client.repository_list(request).await?)
        }
        PreparedBoardCommand::BoardCreate(request) => {
            serialize_result(client.board_create(request).await?)
        }
        PreparedBoardCommand::BoardUpdate(request) => {
            serialize_result(client.board_update(request).await?)
        }
        PreparedBoardCommand::BoardShow(request) => {
            serialize_result(client.board_show(request).await?)
        }
        PreparedBoardCommand::BoardList(request) => {
            serialize_result(client.board_list(request).await?)
        }
        PreparedBoardCommand::BoardArchive(request) => {
            serialize_result(client.board_archive(request).await?)
        }
        PreparedBoardCommand::BoardClose(request) => {
            serialize_result(client.board_close(request).await?)
        }
        PreparedBoardCommand::ProjectCreate(request) => {
            serialize_result(client.project_create(request).await?)
        }
        PreparedBoardCommand::ProjectUpdate(request) => {
            serialize_result(client.project_update(request).await?)
        }
        PreparedBoardCommand::ProjectList(request) => {
            serialize_result(client.project_list(request).await?)
        }
        PreparedBoardCommand::ProjectShow(request) => {
            serialize_result(client.project_show(request).await?)
        }
        PreparedBoardCommand::ThreadPost(pending) => {
            serialize_result(client.thread_post(pending.request).await?)
        }
        PreparedBoardCommand::PostShow(request) => {
            serialize_result(client.post_show(request).await?)
        }
        PreparedBoardCommand::PostList(request) => {
            serialize_result(client.post_list(request).await?)
        }
        PreparedBoardCommand::ThreadShow(request) => {
            serialize_result(client.thread_show(request).await?)
        }
        PreparedBoardCommand::ThreadRefile(request) => {
            serialize_result(client.thread_refile(request).await?)
        }
        PreparedBoardCommand::ThreadResolve(request) => {
            serialize_result(client.thread_resolve(request).await?)
        }
        PreparedBoardCommand::ThreadUnresolve(request) => {
            serialize_result(client.thread_unresolve(request).await?)
        }
        PreparedBoardCommand::ThreadWatch(request) => {
            serialize_result(client.thread_watch(request).await?)
        }
        PreparedBoardCommand::ThreadUnwatch(request) => {
            serialize_result(client.thread_unwatch(request).await?)
        }
        PreparedBoardCommand::ProjectWatch(request) => {
            serialize_result(client.project_watch(request).await?)
        }
        PreparedBoardCommand::ProjectUnwatch(request) => {
            serialize_result(client.project_unwatch(request).await?)
        }
        PreparedBoardCommand::ThreadList(request) => {
            serialize_result(client.thread_list(request).await?)
        }
        PreparedBoardCommand::RepositoryThreadList {
            repository,
            reader,
            page,
            default_repository_path,
        } => repository_threads(client, repository, reader, page, default_repository_path).await,
        PreparedBoardCommand::ThreadCreate(pending) => {
            serialize_result(client.thread_create(pending.request).await?)
        }
        PreparedBoardCommand::ThreadJoin(pending) => {
            serialize_result(client.thread_join(pending.request).await?)
        }
        PreparedBoardCommand::ThreadSubscribe(pending) => {
            serialize_result(client.thread_subscribe(pending.request).await?)
        }
        PreparedBoardCommand::ThreadUnsubscribe(pending) => {
            serialize_result(client.thread_unsubscribe(pending.request).await?)
        }
        PreparedBoardCommand::ThreadSubscriptions(pending) => {
            serialize_result(client.thread_subscriptions(pending.request).await?)
        }
        PreparedBoardCommand::ThreadSubscriptionWait(pending) => {
            serialize_result(super::board_wait_execution::execute(client, pending).await?)
        }
        PreparedBoardCommand::ThreadLeave(pending) => {
            serialize_result(client.thread_leave(pending.request).await?)
        }
        PreparedBoardCommand::ThreadParticipantList(request) => {
            serialize_result(client.thread_participant_list(request).await?)
        }
        PreparedBoardCommand::InboxFetch(request) => {
            serialize_result(client.inbox_fetch(request).await?)
        }
        PreparedBoardCommand::InboxAcknowledge(request) => {
            serialize_result(client.inbox_acknowledge(request).await?)
        }
        PreparedBoardCommand::InboxWorkspaces(request) => {
            serialize_result(client.inbox_workspaces(request).await?)
        }
    }
}

async fn repository_threads(
    client: &mut CollaborationClient,
    repository: collaboration_client::BoardRepositoryLocation,
    reader: Option<Identity>,
    page: PageRequest,
    default_repository_path: Option<String>,
) -> Result<Value, BoardClientError> {
    if page.cursor.is_some() {
        return Err(BoardClientError::Connection(ClientError::Protocol(
            "repository thread cursors are not yet valid",
        )));
    }
    let repository = repository.for_client(client)?;
    let workspaces = client
        .workspace_list(WorkspaceListRequest {
            repository: Some(repository),
            page: PageRequest {
                limit: 100_u32
                    .try_into()
                    .map_err(|_| ClientError::Protocol("page limit"))?,
                cursor: None,
            },
        })
        .await?;
    let no_workspace_hint = repository_thread_list_hint(
        default_repository_path.as_deref(),
        workspaces.page.records.is_empty(),
    );
    let fallback_reader: Identity =
        serde_json::from_value(json!({"kind":"human","humanId":"repository-thread-reader"}))
            .map_err(|_| ClientError::Protocol("reader identity"))?;
    let mut records = Vec::new();
    for workspace in workspaces.page.records {
        let threads = client
            .thread_list(ThreadListRequest {
                workspace_id: workspace.workspace_id.clone(),
                reader: reader.clone().unwrap_or_else(|| fallback_reader.clone()),
                watched_only: false,
                page: PageRequest {
                    limit: 100_u32
                        .try_into()
                        .map_err(|_| ClientError::Protocol("page limit"))?,
                    cursor: None,
                },
            })
            .await?;
        for thread in threads.page.records {
            let root = client
                .post_show(PostShowRequest {
                    post_id: thread.thread.thread_id.clone().into(),
                })
                .await?
                .0;
            let board = client
                .board_show(BoardShowRequest {
                    board_id: root.board_id.clone(),
                })
                .await?
                .board;
            let latest = client
                .post_list(PostListRequest {
                    scope: PostListScope::Thread {
                        thread: ThreadRef {
                            thread: thread.thread.thread_id.clone(),
                            project: Some(thread.project_id.clone()),
                        },
                    },
                    selection: PostSelection::Latest,
                    page: PageRequest {
                        limit: 1_u32
                            .try_into()
                            .map_err(|_| ClientError::Protocol("page limit"))?,
                        cursor: None,
                    },
                })
                .await?;
            let last_activity = latest
                .page
                .records
                .first()
                .map(|message| message.position)
                .unwrap_or(root.position);
            let watch_status = if reader.is_some() {
                client
                    .thread_show(ThreadShowRequest {
                        include_redacted: false,
                        thread: ThreadRef {
                            thread: thread.thread.thread_id.clone(),
                            project: Some(thread.project_id.clone()),
                        },
                        reader: reader.clone().map_or(ReadActor::Anonymous {}, |reader| {
                            ReadActor::Identified { reader }
                        }),
                    })
                    .await?
                    .watch_status
            } else {
                None
            };
            records.push(json!({
                "workspaceId": workspace.workspace_id,
                "projectId": root.project_id,
                "threadId": thread.thread.thread_id,
                "subject": thread.subject,
                "imported": thread.thread.imported,
                "orchestrator": thread.thread.orchestrator,
                "implementer": thread.thread.implementer,
                "lastActivity": last_activity,
                "watchStatus": watch_status,
                "boardId": board.board_id,
            }));
        }
    }
    records.sort_by(|left, right| {
        right
            .get("lastActivity")
            .and_then(Value::as_u64)
            .cmp(&left.get("lastActivity").and_then(Value::as_u64))
    });
    records.truncate(page.limit.get() as usize);
    Ok(match no_workspace_hint {
        Some(message) => json!({
            "page":{"records":records,"nextCursor":null},
            "message":message
        }),
        None => json!({"page":{"records":records,"nextCursor":null}}),
    })
}

fn repository_thread_list_hint(
    default_repository_path: Option<&str>,
    no_associated_workspaces: bool,
) -> Option<String> {
    if !no_associated_workspaces {
        return None;
    }
    default_repository_path.map(|repository_path| format!(
        "No workspace is associated with repository {repository_path}. Attach one with `agent-collaboration repository attach --workspace-id <workspace-id> --repository-path {} --actor self`, or list a known workspace with `agent-collaboration thread list --workspace-id <workspace-id> --reader self`.",
        shell_single_quoted_argument(repository_path)
    ))
}

#[cfg(test)]
mod repository_thread_list_hint_tests {
    use super::repository_thread_list_hint;

    #[test]
    fn empty_default_repository_result_names_attach_and_workspace_fixes() {
        let hint = repository_thread_list_hint(Some("/work/sample-repo"), true)
            .expect("empty default result gets a hint");
        assert!(hint.contains("/work/sample-repo"));
        assert!(hint.contains(
            "repository attach --workspace-id <workspace-id> --repository-path '/work/sample-repo'"
        ));
        assert!(hint.contains("thread list --workspace-id <workspace-id>"));
    }

    #[test]
    fn explicit_repository_and_nonempty_default_results_keep_listing_semantics() {
        assert!(repository_thread_list_hint(None, true).is_none());
        assert!(repository_thread_list_hint(Some("/work/sample-repo"), false).is_none());
    }
}

fn serialize_result<TValue: Serialize>(value: TValue) -> Result<Value, BoardClientError> {
    serde_json::to_value(value)
        .map_err(|_| ClientError::Protocol("board result could not be rendered").into())
}

fn report(result: Result<CommandExecutionResult, CommandExecutionError>, machine: bool) -> i32 {
    match result {
        Ok(CommandExecutionResult::Value(result)) => write_result(result, machine),
        Err(CommandExecutionError::Request {
            error: BoardClientError::Rejected(error),
            thread_create_text_file,
        }) => {
            if machine {
                write_json(
                    &refusal_output(&error, thread_create_text_file.as_deref()),
                    4,
                )
            } else {
                let mut stderr = io::stderr().lock();
                let kind = wire_name(&error.kind);
                let next_action = refusal_command(&error, thread_create_text_file.as_deref());
                let _written = writeln!(
                    stderr,
                    "Error: {kind}\n{}\nNext action: {next_action}",
                    error.message
                );
                4
            }
        }
        Err(CommandExecutionError::Request {
            error:
                BoardClientError::OutcomeUnknown {
                    resource,
                    message,
                    next_action,
                },
            ..
        }) => report_uncertain_outcome(machine, &resource, message, next_action),
        Err(CommandExecutionError::Request {
            error:
                BoardClientError::WaitOutcomeUnknown {
                    actor,
                    filter,
                    renewals,
                },
            ..
        }) => report_wait_outcome_unknown(&actor, &filter, &renewals),
        Err(CommandExecutionError::InvalidUsage(message)) => {
            crate::endpoint_commands::report_failure("invalidField", &message, 2, machine)
        }
        Err(CommandExecutionError::ConnectionBeforeRequest(error)) => {
            crate::permission_diagnostic_reporting::report_actionable_client_error(
                &error,
                crate::permission_diagnostic_reporting::PermissionDiagnosticRendering::Command,
                machine,
            )
            .unwrap_or_else(|| report_connection_failure(machine))
        }
        Err(CommandExecutionError::Request {
            error: BoardClientError::Connection(_),
            ..
        }) => report_connection_failure(machine),
    }
}

pub(super) fn refusal_output(
    error: &BoardError,
    thread_create_text_file: Option<&std::path::Path>,
) -> Value {
    json!({
        "kind": "error",
        "error": {
            "kind": &error.kind,
            "stage": &error.stage,
            "message": &error.message,
            "nextAction": &error.next_action,
            "details": &error.details,
            "correctiveCommand": refusal_command(error, thread_create_text_file),
        },
    })
}

fn refusal_command(
    error: &BoardError,
    thread_create_text_file: Option<&std::path::Path>,
) -> String {
    let actor = |identity: &Identity| {
        serde_json::to_string(identity)
            .map(|value| shell_single_quoted_argument(&value))
            .unwrap_or_else(|_| "<actor>".to_owned())
    };
    match (&error.next_action, &error.details) {
        (BoardNextAction::CreateThread, BoardErrorDetails::ThreadCreateRefusal { refusal }) => {
            let text_file = thread_create_text_file
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "<path>".to_owned());
            format!(
                "agent-collaboration thread create --project-id {} --actor {} --role <role> (--watch | --no-watch) --text-file {} --json",
                refusal.project_id.as_str(),
                actor(&refusal.actor),
                shell_single_quoted_argument(&text_file)
            )
        }
        (BoardNextAction::JoinThread, BoardErrorDetails::ParticipantRefusal { refusal }) => {
            format!(
                "agent-collaboration thread join --thread-id {} --actor {} --role participant --json",
                refusal.thread_id.as_str(),
                actor(&refusal.actor)
            )
        }
        (
            BoardNextAction::ReplaceOrchestrator,
            BoardErrorDetails::ParticipantRefusal { refusal },
        ) => format!(
            "agent-collaboration thread join --thread-id {} --actor {} --role orchestrator --replace {} --json",
            refusal.thread_id.as_str(),
            actor(&refusal.actor),
            refusal
                .holder
                .as_ref()
                .map(actor)
                .unwrap_or_else(|| "<current-orchestrator>".to_owned()),
        ),
        (
            BoardNextAction::ReplaceImplementer,
            BoardErrorDetails::ParticipantRefusal { refusal },
        ) => format!(
            "agent-collaboration thread join --thread-id {} --actor {} --role implementer --replace {} --json",
            refusal.thread_id.as_str(),
            actor(&refusal.actor),
            refusal
                .holder
                .as_ref()
                .map(actor)
                .unwrap_or_else(|| "<current-implementer>".to_owned()),
        ),
        (
            BoardNextAction::LeaveWithHandoverOrResolve,
            BoardErrorDetails::ParticipantRefusal { refusal },
        ) => format!(
            "agent-collaboration thread leave --thread-id {} --actor {} (--to <joined-participant> | --resolve) --json",
            refusal.thread_id.as_str(),
            actor(&refusal.actor)
        ),
        (
            BoardNextAction::JoinHandoverTarget,
            BoardErrorDetails::ParticipantRefusal { refusal },
        ) => format!(
            "agent-collaboration thread join --thread-id {} --actor {} --role <role> --json",
            refusal.thread_id.as_str(),
            refusal
                .target
                .as_ref()
                .map(actor)
                .unwrap_or_else(|| "<handover-target>".to_owned()),
        ),
        (
            BoardNextAction::InspectParticipants,
            BoardErrorDetails::ParticipantRefusal { refusal },
        ) => format!(
            "agent-collaboration thread participant list --thread-id {} --json",
            refusal.thread_id.as_str()
        ),
        (
            BoardNextAction::RepeatJoinWithoutReplace,
            BoardErrorDetails::ParticipantRefusal { refusal },
        ) => format!(
            "agent-collaboration thread join --thread-id {} --actor {} --role orchestrator --json",
            refusal.thread_id.as_str(),
            actor(&refusal.actor)
        ),
        _ => wire_name(&error.next_action),
    }
}

fn shell_single_quoted_argument(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn report_uncertain_outcome(
    machine: bool,
    resource: &ResourceIdentity,
    message: &str,
    next_action: BoardNextAction,
) -> i32 {
    if machine {
        write_json(
            &json!({"kind":"error","error":{"kind":"outcomeUnknown","stage":"inspection","message":message,"nextAction":next_action,"details":{"kind":"resource","resource":resource}}}),
            5,
        )
    } else {
        let mut stderr = io::stderr().lock();
        let resource = serde_json::to_string(resource)
            .unwrap_or_else(|_| "affected resource unavailable".into());
        let next_action = wire_name(&next_action);
        let _written = writeln!(
            stderr,
            "Error: outcomeUnknown\n{message}\nNext action: {next_action}\nAffected resource: {resource}"
        );
        5
    }
}

const WAIT_OUTCOME_UNKNOWN_MESSAGE: &str = "wait result was lost; activity may have been handed off — run board thread subscriptions and board inbox fetch before waiting again";

fn wait_outcome_unknown_output(
    actor: &Identity,
    filter: &ThreadSubscriptionWaitFilter,
    renewals: &[collaboration_client::protocol::SubscriptionRenewalEntry],
) -> Value {
    json!({
        "kind":"error",
        "error":{
            "kind":"outcomeUnknown",
            "stage":"response",
            "effect":"unknown",
            "message":WAIT_OUTCOME_UNKNOWN_MESSAGE,
            "nextAction":BoardNextAction::InspectResource,
            "actor":actor,
            "filter":filter,
            "renewals":renewals,
        }
    })
}

fn report_wait_outcome_unknown(
    actor: &Identity,
    filter: &ThreadSubscriptionWaitFilter,
    renewals: &[collaboration_client::protocol::SubscriptionRenewalEntry],
) -> i32 {
    write_json(&wait_outcome_unknown_output(actor, filter, renewals), 5)
}

fn wire_name<TValue: Serialize>(value: &TValue) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unavailable".to_owned())
}

fn write_result(result: Value, machine: bool) -> i32 {
    if machine {
        write_json(&crate::endpoint_commands::result_envelope(result), 0)
    } else {
        let rendered =
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| "Output unavailable".into());
        if writeln!(io::stdout().lock(), "{rendered}").is_ok() {
            0
        } else {
            3
        }
    }
}

fn report_connection_failure(machine: bool) -> i32 {
    let message =
        "The board service is unavailable. Check the selected service directory and retry.";
    if machine {
        write_json(
            &json!({"kind":"error","error":{"kind":"boardUnavailable","stage":"storage","message":message,"nextAction":"retryLater","details":{"kind":"none"}}}),
            3,
        )
    } else {
        let _written = writeln!(
            io::stderr().lock(),
            "Error: boardUnavailable\n{message}\nNext action: retryLater"
        );
        3
    }
}

fn write_json(value: &Value, success_code: i32) -> i32 {
    if writeln!(io::stdout().lock(), "{value}").is_ok() {
        success_code
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::{
        WAIT_OUTCOME_UNKNOWN_MESSAGE, refusal_command, refusal_output, report_wait_outcome_unknown,
        wait_outcome_unknown_output,
    };
    use collaboration_client::board::*;
    use collaboration_client::protocol::ThreadSubscriptionWaitFilter;
    use std::path::Path;

    #[test]
    fn wait_unknown_cli_output_matches_published_schema_and_retains_inspection_identity() {
        let actor = Identity::Human {
            human_id: HumanId::try_from("cli-test-human".to_owned()).expect("human ID"),
        };
        let filter = ThreadSubscriptionWaitFilter::Roots {
            threads: vec![
                (ThreadId::try_from("01900000-0000-7000-8000-000000000031".to_owned())
                    .expect("root message ID"))
                .into(),
            ],
        };
        let output = wait_outcome_unknown_output(&actor, &filter, &[]);
        let schemas =
            collaboration_client::protocol::protocol_type_schemas().expect("CLI output schemas");
        let validator = jsonschema::validator_for(
            schemas
                .get("FiniteCommandRecord")
                .expect("published CLI error envelope schema"),
        )
        .expect("CLI envelope validator");

        assert!(
            validator.is_valid(&output),
            "unknown wait error must fit the published envelope: {output}"
        );
        assert_eq!(output["error"]["kind"], "outcomeUnknown");
        assert_eq!(output["error"]["stage"], "response");
        assert_eq!(output["error"]["effect"], "unknown");
        assert_eq!(output["error"]["nextAction"], "inspectResource");
        assert_eq!(output["error"]["message"], WAIT_OUTCOME_UNKNOWN_MESSAGE);
        assert_eq!(
            output["error"]["actor"],
            serde_json::to_value(&actor).expect("actor JSON")
        );
        assert_eq!(
            output["error"]["filter"],
            serde_json::to_value(&filter).expect("filter JSON")
        );
        assert_eq!(report_wait_outcome_unknown(&actor, &filter, &[]), 5);
    }

    #[test]
    fn actor_and_text_file_arguments_escape_apostrophes() {
        let project_id = ProjectId::try_from("018f6f67-64d2-7a21-bf9a-8f193f987091".to_owned())
            .expect("project ID");
        let actor = Identity::Human {
            human_id: HumanId::try_from("owner's".to_owned()).expect("human ID"),
        };
        let text = PostText::try_from("root text".to_owned()).expect("message text");
        let refusal = BoardError::session_project_post(project_id, actor, text);
        let command = refusal_command(&refusal, Some(Path::new("/tmp/owner's message.txt")));

        assert!(command.contains("--actor '{\"kind\":\"human\",\"humanId\":\"owner'\\''s\"}'"));
        assert!(command.contains("--text-file '/tmp/owner'\\''s message.txt'"));
        assert!(!command.contains("--text-file '<path>'"));
    }

    #[test]
    fn holder_argument_escapes_apostrophes() {
        let thread_id = ThreadId::try_from("018f6f67-64d2-7a21-bf9a-8f193f987091".to_owned())
            .expect("message ID");
        let actor = Identity::Human {
            human_id: HumanId::try_from("requester".to_owned()).expect("human ID"),
        };
        let holder = Identity::Human {
            human_id: HumanId::try_from("holder's".to_owned()).expect("human ID"),
        };
        let refusal = BoardError {
            kind: BoardFailureKind::OrchestratorAlreadyExists,
            stage: BoardFailureStage::Admission,
            message: "holder exists".to_owned(),
            next_action: BoardNextAction::ReplaceOrchestrator,
            details: BoardErrorDetails::ParticipantRefusal {
                refusal: Box::new(ParticipantRefusalDetails {
                    thread_id,
                    missing_thread_ids: Vec::new(),
                    actor,
                    allowed_roles: Vec::new(),
                    holder: Some(holder),
                    holder_last_seen_position: None,
                    target: None,
                    named_holder: None,
                }),
            },
        };

        let command = refusal_command(&refusal, None);

        assert!(command.contains("--replace '{\"kind\":\"human\",\"humanId\":\"holder'\\''s\"}'"));
    }

    #[test]
    fn handover_target_argument_escapes_apostrophes() {
        let thread_id = ThreadId::try_from("018f6f67-64d2-7a21-bf9a-8f193f987091".to_owned())
            .expect("message ID");
        let actor = Identity::Human {
            human_id: HumanId::try_from("orchestrator".to_owned()).expect("human ID"),
        };
        let target = Identity::Human {
            human_id: HumanId::try_from("target's".to_owned()).expect("human ID"),
        };
        let refusal = BoardError {
            kind: BoardFailureKind::HandoverTargetNotParticipant,
            stage: BoardFailureStage::Admission,
            message: "target must join".to_owned(),
            next_action: BoardNextAction::JoinHandoverTarget,
            details: BoardErrorDetails::ParticipantRefusal {
                refusal: Box::new(ParticipantRefusalDetails {
                    thread_id,
                    missing_thread_ids: Vec::new(),
                    actor,
                    allowed_roles: Vec::new(),
                    holder: None,
                    holder_last_seen_position: None,
                    target: Some(target),
                    named_holder: None,
                }),
            },
        };

        let command = refusal_command(&refusal, None);

        assert!(command.contains("--actor '{\"kind\":\"human\",\"humanId\":\"target'\\''s\"}'"));
    }

    #[test]
    fn machine_refusal_serialization_includes_the_corrective_command() {
        let project_id = ProjectId::try_from("018f6f67-64d2-7a21-bf9a-8f193f987091".to_owned())
            .expect("project ID");
        let actor = Identity::Human {
            human_id: HumanId::try_from("owner".to_owned()).expect("human ID"),
        };
        let text = PostText::try_from("root text".to_owned()).expect("message text");
        let refusal = BoardError::session_project_post(project_id.clone(), actor, text);
        let output = serde_json::from_str::<serde_json::Value>(
            &serde_json::to_string(&refusal_output(
                &refusal,
                Some(Path::new("/tmp/root message.txt")),
            ))
            .expect("refusal JSON serializes"),
        )
        .expect("refusal JSON parses");

        assert_eq!(output["kind"], "error");
        assert_eq!(output["error"]["nextAction"], "createThread");
        assert_eq!(
            output["error"]["correctiveCommand"],
            "agent-collaboration thread create --project-id 018f6f67-64d2-7a21-bf9a-8f193f987091 --actor '{\"kind\":\"human\",\"humanId\":\"owner\"}' --role <role> (--watch | --no-watch) --text-file '/tmp/root message.txt' --json"
        );
        assert_eq!(output["error"]["details"]["projectId"], project_id.as_str());
    }

    #[test]
    fn participant_refusal_names_an_executable_join_command_with_the_required_role() {
        let thread_id = ThreadId::try_from("019f0000-0000-7000-8000-000000000104".to_owned())
            .expect("root message ID");
        let actor = Identity::Human {
            human_id: HumanId::try_from("owner".to_owned()).expect("human ID"),
        };
        let refusal = BoardError::participant_required(thread_id, actor);

        let command = refusal_command(&refusal, None);

        assert_eq!(
            command,
            "agent-collaboration thread join --thread-id 019f0000-0000-7000-8000-000000000104 --actor '{\"kind\":\"human\",\"humanId\":\"owner\"}' --role participant --json"
        );
    }
}
