//! Prepare typed thread operations before transmission.
use super::*;
pub(super) fn prepare_message(
    command: PostCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        PostCommand::Search(arguments) => {
            super::super::board_search_commands::prepare_message_search(arguments)
        }
        PostCommand::Post(arguments) => {
            let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
            let placement =
                match arguments.placement {
                    PlacementKind::Thread => PostTarget::Thread {
                        thread: parse_thread_ref(arguments.thread_id.ok_or(
                            "--thread-id is required; new roots use thread create --about",
                        )?)?,
                    },
                };
            let text_file = arguments.text_file.clone();
            let message_text = match (arguments.text, arguments.text_file) {
                (Some(value), None) => value,
                (None, Some(path)) => read_message_file(&path)?,
                _ => return Err("exactly one of --text or --text-file is required".into()),
            };
            let mut references = Vec::with_capacity(
                arguments.reference_post.len() + arguments.reference_thread.len(),
            );
            for value in arguments.reference_post {
                references.push(ReferenceTarget::Post {
                    post_id: parse_uuid_v7(value, "--reference-post")?,
                });
            }
            for value in arguments.reference_thread {
                references.push(ReferenceTarget::Thread {
                    thread_id: parse_uuid_v7(value, "--reference-thread")?,
                });
            }
            Ok((
                PreparedBoardCommand::ThreadPost(PendingThreadPost {
                    request: ThreadPostRequest {
                        addressees: parse_addressees(arguments.addressees)?,
                        request_id: arguments.identity.request.request_id()?,
                        protocol_version: ProtocolVersion::CURRENT,
                        resubmits: arguments.identity.request.resubmits()?,
                        observed_epoch: arguments.identity.request.observed_epoch()?,
                        post_id: parse_or_generate_uuid_v7(arguments.post_id, "--post-id")?,
                        placement,
                        actor,
                        acting_for,
                        text: parse_bounded_text(message_text, "--text/--text-file")?,
                        references: references
                            .try_into()
                            .map_err(|error: InvalidPostReferences| error.to_string())?,
                    },
                    text_file,
                }),
                command_context(arguments.common),
            ))
        }
        PostCommand::Show(arguments) => Ok((
            PreparedBoardCommand::PostShow(PostShowRequest {
                post_id: parse_uuid_v7(arguments.post_id, "--post-id")?,
            }),
            command_context(arguments.common),
        )),
        PostCommand::List(arguments) => {
            let scope = message_scope(&arguments)?;
            let selection = message_selection(&arguments)?;
            selection
                .validate()
                .map_err(|error| format!("--selection range is invalid: {error}"))?;
            Ok((
                PreparedBoardCommand::PostList(PostListRequest {
                    scope,
                    selection,
                    page: prepare_page_request(arguments.page)?,
                }),
                command_context(arguments.common),
            ))
        }
    }
}

