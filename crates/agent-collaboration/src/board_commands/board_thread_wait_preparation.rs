//! Parse the subscription wait filter and deadline before calling the collaboration API.
use super::board_arguments::ThreadWaitArguments;
use super::board_preparation::{
    self, ActorInput, CommandContext, PreparedBoardCommand, placeholder_identity,
};
use super::board_thread_subscription_preparation::parse_subscription_duration;
use super::board_value_parsing::{command_context, parse_thread_ref, parse_uuid_v7};
use collaboration_client::board::{ProjectId, ThreadRef};
use collaboration_client::protocol::{
    MAX_SUBSCRIPTION_WAIT_SECONDS, ThreadSubscriptionWaitFilter, ThreadSubscriptionWaitRequest,
};
use std::time::Duration;

pub(super) struct PendingSubscriptionWait {
    pub request: ThreadSubscriptionWaitRequest,
    pub actor: ActorInput,
    pub timeout: Duration,
}

pub(super) fn prepare(
    arguments: ThreadWaitArguments,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    if !arguments.common.json {
        return Err("Thread Wait requires --json".into());
    }
    let actor = board_preparation::parse_actor_input(&arguments.actor)?;
    let thread_ids = arguments
        .thread_id
        .into_iter()
        .map(parse_thread_ref)
        .collect::<Result<Vec<_>, _>>()?;
    let filter = match (thread_ids.is_empty(), arguments.project_id) {
        (false, None) => {
            let mut unique_roots = Vec::with_capacity(thread_ids.len());
            for thread_id in thread_ids {
                if unique_roots
                    .iter()
                    .any(|existing: &ThreadRef| existing.thread == thread_id.thread)
                {
                    return Err("--thread-id values must be unique".into());
                }
                unique_roots.push(thread_id);
            }
            ThreadSubscriptionWaitFilter::Roots {
                threads: unique_roots,
            }
        }
        (true, Some(project)) => ThreadSubscriptionWaitFilter::Project {
            project_id: parse_uuid_v7::<ProjectId>(project, "--project-id")?,
        },
        (true, None) => ThreadSubscriptionWaitFilter::All {},
        (false, Some(_)) => {
            return Err("Choose repeated --thread-id values or one --project-id".into());
        }
    };
    let max_wait_seconds = parse_subscription_duration(&arguments.max_wait, "--max-wait", true)?;
    if max_wait_seconds > MAX_SUBSCRIPTION_WAIT_SECONDS {
        return Err("--max-wait must be at most 1500 seconds; for example --max-wait 10m".into());
    }
    let request = ThreadSubscriptionWaitRequest {
        protocol_version: collaboration_client::board::ProtocolVersion::CURRENT,
        acting_for: arguments
            .acting_for
            .as_deref()
            .map(super::board_value_parsing::parse_acting_for)
            .transpose()?,
        renewals: arguments
            .renewals
            .into_iter()
            .map(|value| {
                serde_json::from_str(&value)
                    .map_err(|_| "--renewal must be typed SubscriptionRenewalEntry JSON".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?,
        actor: match &actor {
            ActorInput::Explicit(identity) => identity.clone(),
            ActorInput::Self_ => placeholder_identity()?,
        },
        filter,
        max_wait_seconds,
    };
    request.validate().map_err(|error| error.to_string())?;
    let timeout = Duration::from_secs(max_wait_seconds).saturating_add(Duration::from_secs(5));
    Ok((
        PreparedBoardCommand::ThreadSubscriptionWait(PendingSubscriptionWait {
            request,
            actor,
            timeout,
        }),
        command_context(arguments.common),
    ))
}
