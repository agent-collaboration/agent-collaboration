//! View flags become typed SDK requests; the CLI never opens a board store.
use super::{
    board_arguments::CommonArguments,
    board_preparation::{
        ActorInput, CommandContext, PreparedBoardCommand, finalize_actor, finalize_identity,
    },
    board_value_parsing::{command_context, parse_identity, parse_uuid_v7},
};
use clap::{Args, Subcommand};
use collaboration_client::{BoardClientError, ClientError, CollaborationClient, board::*};
use serde::Serialize;
use serde_json::Value;

#[derive(Args)]
pub(super) struct WorkViewArguments {
    #[arg(long)]
    project_id: String,
    #[arg(long)]
    include_removed: bool,
    #[command(flatten)]
    common: CommonArguments,
}
#[derive(Args)]
pub(super) struct CardArguments {
    #[arg(long)]
    project_id: String,
    /// Identity JSON or self.
    #[arg(long)]
    actor: String,
    /// TrailSubject JSON selecting the project, workstream or task.
    #[arg(long)]
    subject: String,
    #[command(flatten)]
    common: CommonArguments,
}
#[derive(Args)]
pub(super) struct ContextArguments {
    /// SessionRef JSON or self, resolved against the selected service.
    #[arg(long)]
    session: String,
    #[command(flatten)]
    common: CommonArguments,
}
#[derive(Subcommand)]
pub(super) enum ViewCommand {
    Work(WorkViewArguments),
    Card(CardArguments),
    Owner {
        #[command(flatten)]
        common: CommonArguments,
    },
    LeadLoad {
        /// RecordedActor JSON for the lead whose load to inspect.
        #[arg(long)]
        lead: String,
        #[command(flatten)]
        common: CommonArguments,
    },
    SessionContext(ContextArguments),
}
pub(super) enum ContextInput {
    Session(SessionRef),
    Self_,
}
pub(super) enum PreparedViewCommand {
    Work(ProjectViewRequest),
    Card(ContinuationCardRequest),
    Owner(OwnerViewRequest),
    LeadLoad(LeadLoadRequest),
    Context(ContextInput),
}
fn prepared(
    command: PreparedViewCommand,
    common: CommonArguments,
) -> (PreparedBoardCommand, CommandContext) {
    (
        PreparedBoardCommand::View(Box::new(command)),
        command_context(common),
    )
}
fn card(arguments: CardArguments) -> Result<(ContinuationCardRequest, CommonArguments), String> {
    Ok((
        ContinuationCardRequest {
            project_id: parse_uuid_v7(arguments.project_id, "--project-id")?,
            actor: parse_identity(&arguments.actor, "--actor")?,
            subject: serde_json::from_str(&arguments.subject)
                .map_err(|_| "--subject requires TrailSubject JSON")?,
        },
        arguments.common,
    ))
}
fn context(arguments: ContextArguments) -> Result<(ContextInput, CommonArguments), String> {
    let input = if arguments.session == "self" {
        ContextInput::Self_
    } else {
        ContextInput::Session(
            serde_json::from_str(&arguments.session)
                .map_err(|_| "--session requires SessionRef JSON or self")?,
        )
    };
    Ok((input, arguments.common))
}
pub(super) fn prepare_view(
    command: ViewCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    Ok(match command {
        ViewCommand::Work(args) => prepared(
            PreparedViewCommand::Work(ProjectViewRequest {
                project_id: parse_uuid_v7(args.project_id, "--project-id")?,
                include_removed: args.include_removed,
            }),
            args.common,
        ),
        ViewCommand::Card(args) => {
            let (request, common) = card(args)?;
            prepared(PreparedViewCommand::Card(request), common)
        }
        ViewCommand::Owner { common } => {
            prepared(PreparedViewCommand::Owner(OwnerViewRequest {}), common)
        }
        ViewCommand::LeadLoad { lead, common } => prepared(
            PreparedViewCommand::LeadLoad(LeadLoadRequest {
                lead: serde_json::from_str(&lead)
                    .map_err(|_| "--lead requires RecordedActor JSON")?,
            }),
            common,
        ),
        ViewCommand::SessionContext(args) => {
            let (input, common) = context(args)?;
            prepared(PreparedViewCommand::Context(input), common)
        }
    })
}
impl PreparedViewCommand {
    pub(super) fn finalize(&mut self, client: &CollaborationClient) -> Result<(), String> {
        match self {
            Self::Card(request) => finalize_identity(&mut request.actor, client),
            _ => Ok(()),
        }
    }
}
fn context_request(
    input: ContextInput,
    client: &CollaborationClient,
) -> Result<SessionContextRequest, BoardClientError> {
    let session = match input {
        ContextInput::Session(session) => session,
        ContextInput::Self_ => match finalize_actor(&ActorInput::Self_, client)
            .map_err(|_| ClientError::InvalidRequest("current session identity unavailable"))?
        {
            Identity::Session { session } => session,
            Identity::Human { .. } => {
                return Err(ClientError::InvalidRequest("context requires a session").into());
            }
        },
    };
    Ok(SessionContextRequest { session })
}
fn serialized(value: impl Serialize) -> Result<Value, BoardClientError> {
    serde_json::to_value(value)
        .map_err(|_| ClientError::Protocol("view result could not be serialized").into())
}
pub(super) async fn dispatch(
    client: &CollaborationClient,
    command: PreparedViewCommand,
) -> Result<Value, BoardClientError> {
    match command {
        PreparedViewCommand::Work(request) => serialized(client.view_work(request).await?),
        PreparedViewCommand::Card(request) => serialized(client.view_card(request).await?),
        PreparedViewCommand::Owner(request) => serialized(client.view_owner(request).await?),
        PreparedViewCommand::LeadLoad(request) => serialized(client.view_lead_load(request).await?),
        PreparedViewCommand::Context(input) => serialized(
            client
                .view_session_context(context_request(input, client)?)
                .await?,
        ),
    }
}