pub(super) fn prepare_thread(
    command: ThreadCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        ThreadCommand::Create(arguments) => prepare_thread_create(arguments),
        ThreadCommand::Join(arguments) => prepare_thread_join(arguments),
        ThreadCommand::Subscribe(arguments) => {
            super::super::board_thread_subscription_preparation::prepare_subscribe(arguments)
        }
        ThreadCommand::Unsubscribe(arguments) => {
            super::super::board_thread_subscription_preparation::prepare_unsubscribe(arguments)
        }
        ThreadCommand::Subscriptions(arguments) => {
            super::super::board_thread_subscription_preparation::prepare_subscriptions(arguments)
        }
        ThreadCommand::Leave(arguments) => prepare_thread_leave(arguments),
        ThreadCommand::Participant { command } => prepare_thread_participant(command),
        ThreadCommand::Show(arguments) => Ok((
            PreparedBoardCommand::ThreadShow(ThreadShowRequest {
                thread: parse_thread_ref(arguments.thread_id)?,
                reader: read_actor(arguments.reader.as_deref())?,
                include_redacted: arguments.include_redacted,
            }),
            command_context(arguments.common),
        )),
        ThreadCommand::Resolve(arguments) => {
            let reason: ThreadResolutionReason = arguments
                .reason
                .try_into()
                .map_err(|error: TextValidationError| error.to_string())?;
            let request_id = arguments.mutation.identity.request.request_id()?;
            let observed_epoch = arguments.mutation.identity.request.observed_epoch()?;
            let resubmits = arguments.mutation.identity.request.resubmits()?;
            thread_mutation(
                arguments.mutation,
                PreparedBoardCommand::ThreadResolve,
                |thread, actor, acting_for| ThreadResolveRequest {
                    request_id,
                    protocol_version: ProtocolVersion::CURRENT,
                    resubmits,
                    observed_epoch,
                    thread,
                    actor,
                    acting_for,
                    reason,
                },
            )
        }
        ThreadCommand::Refile(arguments) => {
            let about = parse_thread_subject(&arguments.about)?;
            let request_id = arguments.mutation.identity.request.request_id()?;
            let observed_epoch = arguments.mutation.identity.request.observed_epoch()?;
            let resubmits = arguments.mutation.identity.request.resubmits()?;
            thread_mutation(
                arguments.mutation,
                PreparedBoardCommand::ThreadRefile,
                |thread, actor, acting_for| ThreadRefileRequest {
                    request_id,
                    protocol_version: ProtocolVersion::CURRENT,
                    resubmits,
                    observed_epoch,
                    thread,
                    actor,
                    acting_for,
                    about,
                },
            )
        }
        ThreadCommand::Unresolve(arguments) => {
            let request_id = arguments.identity.request.request_id()?;
            let observed_epoch = arguments.identity.request.observed_epoch()?;
            let resubmits = arguments.identity.request.resubmits()?;
            thread_mutation(
                arguments,
                PreparedBoardCommand::ThreadUnresolve,
                |thread, actor, acting_for| ThreadUnresolveRequest {
                    request_id,
                    protocol_version: ProtocolVersion::CURRENT,
                    observed_epoch,
                    resubmits,
                    thread,
                    actor,
                    acting_for,
                },
            )
        }
        ThreadCommand::Watch(arguments) => prepare_watch(arguments, true),
        ThreadCommand::Unwatch(arguments) => prepare_watch(arguments, false),
        ThreadCommand::List(arguments) => Ok((
            super::super::board_thread_list_preparation::prepare_thread_list(&arguments)?,
            command_context(arguments.common),
        )),
        ThreadCommand::Wait(arguments) => {
            super::super::board_thread_wait_preparation::prepare(arguments)
        }
    }
}

pub(super) fn prepare_watch(
    arguments: ThreadWatchArguments,
    watch: bool,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
    let command = match (arguments.thread_id, arguments.project_id, watch) {
        (Some(root), None, true) => PreparedBoardCommand::ThreadWatch(ThreadWatchRequest {
            request_id: arguments.identity.request.request_id()?,
            protocol_version: ProtocolVersion::CURRENT,
            resubmits: arguments.identity.request.resubmits()?,
            observed_epoch: arguments.identity.request.observed_epoch()?,
            thread: parse_thread_ref(root)?,
            actor,
            acting_for,
        }),
        (Some(root), None, false) => PreparedBoardCommand::ThreadUnwatch(ThreadUnwatchRequest {
            request_id: arguments.identity.request.request_id()?,
            protocol_version: ProtocolVersion::CURRENT,
            resubmits: arguments.identity.request.resubmits()?,
            observed_epoch: arguments.identity.request.observed_epoch()?,
            thread: parse_thread_ref(root)?,
            actor,
            acting_for,
        }),
        (None, Some(project), true) => PreparedBoardCommand::ProjectWatch(ProjectWatchRequest {
            request_id: arguments.identity.request.request_id()?,
            protocol_version: ProtocolVersion::CURRENT,
            resubmits: arguments.identity.request.resubmits()?,
            observed_epoch: arguments.identity.request.observed_epoch()?,
            project_id: parse_uuid_v7(project, "--project-id")?,
            actor,
            acting_for,
        }),
        (None, Some(project), false) => PreparedBoardCommand::ProjectUnwatch(ProjectWatchRequest {
            request_id: arguments.identity.request.request_id()?,
            protocol_version: ProtocolVersion::CURRENT,
            resubmits: arguments.identity.request.resubmits()?,
            observed_epoch: arguments.identity.request.observed_epoch()?,
            project_id: parse_uuid_v7(project, "--project-id")?,
            actor,
            acting_for,
        }),
        _ => return Err("Choose exactly one of --thread-id or --project-id".into()),
    };
    Ok((command, command_context(arguments.common)))
}

