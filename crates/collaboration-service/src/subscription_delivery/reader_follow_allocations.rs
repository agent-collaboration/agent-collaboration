//! Due windows become proposals; admission and native publishing own every effect.
use super::*;

impl ReaderDeliveryOwner {
    pub(super) async fn expire_due_follow_memberships(&mut self) -> Result<(), BoardError> {
        let BoardAvailability::Available(store) = &self.board_availability else {
            return Ok(());
        };
        let candidates = {
            let store = store.lock().await;
            let projects = match &self.pending_scope {
                Some(ReaderScanScope::Projects(projects)) => projects.iter().cloned().collect(),
                Some(ReaderScanScope::All) | None => {
                    store.membership_projects(Some(&self.reader)).await?
                }
            };
            store
                .membership_expiry_candidates(&self.reader, &projects)
                .await?
        };
        let now = self.clock.now();
        let monotonic = self.clock.monotonic_now();
        self.membership_expiry_deadline = candidates
            .iter()
            .map(|(_, proposal)| proposal.expires_at)
            .min()
            .map(|expires| wall_deadline(expires, now, monotonic));
        let proposals: Vec<_> = candidates
            .into_iter()
            .filter(|(_, proposal)| now >= proposal.expires_at)
            .collect();
        if proposals.is_empty() {
            return Ok(());
        }
        let port = self
            .follow_admission
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some((admission, publisher)) =
            port.and_then(|port| port.admission.upgrade().zip(port.publisher.upgrade()))
        else {
            return Err(BoardError::board_unavailable());
        };
        for (project, proposal) in proposals {
            if admission
                .expire_follow_membership(&project, proposal, self.clock.now())
                .await
                .map_err(|error| {
                    tracing::warn!(%error, project = project.as_str(), "membership expiry admission delayed");
                    BoardError::board_unavailable()
                })?
            {
                publisher.queue_project(project.clone()).await?;
                self.request_scan(ReaderScanScope::project(project));
            }
        }
        Ok(())
    }

    pub(super) async fn allocate_due_follow(
        &mut self,
        due: &[message_board::ThreadId],
        facts: &HashMap<
            message_board::ThreadId,
            super::super::subscription_facts::RootSubscriptionFacts,
        >,
    ) -> Result<bool, BoardError> {
        // Human polling keeps its bodyless board ranges; SDK addresses are sessions.
        if !matches!(self.reader, Identity::Session { .. }) {
            return Ok(false);
        }
        let roots: Vec<_> = due
            .iter()
            .filter(|root| {
                facts
                    .get(*root)
                    .is_some_and(|facts| facts.mode != message_board::SubscriptionMode::Off)
            })
            .cloned()
            .collect();
        if roots.is_empty() {
            return Ok(false);
        }
        let port = self
            .follow_admission
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let Some((admission, publisher)) =
            port.and_then(|port| port.admission.upgrade().zip(port.publisher.upgrade()))
        else {
            return Err(BoardError::board_unavailable());
        };
        let target = super::super::subscription_push::target_session(&self.reader)?;
        // Directory IO is complete before either the window gate or project lock.
        let service =
            message_board::ServiceId::try_from(String::from(target.endpoint.service_id.clone()))
                .map_err(|_| BoardError::board_unavailable())?;
        let enrollment = publisher.follow_recipient_enrollment(&service).await?;
        let proposals = self
            .board_store()?
            .lock()
            .await
            .propose_follow_batches(
                &self.reader,
                &roots,
                self.clock.now(),
                &self.pass_projects,
                &self.pass_preventions,
            )
            .await?;
        let mut allocated = false;
        for proposal in proposals {
            let input = crate::project_admission::FollowBatchProposal {
                membership: proposal.membership,
                activated_at: proposal.activated_at,
                generation: proposal.generation,
                roots: proposal.roots,
                item: fleet_sync_protocol::ItemId::new(uuid::Uuid::now_v7())
                    .map_err(|_| BoardError::board_unavailable())?,
                enrollment: enrollment.clone(),
                preventions: proposal.preventions,
            };
            if admission
                .allocate_follow_batch(&proposal.project, input, self.clock.now())
                .await
                .map_err(|_| BoardError::board_unavailable())?
                .is_some()
            {
                publisher.queue_project(proposal.project.clone()).await?;
                allocated = true;
            }
        }
        if allocated {
            self.request_scan(ReaderScanScope::Projects(
                self.pass_projects.iter().cloned().collect(),
            ));
        }
        Ok(allocated)
    }
}
