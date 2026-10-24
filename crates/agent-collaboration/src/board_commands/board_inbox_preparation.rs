//! Prepare typed inbox operations before transmission.
use super::*;
pub(super) fn prepare_inbox(
    command: InboxCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        InboxCommand::Fetch(arguments) => {
            let scope = inbox_scope(&arguments)?;
            let read_mode = match arguments.read_mode {
                InboxModeKind::Unread => InboxReadMode::Unread,
                InboxModeKind::Latest => InboxReadMode::Latest,
            };
            let initializations = if !arguments.initializations.is_empty() {
                arguments
                    .initializations
                    .iter()
                    .map(|entry| {
                        serde_json::from_str::<ReadInitializationEntry>(entry).map_err(|_| {
                            "--initialization requires typed per-project request metadata JSON"
                                .to_owned()
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?
            } else if arguments.page.cursor.is_some() {
                Vec::new()
            } else {
                match (&scope, read_mode) {
                    (InboxScope::Project { project_id }, InboxReadMode::Unread) => {
                        vec![ReadInitializationEntry {
                            project_id: project_id.clone(),
                            request_id: RequestId::generate(),
                        }]
                    }
                    _ => Vec::new(),
                }
            };
            let request = InboxFetchRequest::new(InboxFetchRequestProps {
                protocol_version: ProtocolVersion::CURRENT,
                acting_for: None,
                scope,
                read_mode,
                reader: parse_identity(&arguments.reader, "--reader")?,
                initializations,
                page: prepare_page_request(arguments.page)?,
            })
            .map_err(|error| error.to_string())?;
            Ok((
                PreparedBoardCommand::InboxFetch(request),
                command_context(arguments.common),
            ))
        }
        InboxCommand::Acknowledge(arguments) => {
            let actor = parse_identity(&arguments.actor, "--actor")?;
            let acting_for = arguments
                .acting_for
                .as_deref()
                .map(parse_acting_for)
                .transpose()?;
            let entries = arguments
                .entries
                .iter()
                .map(|entry| {
                    serde_json::from_str::<AcknowledgementEntry>(entry).map_err(|_| {
                        "--entry requires requestId, scope and FeedCursor JSON".to_owned()
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok((
                PreparedBoardCommand::InboxAcknowledge(
                    InboxAcknowledgeRequest::new(InboxAcknowledgeRequestProps {
                        protocol_version: ProtocolVersion::CURRENT,
                        actor,
                        acting_for,
                        entries,
                    })
                    .map_err(|error| error.to_string())?,
                ),
                command_context(arguments.common),
            ))
        }
        InboxCommand::Workspaces(arguments) => Ok((
            PreparedBoardCommand::InboxWorkspaces(InboxWorkspacesRequest {
                reader: parse_identity(&arguments.reader, "--reader")?,
                unread_only: arguments.unread_only,
                page: prepare_page_request(arguments.page)?,
            }),
            command_context(arguments.common),
        )),
    }
}
