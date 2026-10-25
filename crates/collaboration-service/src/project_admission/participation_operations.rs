//! Conversation seats stay independent of project authority, with their retained refusals.
use super::{
    ProjectAdmissionError,
    operation_context::{ConversationMutation, OperationContext, OperationPlan},
};
use message_board::*;
use message_board_storage::ProjectWriteTransaction;

impl OperationContext<'_> {
    pub(super) async fn participation_plan(
        &self,
        write: &mut ProjectWriteTransaction,
    ) -> Result<Result<OperationPlan, Refusal>, ProjectAdmissionError> {
        match &self.request.operation {
            BoardOperation::ThreadJoin {
                thread_id,
                role,
                watch,
                mode,
                when_idle,
                replace,
                note,
            } => {
                let participants = write.participants(thread_id).await?;
                let plan = self.join_participant_plan(JoinPlan {
                    thread_id,
                    role: *role,
                    replace: replace.as_ref(),
                    note: note.as_ref(),
                    participants: &participants,
                });
                let mut plan = match plan {
                    Ok(plan) => plan,
                    Err(reason) => return Ok(Err(reason)),
                };
                let scope = SubscriptionScope::thread(thread_id.clone());
                if let Some(replaced) = replace {
                    let membership = write.membership(replaced, &scope).await?;
                    match self.end_membership_plan(membership.as_ref(), EndReason::Replaced) {
                        Ok(end) => plan.records.extend(end.records),
                        Err(reason) => return Ok(Err(reason)),
                    }
                }
                let existing = write.membership(&self.request.actor, &scope).await?;
                let membership = if *watch {
                    self.subscribe_plan(
                        &scope,
                        &SubscriptionPolicyPatch {
                            mode: *mode,
                            when_idle: *when_idle,
                            ..SubscriptionPolicyPatch::default()
                        },
                        existing.as_ref(),
                    )
                } else {
                    self.end_membership_plan(existing.as_ref(), EndReason::Cancelled)
                };
                match membership {
                    Ok(member) => {
                        plan.activated_memberships
                            .extend(member.activated_memberships);
                        plan.records.extend(member.records);
                        plan.conversation.extend(member.conversation);
                    }
                    Err(reason) => return Ok(Err(reason)),
                }
                plan.conversation.push(ConversationMutation::Watch {
                    thread: thread_id.clone(),
                    reader: self.request.actor.clone(),
                    starts_after: self.high_water,
                    active: *watch,
                });
                Ok(Ok(plan))
            }
            BoardOperation::ThreadLeave {
                thread_id,
                after_leave,
            } => {
                let participants = write.participants(thread_id).await?;
                let result =
                    self.leave_participant_plan(thread_id, after_leave.as_ref(), &participants);
                let mut plan = match result {
                    Ok(plan) => plan,
                    Err(reason) => return Ok(Err(reason)),
                };
                let scope = SubscriptionScope::thread(thread_id.clone());
                let existing = write.membership(&self.request.actor, &scope).await?;
                let reason = if matches!(after_leave, Some(LeaveFollowUp::HandTo { .. })) {
                    EndReason::Replaced
                } else {
                    EndReason::Left
                };
                match self.end_membership_plan(existing.as_ref(), reason) {
                    Ok(end) => plan.records.extend(end.records),
                    Err(reason) => return Ok(Err(reason)),
                }
                plan.conversation.push(ConversationMutation::Watch {
                    thread: thread_id.clone(),
                    reader: self.request.actor.clone(),
                    starts_after: self.high_water,
                    active: false,
                });
                if matches!(after_leave, Some(LeaveFollowUp::Resolve { .. })) {
                    self.end_resolved_memberships(&mut plan, thread_id)
                        .map_err(|reason| {
                            ProjectAdmissionError::InvalidRecord(format!(
                                "resolution membership: {reason:?}"
                            ))
                        })?;
                }
                Ok(Ok(plan))
            }
            _ => Ok(Err(Refusal::InvalidShape {
                field: "operation".into(),
            })),
        }
    }

    fn join_participant_plan(&self, input: JoinPlan<'_>) -> Result<OperationPlan, Refusal> {
        let mut thread = self.thread(input.thread_id)?;
        if thread.state == ThreadState::Resolved {
            return Err(Refusal::Closed {
                subject: input.thread_id.clone().into(),
            });
        }
        if input.replace == Some(&self.request.actor) {
            return Err(thread_refusal(ThreadRefusal::SelfReplace {
                thread_id: input.thread_id.clone(),
                actor: self.request.actor.clone(),
            }));
        }
        if input.replace.is_some()
            && !matches!(
                input.role,
                ParticipantRole::Orchestrator | ParticipantRole::Implementer
            )
        {
            return Err(Refusal::InvalidShape {
                field: "replace".into(),
            });
        }
        let own = input.participants.iter().find(|participant| {
            participant.identity == self.request.actor && participant.is_open()
        });
        let previous = input
            .participants
            .iter()
            .filter(|participant| participant.identity == self.request.actor)
            .max_by_key(|participant| participant.joined_at_position.position);
        if own.is_some_and(|participant| {
            participant.role == RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
        }) && input.role != ParticipantRole::Orchestrator
        {
            return Err(thread_refusal(ThreadRefusal::OrchestratorRoleChange {
                thread_id: input.thread_id.clone(),
                actor: self.request.actor.clone(),
            }));
        }
        let holder = if matches!(
            input.role,
            ParticipantRole::Orchestrator | ParticipantRole::Implementer
        ) {
            input.participants.iter().find(|participant| {
                participant.role == RecordedParticipantRole::Recorded(input.role)
                    && participant.is_open()
            })
        } else {
            None
        };
        match (holder, input.replace) {
            (Some(holder), None) if holder.identity != self.request.actor => {
                let refusal = if input.role == ParticipantRole::Orchestrator {
                    ThreadRefusal::OrchestratorAlreadyExists {
                        thread_id: input.thread_id.clone(),
                        actor: self.request.actor.clone(),
                        holder: holder.identity.clone(),
                        last_seen: holder.last_seen_position,
                    }
                } else {
                    ThreadRefusal::ImplementerAlreadyExists {
                        thread_id: input.thread_id.clone(),
                        actor: self.request.actor.clone(),
                        holder: holder.identity.clone(),
                        last_seen: holder.last_seen_position,
                    }
                };
                return Err(thread_refusal(refusal));
            }
            (current, Some(named)) if current.is_none_or(|holder| holder.identity != *named) => {
                let holder = current.map(|holder| holder.identity.clone());
                let last_seen = current.map(|holder| holder.last_seen_position);
                let refusal = if input.role == ParticipantRole::Orchestrator {
                    ThreadRefusal::StaleOrchestrator {
                        thread_id: input.thread_id.clone(),
                        actor: self.request.actor.clone(),
                        holder,
                        last_seen,
                        named_holder: named.clone(),
                    }
                } else {
                    ThreadRefusal::StaleImplementer {
                        thread_id: input.thread_id.clone(),
                        actor: self.request.actor.clone(),
                        holder,
                        last_seen,
                        named_holder: named.clone(),
                    }
                };
                return Err(thread_refusal(refusal));
            }
            _ => {}
        }
        let position = self.next_cursor(1)?;
        let mut plan = OperationPlan::changed(Vec::new());
        if let Some(holder) = holder
            && input.replace.is_some()
        {
            let mut ended = holder.clone();
            ended.last_seen_position = position;
            ended.closed_at_position = Some(position);
            ended.closed_reason = Some(ParticipantClosedReason::Replaced);
            ended.replaced_by = Some(self.request.actor.clone());
            plan.conversation.push(ConversationMutation::Participant {
                thread: input.thread_id.clone(),
                participant: ended,
            });
        }
        if let Some(own) = own {
            let mut ended = own.clone();
            ended.last_seen_position = position;
            ended.closed_at_position = Some(position);
            ended.closed_reason = Some(ParticipantClosedReason::Rejoined);
            ended.replaced_by = None;
            plan.conversation.push(ConversationMutation::Participant {
                thread: input.thread_id.clone(),
                participant: ended,
            });
        }
        let participant = Participant::new(ParticipantProps {
            identity: self.request.actor.clone(),
            role: RecordedParticipantRole::Recorded(input.role),
            note: input
                .note
                .cloned()
                .or_else(|| previous.and_then(|previous| previous.note.clone())),
            joined_at_position: position,
            last_seen_position: position,
            closed_at_position: None,
            closed_reason: None,
            replaced_by: None,
        })
        .map_err(|_| Refusal::InvalidShape {
            field: "participant".into(),
        })?;
        update_seat(&mut thread, &participant);
        thread.envelope = self.envelope(input.thread_id.clone().into());
        plan.records.push(Record::Thread(thread));
        plan.conversation.push(ConversationMutation::Participant {
            thread: input.thread_id.clone(),
            participant,
        });
        Ok(plan)
    }

    fn leave_participant_plan(
        &self,
        thread_id: &ThreadId,
        follow_up: Option<&LeaveFollowUp>,
        participants: &[Participant],
    ) -> Result<OperationPlan, Refusal> {
        let mut thread = self.thread(thread_id)?;
        let Some(own) = participants.iter().find(|participant| {
            participant.identity == self.request.actor && participant.is_open()
        }) else {
            return Err(thread_refusal(ThreadRefusal::ParticipantRequired {
                thread_id: thread_id.clone(),
                actor: self.request.actor.clone(),
            }));
        };
        if own.role == RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
            && follow_up.is_none()
        {
            return Err(thread_refusal(
                ThreadRefusal::OrchestratorHandoverRequired {
                    thread_id: thread_id.clone(),
                    actor: self.request.actor.clone(),
                },
            ));
        }
        if own.role != RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator)
            && follow_up.is_some()
        {
            return Err(Refusal::InvalidShape {
                field: "afterLeave".into(),
            });
        }
        if let Some(LeaveFollowUp::Resolve { reason }) = follow_up {
            return self.resolve_thread_plan(thread_id, reason, participants);
        }
        let position = self.next_cursor(1)?;
        let mut ended = own.clone();
        ended.last_seen_position = position;
        ended.closed_at_position = Some(position);
        ended.closed_reason = Some(ParticipantClosedReason::Left);
        ended.replaced_by = None;
        let mut plan = OperationPlan::changed(Vec::new());
        if let Some(LeaveFollowUp::HandTo { to }) = follow_up {
            if *to == self.request.actor {
                return Err(Refusal::InvalidShape {
                    field: "afterLeave.to".into(),
                });
            }
            let Some(target) = participants
                .iter()
                .find(|participant| participant.identity == *to && participant.is_open())
            else {
                return Err(thread_refusal(
                    ThreadRefusal::HandoverTargetNotParticipant {
                        thread_id: thread_id.clone(),
                        actor: self.request.actor.clone(),
                        target: to.clone(),
                    },
                ));
            };
            let mut former_target = target.clone();
            former_target.last_seen_position = position;
            former_target.closed_at_position = Some(position);
            former_target.closed_reason = Some(ParticipantClosedReason::Promoted);
            former_target.replaced_by = None;
            plan.conversation.push(ConversationMutation::Participant {
                thread: thread_id.clone(),
                participant: former_target,
            });
            let target = Participant::new(ParticipantProps {
                identity: target.identity.clone(),
                role: RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator),
                note: target.note.clone(),
                joined_at_position: position,
                last_seen_position: position,
                closed_at_position: None,
                closed_reason: None,
                replaced_by: None,
            })
            .map_err(|_| Refusal::InvalidShape {
                field: "participant".into(),
            })?;
            ended.closed_reason = Some(ParticipantClosedReason::Replaced);
            ended.replaced_by = Some(to.clone());
            update_seat(&mut thread, &target);
            plan.conversation.push(ConversationMutation::Participant {
                thread: thread_id.clone(),
                participant: target,
            });
        } else {
            if own.role == RecordedParticipantRole::Recorded(ParticipantRole::Implementer) {
                thread.implementer = None;
            }
        }
        thread.envelope = self.envelope(thread_id.clone().into());
        plan.records.push(Record::Thread(thread));
        // End the former unique seat before promoting another participant.
        plan.conversation.insert(
            0,
            ConversationMutation::Participant {
                thread: thread_id.clone(),
                participant: ended,
            },
        );
        Ok(plan)
    }

    pub(super) fn end_resolved_memberships(
        &self,
        plan: &mut OperationPlan,
        thread: &ThreadId,
    ) -> Result<(), Refusal> {
        let scope = SubscriptionScope::thread(thread.clone());
        let ended_at = self.next_cursor(1)?;
        for record in &self.records {
            if let Record::SubscriptionMembership(member) = record
                && member.scope() == &scope
                && member.ended().is_none()
            {
                // A leave may already have ended its actor's membership in this plan.
                if plan
                    .records
                    .iter()
                    .any(|row| row.reference() == record.reference())
                {
                    continue;
                }
                let mut props: SubscriptionMembershipProps = member.clone().into();
                props.envelope = self.envelope(props.envelope.id.clone());
                props.ended = Some(MembershipEnd {
                    reason: EndReason::Resolved,
                    position: ended_at,
                    at: self.now,
                });
                let ended =
                    SubscriptionMembership::new(props).map_err(|_| Refusal::InvalidShape {
                        field: "membership".into(),
                    })?;
                plan.records.push(Record::SubscriptionMembership(ended));
            }
        }
        Ok(())
    }
}
struct JoinPlan<'input> {
    thread_id: &'input ThreadId,
    role: ParticipantRole,
    replace: Option<&'input Identity>,
    note: Option<&'input ParticipantNote>,
    participants: &'input [Participant],
}
fn thread_refusal(refusal: ThreadRefusal) -> Refusal {
    Refusal::Thread(Box::new(refusal))
}
fn update_seat(thread: &mut Thread, participant: &Participant) {
    match participant.role {
        RecordedParticipantRole::Recorded(ParticipantRole::Orchestrator) => {
            if thread
                .implementer
                .as_ref()
                .is_some_and(|holder| holder.identity == participant.identity)
            {
                thread.implementer = None;
            }
            thread.orchestrator = Some(OrchestratorHolder {
                identity: participant.identity.clone(),
                last_seen_position: participant.last_seen_position,
            });
        }
        RecordedParticipantRole::Recorded(ParticipantRole::Implementer) => {
            thread.implementer = Some(ImplementerHolder {
                identity: participant.identity.clone(),
                last_seen_position: participant.last_seen_position,
            });
        }
        _ => {
            if thread
                .implementer
                .as_ref()
                .is_some_and(|holder| holder.identity == participant.identity)
            {
                thread.implementer = None;
            }
        }
    }
}