pub(super) fn prepare_thread_participant(
    command: ThreadParticipantCommand,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    match command {
        ThreadParticipantCommand::List(arguments) => {
            require_thread_json(&arguments.common, "Thread Participant List")?;
            Ok((
                PreparedBoardCommand::ThreadParticipantList(ThreadParticipantListRequest {
                    thread: parse_thread_ref(arguments.thread_id)?,
                    page: prepare_page_request(arguments.page)?,
                }),
                command_context(arguments.common),
            ))
        }
    }
}

pub(super) fn watch_choice(watch: bool, no_watch: bool) -> Result<bool, String> {
    match (watch, no_watch) {
        (true, false) => Ok(true),
        (false, true) => Ok(false),
        _ => Err("Choose exactly one Watch choice: --watch or --no-watch".into()),
    }
}

pub(super) fn participant_role(role: ParticipantRoleKind) -> ParticipantRole {
    match role {
        ParticipantRoleKind::Orchestrator => ParticipantRole::Orchestrator,
        ParticipantRoleKind::Implementer => ParticipantRole::Implementer,
        ParticipantRoleKind::Advisor => ParticipantRole::Advisor,
        ParticipantRoleKind::Reviewer => ParticipantRole::Reviewer,
        ParticipantRoleKind::Participant => ParticipantRole::Participant,
    }
}

pub(super) fn thread_references(
    message_references: Vec<String>,
    thread_references: Vec<String>,
) -> Result<PostReferences, String> {
    let mut references = Vec::with_capacity(message_references.len() + thread_references.len());
    for value in message_references {
        references.push(ReferenceTarget::Post {
            post_id: parse_uuid_v7(value, "--reference-post")?,
        });
    }
    for value in thread_references {
        references.push(ReferenceTarget::Thread {
            thread_id: parse_uuid_v7(value, "--reference-thread")?,
        });
    }
    references
        .try_into()
        .map_err(|error: InvalidPostReferences| error.to_string())
}

pub(super) fn prepare_thread_create(
    arguments: ThreadCreateArguments,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    require_thread_json(&arguments.common, "Thread Create")?;
    let actor = parse_actor_input(&arguments.actor)?;
    let acting_for = arguments
        .acting_for
        .as_deref()
        .map(parse_acting_for)
        .transpose()?;
    let role = arguments.role.map(participant_role);
    let watch = watch_choice(arguments.watch, arguments.no_watch)?;
    if matches!(&actor, ActorInput::Explicit(Identity::Session { .. })) && role.is_none() {
        return Err("Thread Create requires --role for a session actor".into());
    }
    let request = ThreadCreateRequest {
        about: parse_thread_subject(&arguments.about)?,
        addressees: parse_addressees(arguments.addressees)?,
        request_id: arguments.request.request_id()?,
        protocol_version: ProtocolVersion::CURRENT,
        resubmits: arguments.request.resubmits()?,
        observed_epoch: arguments.request.observed_epoch()?,
        post_id: parse_or_generate_uuid_v7(arguments.post_id, "--post-id")?,
        project_id: parse_uuid_v7(arguments.project_id, "--project-id")?,
        actor: match &actor {
            ActorInput::Explicit(identity) => identity.clone(),
            ActorInput::Self_ => placeholder_identity()?,
        },
        acting_for,
        text: parse_bounded_text(read_message_file(&arguments.text_file)?, "--text-file")?,
        references: thread_references(arguments.reference_post, arguments.reference_thread)?,
        role,
        watch,
    };
    Ok((
        PreparedBoardCommand::ThreadCreate(Box::new(PendingThreadCreate { request, actor })),
        command_context(arguments.common),
    ))
}

