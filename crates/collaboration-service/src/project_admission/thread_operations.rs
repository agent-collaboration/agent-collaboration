use super::{
    ProjectAdmissionError,
    operation_context::{ConversationMutation, OperationContext, OperationPlan},
};
use message_board::{
    BoardOperation, FeedCursor, FeedPosition, Identity, ImplementerHolder, LifecycleFact,
    NotifyTarget, OrchestratorHolder, Participant, ParticipantClosedReason, ParticipantProps,
    ParticipantRole, Placement, Post, PostCooldownSeconds, PostId, PostReferences, PostText,
    ReadMark, ReadScope, Record, RecordId, RecordKind, RecordPlacement, RecordRef,
    RecordedParticipantRole, ReferenceTarget, Refusal, SubscriptionPolicyPatch, SubscriptionScope,
    Thread, ThreadId, ThreadRefusal, ThreadResolutionReason, ThreadState, ThreadSubject,
};
use message_board_storage::ProjectWriteTransaction;

impl OperationContext<'_> {
    pub(super) async fn thread_plan(
        &self,
        write: &mut ProjectWriteTransaction,
    ) -> Result<Result<OperationPlan, Refusal>, ProjectAdmissionError> {
        match &self.request.operation {
            BoardOperation::ThreadCreate {
                post_id,
                about,
                text,
                references,
                participation,
                watch,
                addressees,
            } => {
                let mut plan = match self.create_thread_plan(ThreadCreatePlan {
                    post_id,
                    about,
                    text,
                    references,
                    participation: *participation,
                    watch: *watch,
                    addressees,
                }) {
                    Ok(plan) => plan,
                    Err(reason) => return Ok(Err(reason)),
                };
                if *watch && matches!(self.request.actor, Identity::Session { .. }) {
                    match self.subscribe_plan(
                        &SubscriptionScope::thread(ThreadId::from(post_id.clone())),
                        &SubscriptionPolicyPatch::default(),
                        None,
                    ) {
                        Ok(membership) => {
                            plan.activated_memberships
                                .extend(membership.activated_memberships);
                            plan.records.extend(membership.records);
                            plan.conversation.extend(membership.conversation);
                        }
                        Err(reason) => return Ok(Err(reason)),
                    }
                }
                Ok(Ok(plan))
            }
            BoardOperation::ThreadRefile { thread_id, about } => {
                Ok(self.refile_thread_plan(thread_id, about))
            }
            BoardOperation::ThreadPost {
                post_id,
                thread_id,
                text,
                references,
                addressees,
            } => {
                let participants = write.participants(thread_id).await?;
                let mut plan = match self.post_plan(ThreadPostPlan {
                    post_id,
                    thread_id,
                    text,
                    references,
                    addressees,
                    participants: &participants,
                }) {
                    Ok(plan) => plan,
                    Err(reason) => return Ok(Err(reason)),
                };
                let thread_scope = SubscriptionScope::thread(thread_id.clone());
                let thread_membership =
                    write.membership(&self.request.actor, &thread_scope).await?;
                let covering = match thread_membership {
                    Some(membership) => Some(membership),
                    None => {
                        write
                            .membership(
                                &self.request.actor,
                                &SubscriptionScope::project(self.request.project_id.clone()),
                            )
                            .await?
                    }
                };
                if write
                    .effective_thread_watch(thread_id, &self.request.actor)
                    .await?
                    .is_some()
                {
                    match self.renew_membership_plan(covering.as_ref()) {
                        Ok(renewal) => {
                            plan.activated_memberships
                                .extend(renewal.activated_memberships);
                            plan.records.extend(renewal.records);
                            plan.conversation.extend(renewal.conversation);
                        }
                        Err(reason) => return Ok(Err(reason)),
                    }
                }
                Ok(Ok(plan))
            }
            BoardOperation::ThreadResolve { thread_id, reason } => {
                let participants = write.participants(thread_id).await?;
                Ok(self.resolve_thread_plan(thread_id, reason, &participants))
            }
            BoardOperation::ThreadUnresolve { thread_id } => {
                Ok(self.unresolve_thread_plan(thread_id))
            }
            BoardOperation::ReadMarkInitialize { reader, scope } => {
                if let Err(reason) = self.check_read_mark_scope(reader, scope) {
                    return Ok(Err(reason));
                }
                let (current, _) = write.normalize_read_mark(reader, scope, self.now).await?;
                if current.is_some() {
                    Ok(Ok(OperationPlan::unchanged()))
                } else {
                    Ok(self.advance_read_mark(reader, scope, self.high_water, None))
                }
            }
            BoardOperation::ReadMarkAdvance {
                reader,
                scope,
                through,
            } => {
                if let Err(reason) = self.check_read_mark_scope(reader, scope) {
                    return Ok(Err(reason));
                }
                if write.cursor_validity(*through).await?
                    != message_board_storage::CursorValidity::Valid
                {
                    return Ok(Err(Refusal::InvalidShape {
                        field: "through".into(),
                    }));
                }
                let (current, _) = write.normalize_read_mark(reader, scope, self.now).await?;
                Ok(self.advance_read_mark(reader, scope, *through, current.as_ref()))
            }
            _ => Ok(Err(Refusal::InvalidShape {
                field: "operation".into(),
            })),
        }
    }

    fn create_thread_plan(&self, input: ThreadCreatePlan<'_>) -> Result<OperationPlan, Refusal> {
        let thread_id = ThreadId::from(input.post_id.clone());
        self.require_new(input.post_id.clone().into())?;
        self.require_new(thread_id.clone().into())?;
        if matches!(self.request.actor, Identity::Session { .. }) && input.participation.is_none() {
            return Err(Refusal::InvalidShape {
                field: "participation".into(),
            });
        }
        let latest_top_level = self.records.iter().filter_map(|record| match record {
            Record::Post(post)
                if post.actor == self.request.actor
                    && matches!(&post.placement, Placement::Project { project_id } if project_id == &self.request.project_id) =>
            {
                post.envelope.time.as_router_time()
            }
            _ => None,
        }).max();
        if let Some(previous) = latest_top_level
            && let Some(retry_after_seconds) = remaining_post_cooldown(self.now, previous)?
        {
            return Err(Refusal::TopLevelPostCooldown {
                retry_after_seconds,
            });
        }
        self.validate_thread_subject(input.about)?;
        if matches!(input.about, ThreadSubject::Project { .. }) {
            self.require_project_role()?;
        }
        let first_post = self.next_cursor(2)?;
        let orchestrator = if input.participation == Some(ParticipantRole::Orchestrator) {
            Some(OrchestratorHolder {
                identity: self.request.actor.clone(),
                last_seen_position: first_post,
            })
        } else {
            None
        };
        let implementer = if input.participation == Some(ParticipantRole::Implementer) {
            Some(ImplementerHolder {
                identity: self.request.actor.clone(),
                last_seen_position: first_post,
            })
        } else {
            None
        };
        let thread = Thread {
            envelope: self.envelope(thread_id.clone().into()),
            thread_id: thread_id.clone(),
            about: input.about.clone(),
            imported: false,
            state: ThreadState::Open,
            orchestrator,
            implementer,
        };
        let post = self.post_record(
            input.post_id,
            Placement::Project {
                project_id: self.request.project_id.clone(),
            },
            input.text,
            input.references,
            input.participation,
        )?;
        let mut plan = OperationPlan::changed(vec![Record::Thread(thread), Record::Post(post)]);
        if let Some(role) = input.participation {
            let participant = Participant::new(ParticipantProps {
                identity: self.request.actor.clone(),
                role: RecordedParticipantRole::Recorded(role),
                note: None,
                joined_at_position: first_post,
                last_seen_position: first_post,
                closed_at_position: None,
                closed_reason: None,
                replaced_by: None,
            })
            .map_err(|_| Refusal::InvalidShape {
                field: "participation".into(),
            })?;
            plan.conversation.push(ConversationMutation::Participant {
                thread: thread_id.clone(),
                participant,
            });
        }
        plan.conversation.push(ConversationMutation::Watch {
            thread: thread_id,
            reader: self.request.actor.clone(),
            starts_after: first_post,
            active: input.watch,
        });
        add_targets(&mut plan, input.post_id.clone().into(), input.addressees);
        Ok(plan)
    }
    fn post_plan(&self, input: ThreadPostPlan<'_>) -> Result<OperationPlan, Refusal> {
        self.require_new(input.post_id.clone().into())?;
        let thread = self.thread(input.thread_id)?;
        if thread.state == ThreadState::Resolved {
            return Err(Refusal::Closed {
                subject: input.thread_id.clone().into(),
            });
        }
        let participant = input.participants.iter().find(|participant| {
            participant.identity == self.request.actor && participant.is_open()
        });
        if matches!(self.request.actor, Identity::Session { .. }) && participant.is_none() {
            return Err(Refusal::Thread(Box::new(
                ThreadRefusal::ParticipantRequired {
                    thread_id: input.thread_id.clone(),
                    actor: self.request.actor.clone(),
                },
            )));
        }
        let post = self.post_record(
            input.post_id,
            Placement::Thread {
                thread_id: input.thread_id.clone(),
            },
            input.text,
            input.references,
            participant
                .map(|participant| match participant.role {
                    RecordedParticipantRole::Recorded(role) => Ok(role),
                    RecordedParticipantRole::NotRecorded => Err(Refusal::InvalidShape {
                        field: "participant.role".into(),
                    }),
                })
                .transpose()?,
        )?;
        let mut plan = OperationPlan::changed(vec![Record::Post(post)]);
        if let Some(mut participant) = participant.cloned() {
            participant.last_seen_position = self.next_cursor(1)?;
            plan.conversation.push(ConversationMutation::Participant {
                thread: input.thread_id.clone(),
                participant,
            });
        }
        add_targets(&mut plan, input.post_id.clone().into(), input.addressees);
        Ok(plan)
    }
    pub(super) fn resolve_thread_plan(
        &self,
        thread_id: &ThreadId,
        reason: &ThreadResolutionReason,
        participants: &[Participant],
    ) -> Result<OperationPlan, Refusal> {
        let mut thread = self.thread(thread_id)?;
        if matches!(self.request.actor, Identity::Session { .. })
            && !participants.iter().any(|participant| {
                participant.identity == self.request.actor && participant.is_open()
            })
        {
            return Err(Refusal::Thread(Box::new(
                ThreadRefusal::ParticipantRequired {
                    thread_id: thread_id.clone(),
                    actor: self.request.actor.clone(),
                },
            )));
        }
        if let Identity::Session { .. } = self.request.actor
            && !thread
                .orchestrator
                .as_ref()
                .is_some_and(|holder| holder.identity == self.request.actor)
        {
            return Err(Refusal::Thread(Box::new(
                ThreadRefusal::OrchestratorRequired {
                    thread_id: thread_id.clone(),
                    actor: self.request.actor.clone(),
                    holder: thread
                        .orchestrator
                        .as_ref()
                        .map(|holder| holder.identity.clone()),
                    last_seen: thread
                        .orchestrator
                        .as_ref()
                        .map(|holder| holder.last_seen_position),
                },
            )));
        }
        if thread.state == ThreadState::Resolved {
            return Ok(OperationPlan::unchanged());
        }
        thread.state = ThreadState::Resolved;
        thread.orchestrator = None;
        thread.implementer = None;
        thread.envelope = self.envelope(thread_id.clone().into());
        let mut plan = OperationPlan::changed(vec![
            Record::Thread(thread),
            Record::Fact(self.lifecycle_fact(LifecycleFact::ThreadResolved {
                thread: thread_id.clone(),
                reason: reason.clone(),
            })),
        ]);
        let closed_at = self.next_cursor(1)?;
        for mut participant in participants
            .iter()
            .filter(|participant| participant.is_open())
            .cloned()
        {
            participant.last_seen_position = closed_at;
            participant.closed_at_position = Some(closed_at);
            participant.closed_reason = Some(ParticipantClosedReason::Resolved);
            participant.replaced_by = None;
            plan.conversation.push(ConversationMutation::Participant {
                thread: thread_id.clone(),
                participant,
            });
        }
        self.end_resolved_memberships(&mut plan, thread_id)?;
        Ok(plan)
    }
    fn unresolve_thread_plan(&self, thread_id: &ThreadId) -> Result<OperationPlan, Refusal> {
        let mut thread = self.thread(thread_id)?;
        if thread.state == ThreadState::Open {
            return Ok(OperationPlan::unchanged());
        }
        thread.state = ThreadState::Open;
        thread.envelope = self.envelope(thread_id.clone().into());
        Ok(OperationPlan::changed(vec![Record::Thread(thread)]))
    }
    fn advance_read_mark(
        &self,
        reader: &Identity,
        scope: &ReadScope,
        through: FeedCursor,
        current: Option<&ReadMark>,
    ) -> Result<OperationPlan, Refusal> {
        self.check_read_mark_scope(reader, scope)?;
        if through.position > self.high_water.position {
            return Err(Refusal::InvalidShape {
                field: "through".into(),
            });
        }
        if current.is_some_and(|mark| mark.through.position >= through.position) {
            return Ok(OperationPlan::unchanged());
        }
        let mut envelope =
            self.envelope(current.map_or_else(RecordId::generate, |mark| mark.envelope.id.clone()));
        envelope.position = RecordPlacement::Unsequenced;
        Ok(OperationPlan::changed(vec![Record::ReadMark(ReadMark {
            envelope,
            reader: reader.clone(),
            scope: scope.clone(),
            through,
        })]))
    }

    fn check_read_mark_scope(&self, reader: &Identity, scope: &ReadScope) -> Result<(), Refusal> {
        if reader != &self.request.actor {
            return Err(Refusal::InvalidShape {
                field: "reader".into(),
            });
        }
        match scope {
            ReadScope::Project { project_id } if *project_id != self.request.project_id => {
                return Err(Refusal::NotFound(project_id.clone().into()));
            }
            ReadScope::Thread { thread_id } => {
                self.thread(thread_id)?;
            }
            ReadScope::Project { .. } => {}
        }
        Ok(())
    }
    pub(super) fn thread(&self, id: &ThreadId) -> Result<Thread, Refusal> {
        match self.record(&id.clone().into())? {
            Record::Thread(thread) => Ok(thread.clone()),
            _ => Err(Refusal::NotFound(id.clone().into())),
        }
    }
    pub(super) fn next_cursor(&self, offset: u64) -> Result<FeedCursor, Refusal> {
        Ok(FeedCursor {
            epoch: self.admitted_epoch,
            position: FeedPosition::try_from(self.high_water.position.get().saturating_add(offset))
                .map_err(|_| Refusal::InvalidShape {
                    field: "position".into(),
                })?,
        })
    }
    fn post_record(
        &self,
        id: &PostId,
        placement: Placement,
        text: &PostText,
        references: &[RecordRef],
        role: Option<ParticipantRole>,
    ) -> Result<Post, Refusal> {
        let mut targets = Vec::with_capacity(references.len());
        for reference in references {
            let target = match reference.kind {
                RecordKind::Post => ReferenceTarget::Post {
                    post_id: PostId::try_from(reference.id.as_str().to_owned()).map_err(|_| {
                        Refusal::InvalidShape {
                            field: "references".into(),
                        }
                    })?,
                },
                RecordKind::Thread => ReferenceTarget::Thread {
                    thread_id: ThreadId::try_from(reference.id.as_str().to_owned()).map_err(
                        |_| Refusal::InvalidShape {
                            field: "references".into(),
                        },
                    )?,
                },
                _ => {
                    return Err(Refusal::InvalidShape {
                        field: "references".into(),
                    });
                }
            };
            targets.push(target);
        }
        let board_id = self
            .scope
            .board
            .clone()
            .ok_or_else(|| Refusal::InvalidShape {
                field: "board".into(),
            })?;
        Ok(Post {
            envelope: self.envelope(id.clone().into()),
            post_id: id.clone(),
            board_id,
            project_id: self.request.project_id.clone(),
            placement,
            actor: self.request.actor.clone(),
            acting_for: self.request.acting_for.clone(),
            text: text.clone(),
            references: PostReferences::try_from(targets).map_err(|_| Refusal::InvalidShape {
                field: "references".into(),
            })?,
            position: self.envelope(id.clone().into()).position,
            posted_as_role: role,
        })
    }
}

