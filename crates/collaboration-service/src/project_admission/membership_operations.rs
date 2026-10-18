//! Declared subscription/watch changes share the project transaction and request ledger.
use super::{
    ProjectAdmissionError,
    operation_context::{ConversationMutation, OperationContext, OperationPlan},
};
use message_board::*;
use message_board_storage::ProjectWriteTransaction;

impl OperationContext<'_> {
    pub(super) async fn materialize_covered_post_watches(
        &self,
        write: &mut ProjectWriteTransaction,
        plan: &mut OperationPlan,
    ) -> Result<(), ProjectAdmissionError> {
        if !plan
            .records
            .iter()
            .any(|record| matches!(record, Record::Post(_)))
        {
            return Ok(());
        }
        let readers = write.live_project_watchers().await?;
        for record in &plan.records {
            let Record::Post(post) = record else { continue };
            let thread = match &post.placement {
                Placement::Thread { thread_id } => thread_id.clone(),
                Placement::Project { .. } => ThreadId::from(post.post_id.clone()),
            };
            for reader in &readers {
                // Both disabled rows and an explicit watch choice in this request win over
                // project coverage. This effect belongs to the post, never delivery.
                if write.thread_watch_exists(&thread, reader).await?
                    || plan.conversation.iter().any(|mutation| {
                        matches!(mutation,
                        ConversationMutation::Watch { thread: watched, reader: watched_reader, .. }
                        if watched == &thread && watched_reader == reader)
                    })
                {
                    continue;
                }
                // Persist the coverage's original history boundary. A policy change
                // or this post's new records must not move the reader's watch start.
                let starts_after = write.project_watch(reader).await?.ok_or_else(|| {
                    ProjectAdmissionError::InvalidRecord(
                        "live project watch disappeared inside its write transaction".to_owned(),
                    )
                })?;
                plan.conversation.push(ConversationMutation::Watch {
                    thread: thread.clone(),
                    reader: reader.clone(),
                    starts_after,
                    active: true,
                });
            }
        }
        Ok(())
    }

    pub(super) async fn membership_plan(
        &self,
        write: &mut ProjectWriteTransaction,
    ) -> Result<Result<OperationPlan, Refusal>, ProjectAdmissionError> {
        match &self.request.operation {
            BoardOperation::ThreadSubscribe { scope, policy } => {
                if let Err(reason) = self.validate_subscription_scope(scope) {
                    return Ok(Err(reason));
                }
                if let SubscriptionScope::Thread { thread_id } = scope
                    && !write
                        .participants(thread_id)
                        .await?
                        .iter()
                        .any(|participant| {
                            participant.identity == self.request.actor && participant.is_open()
                        })
                {
                    return Ok(Err(Refusal::Thread(Box::new(
                        ThreadRefusal::ParticipantRequired {
                            thread_id: thread_id.clone(),
                            actor: self.request.actor.clone(),
                        },
                    ))));
                }
                let existing = write.membership(&self.request.actor, scope).await?;
                Ok(self.subscribe_plan(scope, policy, existing.as_ref()))
            }
            BoardOperation::ThreadUnsubscribe { scope } => {
                if let Err(reason) = self.validate_subscription_scope(scope) {
                    return Ok(Err(reason));
                }
                let existing = write.membership(&self.request.actor, scope).await?;
                Ok(self.end_membership_plan(existing.as_ref(), EndReason::Cancelled))
            }
            BoardOperation::ThreadRenew { scope } => {
                if let Err(reason) = self.validate_subscription_scope(scope) {
                    return Ok(Err(reason));
                }
                let existing = write.membership(&self.request.actor, scope).await?;
                Ok(self.renew_membership_plan(existing.as_ref()))
            }
            BoardOperation::ThreadWatch { thread_id }
            | BoardOperation::ThreadUnwatch { thread_id } => {
                if let Err(reason) = self.thread(thread_id) {
                    return Ok(Err(reason));
                }
                let active = matches!(self.request.operation, BoardOperation::ThreadWatch { .. });
                let current = write
                    .effective_thread_watch(thread_id, &self.request.actor)
                    .await?;
                if current.is_some() == active
                    && (active
                        || write
                            .thread_watch_exists(thread_id, &self.request.actor)
                            .await?)
                {
                    return Ok(Ok(OperationPlan::unchanged()));
                }
                let mut plan = OperationPlan::changed(Vec::new());
                plan.conversation.push(ConversationMutation::Watch {
                    thread: thread_id.clone(),
                    reader: self.request.actor.clone(),
                    starts_after: self.high_water,
                    active,
                });
                if !active {
                    let membership = write
                        .membership(
                            &self.request.actor,
                            &SubscriptionScope::thread(thread_id.clone()),
                        )
                        .await?;
                    match self.end_membership_plan(membership.as_ref(), EndReason::Cancelled) {
                        Ok(end) => plan.records.extend(end.records),
                        Err(reason) => return Ok(Err(reason)),
                    }
                }
                Ok(Ok(plan))
            }
            BoardOperation::ProjectWatch {} | BoardOperation::ProjectUnwatch {} => {
                let active = matches!(self.request.operation, BoardOperation::ProjectWatch {});
                let current = write.project_watch(&self.request.actor).await?;
                if current.is_some() == active {
                    return Ok(Ok(OperationPlan::unchanged()));
                }
                let mut plan = OperationPlan::changed(Vec::new());
                plan.conversation.push(ConversationMutation::ProjectWatch {
                    reader: self.request.actor.clone(),
                    starts_after: self.high_water,
                    active,
                });
                if !active {
                    let membership = write
                        .membership(
                            &self.request.actor,
                            &SubscriptionScope::project(self.request.project_id.clone()),
                        )
                        .await?;
                    match self.end_membership_plan(membership.as_ref(), EndReason::Cancelled) {
                        Ok(end) => plan.records.extend(end.records),
                        Err(reason) => return Ok(Err(reason)),
                    }
                }
                Ok(Ok(plan))
            }
            BoardOperation::ThreadJoin { .. } | BoardOperation::ThreadLeave { .. } => {
                self.participation_plan(write).await
            }
            _ => Ok(Err(Refusal::InvalidShape {
                field: "operation".into(),
            })),
        }
    }

    pub(super) fn validate_subscription_scope(
        &self,
        scope: &SubscriptionScope,
    ) -> Result<(), Refusal> {
        match scope {
            SubscriptionScope::Thread { thread_id } => self.thread(thread_id).map(|_| ()),
            SubscriptionScope::Project { project_id } if *project_id == self.request.project_id => {
                Ok(())
            }
            SubscriptionScope::Project { project_id } => {
                Err(Refusal::NotFound(project_id.clone().into()))
            }
        }
    }
    pub(super) fn subscribe_plan(
        &self,
        scope: &SubscriptionScope,
        patch: &SubscriptionPolicyPatch,
        existing: Option<&SubscriptionMembership>,
    ) -> Result<OperationPlan, Refusal> {
        let base_policy = existing.map_or_else(
            || SubscriptionPolicy::defaults_for(&self.request.actor),
            |member| member.policy().clone(),
        );
        let policy = base_policy
            .apply_patch(&self.request.actor, patch)
            .map_err(|_| Refusal::InvalidShape {
                field: "policy".into(),
            })?;
        let generation = match existing {
            Some(member)
                if member.state_at(self.now, None, false) == SubscriptionState::Active
                    && member.policy() == &policy =>
            {
                member.generation()
            }
            Some(member) => member
                .generation()
                .next()
                .map_err(|_| Refusal::InvalidShape {
                    field: "generation".into(),
                })?,
            None => SubscriptionGeneration::new(1).map_err(|_| Refusal::InvalidShape {
                field: "generation".into(),
            })?,
        };
        let seconds =
            i64::try_from(policy.lifetime().seconds()).map_err(|_| Refusal::InvalidShape {
                field: "lifetime".into(),
            })?;
        let expires_at = self
            .now
            .checked_add_signed(chrono::Duration::seconds(seconds))
            .ok_or_else(|| Refusal::InvalidShape {
                field: "expiresAt".into(),
            })?;
        let id = existing.map_or_else(RecordId::generate, |member| member.envelope.id.clone());
        let remains_active = existing.is_some_and(|member| {
            member.state_at(self.now, None, false) == SubscriptionState::Active
        });
        let activated_at = if remains_active {
            existing.map_or(self.high_water, SubscriptionMembership::activated_at)
        } else {
            self.high_water // apply_plan binds the activation to this request's ledger position.
        };
        let member = SubscriptionMembership::new(SubscriptionMembershipProps {
            envelope: self.envelope(id.clone()),
            reader: self.request.actor.clone(),
            scope: scope.clone(),
            policy,
            renewed_at: self.now,
            expires_at,
            generation,
            activated_at,
            ended: None,
        })
        .map_err(|_| Refusal::InvalidShape {
            field: "membership".into(),
        })?;
        let mut plan = OperationPlan::changed(vec![Record::SubscriptionMembership(member)]);
        if !remains_active {
            plan.activated_memberships.push(id);
        }
        match scope {
            SubscriptionScope::Thread { thread_id } => {
                plan.conversation.push(ConversationMutation::Watch {
                    thread: thread_id.clone(),
                    reader: self.request.actor.clone(),
                    starts_after: self.high_water,
                    active: true,
                })
            }
            SubscriptionScope::Project { .. } => {
                plan.conversation.push(ConversationMutation::ProjectWatch {
                    reader: self.request.actor.clone(),
                    starts_after: self.high_water,
                    active: true,
                })
            }
        }
        Ok(plan)
    }
    pub(super) fn end_membership_plan(
        &self,
        existing: Option<&SubscriptionMembership>,
        reason: EndReason,
    ) -> Result<OperationPlan, Refusal> {
        let Some(existing) = existing else {
            return Ok(OperationPlan::unchanged());
        };
        if existing.ended().is_some()
            || existing.state_at(self.now, None, false)
                == (SubscriptionState::Ended {
                    reason: EndReason::Expired,
                })
        {
            return Ok(OperationPlan::unchanged());
        }
        let mut props: SubscriptionMembershipProps = existing.clone().into();
        props.envelope = self.envelope(props.envelope.id.clone());
        props.ended = Some(MembershipEnd {
            reason,
            position: self.next_cursor(1)?,
            at: self.now,
        });
        let member = SubscriptionMembership::new(props).map_err(|_| Refusal::InvalidShape {
            field: "membership".into(),
        })?;
        Ok(OperationPlan::changed(vec![
            Record::SubscriptionMembership(member),
        ]))
    }
    pub(super) fn renew_membership_plan(
        &self,
        existing: Option<&SubscriptionMembership>,
    ) -> Result<OperationPlan, Refusal> {
        let Some(existing) = existing else {
            return Ok(OperationPlan::unchanged());
        };
        if existing.state_at(self.now, None, false) != SubscriptionState::Active {
            return Ok(OperationPlan::unchanged());
        }
        self.subscribe_plan(
            existing.scope(),
            &SubscriptionPolicyPatch::default(),
            Some(existing),
        )
    }
}