pub(super) fn prepare_thread_join(
    arguments: ThreadJoinArguments,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    require_thread_json(&arguments.common, "Thread Join")?;
    let actor = parse_actor_input(&arguments.actor)?;
    let role = participant_role(arguments.role);
    let replace = arguments
        .replace
        .map(|value| parse_identity(&value, "--replace"))
        .transpose()?;
    if replace.is_some() && role != ParticipantRole::Orchestrator {
        return Err("--replace requires --role orchestrator".into());
    }
    let request = ThreadJoinRequest {
        request_id: arguments.request.request_id()?,
        protocol_version: ProtocolVersion::CURRENT,
        resubmits: arguments.request.resubmits()?,
        observed_epoch: arguments.request.observed_epoch()?,
        mode: arguments
            .mode
            .map(super::super::board_thread_subscription_preparation::subscription_mode),
        when_idle: arguments
            .when_idle
            .map(super::super::board_thread_subscription_preparation::when_idle),
        thread: parse_thread_ref(arguments.thread_id)?,
        actor: match &actor {
            ActorInput::Explicit(identity) => identity.clone(),
            ActorInput::Self_ => placeholder_identity()?,
        },
        role,
        watch: !arguments.no_watch,
        replace,
        note: arguments
            .note
            .map(|value| parse_bounded_text(value, "--note"))
            .transpose()?,
    };
    Ok((
        PreparedBoardCommand::ThreadJoin(Box::new(PendingThreadJoin { request, actor })),
        command_context(arguments.common),
    ))
}

pub(super) fn prepare_thread_leave(
    arguments: ThreadLeaveArguments,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    require_thread_json(&arguments.common, "Thread Leave")?;
    let actor = parse_actor_input(&arguments.actor)?;
    let request = ThreadLeaveRequest {
        request_id: arguments.request.request_id()?,
        protocol_version: ProtocolVersion::CURRENT,
        resubmits: arguments.request.resubmits()?,
        observed_epoch: arguments.request.observed_epoch()?,
        thread: parse_thread_ref(arguments.thread_id)?,
        actor: match &actor {
            ActorInput::Explicit(identity) => identity.clone(),
            ActorInput::Self_ => placeholder_identity()?,
        },
        after_leave: match (arguments.to, arguments.resolve) {
            (Some(to), None) => Some(LeaveFollowUp::HandTo {
                to: parse_identity(&to, "--to")?,
            }),
            (None, Some(reason)) => Some(LeaveFollowUp::Resolve {
                reason: reason
                    .try_into()
                    .map_err(|error: TextValidationError| error.to_string())?,
            }),
            (None, None) => None,
            _ => return Err("--to conflicts with --resolve".to_owned()),
        },
    };
    Ok((
        PreparedBoardCommand::ThreadLeave(PendingThreadLeave { request, actor }),
        command_context(arguments.common),
    ))
}

pub(super) fn require_thread_json(common: &CommonArguments, command: &str) -> Result<(), String> {
    if common.json {
        Ok(())
    } else {
        Err(format!("{command} requires --json"))
    }
}

pub(super) fn thread_mutation<TRequest>(
    arguments: ThreadMutationArguments,
    wrap: impl FnOnce(TRequest) -> PreparedBoardCommand,
    build: impl FnOnce(ThreadRef, Identity, Option<ActingForIdentity>) -> TRequest,
) -> Result<(PreparedBoardCommand, CommandContext), String> {
    let (actor, acting_for) = parse_mutation_identity(&arguments.identity)?;
    let request = build(parse_thread_ref(arguments.thread_id)?, actor, acting_for);
    Ok((wrap(request), command_context(arguments.common)))
}