fn remaining_post_cooldown(
    now: chrono::DateTime<chrono::Utc>,
    previous: chrono::DateTime<chrono::Utc>,
) -> Result<Option<PostCooldownSeconds>, Refusal> {
    let elapsed = now
        .signed_duration_since(previous)
        .num_milliseconds()
        .max(0);
    let remaining = 60_000_i64.saturating_sub(elapsed);
    if remaining <= 0 {
        return Ok(None);
    }
    let seconds = u64::try_from((remaining + 999) / 1_000).map_err(|_| Refusal::InvalidShape {
        field: "routerTime".to_owned(),
    })?;
    PostCooldownSeconds::try_from(seconds)
        .map(Some)
        .map_err(|_| Refusal::InvalidShape {
            field: "routerTime".to_owned(),
        })
}

struct ThreadCreatePlan<'input> {
    post_id: &'input PostId,
    about: &'input ThreadSubject,
    text: &'input PostText,
    references: &'input [RecordRef],
    participation: Option<ParticipantRole>,
    watch: bool,
    addressees: &'input [NotifyTarget],
}
struct ThreadPostPlan<'input> {
    post_id: &'input PostId,
    thread_id: &'input ThreadId,
    text: &'input PostText,
    references: &'input [RecordRef],
    addressees: &'input [NotifyTarget],
    participants: &'input [Participant],
}
fn add_targets(plan: &mut OperationPlan, cause: RecordRef, targets: &[NotifyTarget]) {
    for target in targets {
        if !plan
            .intents
            .iter()
            .any(|(existing_cause, existing)| existing_cause == &cause && existing == target)
        {
            plan.intents.push((cause.clone(), target.clone()));
        }
    }
}

#[cfg(test)]
mod cooldown_tests {
    use super::remaining_post_cooldown;
    #[test]
    #[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
    fn cooldown_rounds_up_and_caps_a_regressed_clock() -> Result<(), Box<dyn std::error::Error>> {
        let previous = chrono::DateTime::from_timestamp_millis(100_000).ok_or("time")?;
        for (elapsed, expected) in [
            (-1, Some(60)),
            (0, Some(60)),
            (999, Some(60)),
            (1_000, Some(59)),
            (59_999, Some(1)),
            (60_000, None),
            (120_000, None),
        ] {
            let now = chrono::DateTime::from_timestamp_millis(100_000 + elapsed).ok_or("time")?;
            assert_eq!(
                remaining_post_cooldown(now, previous)
                    .map_err(|error| format!("{error:?}"))?
                    .map(PostCooldownSeconds::get),
                expected
            );
        }
        Ok(())
    }
    use message_board::PostCooldownSeconds;
}
