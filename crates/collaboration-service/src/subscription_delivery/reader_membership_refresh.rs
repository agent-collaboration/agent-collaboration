//! Refresh one explicit project set while retaining unaffected delivery records.
#[cfg(test)]
use super::OwnerObservation;
use super::{
    BoardAvailability, BoardError, ProjectSubscriptionRecord, ReaderDeliveryOwner, ReaderScanScope,
    ReaderSubscriptionSnapshot, target_session,
};

impl ReaderDeliveryOwner {
    pub(super) fn request_scan(&mut self, requested: ReaderScanScope) {
        self.rescan = true;
        match &mut self.pending_scope {
            Some(scope) => scope.merge(requested),
            None => self.pending_scope = Some(requested),
        }
    }
    pub(super) async fn refresh_subscription_records(
        &mut self,
    ) -> Result<ReaderSubscriptionSnapshot, BoardError> {
        let scope = self.pending_scope.take().unwrap_or(ReaderScanScope::All);
        let result = async {
            match &self.board_availability {
                BoardAvailability::Available(store) => {
                    let (projects, unavailable) = match &scope {
                        ReaderScanScope::All => {
                            let selected = store
                                .lock()
                                .await
                                .select_membership_projects(Some(&self.reader))
                                .await?;
                            (selected.projects, selected.unavailable)
                        }
                        ReaderScanScope::Projects(projects) => {
                            (projects.iter().cloned().collect(), Vec::new())
                        }
                    };
                    let port = self
                        .follow_admission
                        .read()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .clone();
                    let preventions = match port.and_then(|port| port.publisher.upgrade()) {
                        Some(publisher) => {
                            let memberships = store
                                .lock()
                                .await
                                .memberships_in_projects(Some(&self.reader), &projects)
                                .await?;
                            publisher
                                .generation_preventions(&projects, &memberships)
                                .await?
                        }
                        None => Vec::new(),
                    };
                    let mut store = store.lock().await;
                    if let Some(restoration) = &self.pending_restoration {
                        let restoring: Vec<_> = projects
                            .iter()
                            .filter(|project| match restoration {
                                ReaderScanScope::All => true,
                                ReaderScanScope::Projects(selected) => selected.contains(*project),
                            })
                            .cloned()
                            .collect();
                        store
                            .restore_project_subscription_windows(
                                &self.reader,
                                self.clock.now(),
                                &restoring,
                                &preventions,
                            )
                            .await?;
                    }
                    if self.rescan {
                        store
                            .rescan_project_subscription_windows(
                                &self.reader,
                                self.clock.now(),
                                &projects,
                                &preventions,
                            )
                            .await?;
                    }
                    let mut records = store
                        .project_subscription_records(
                            &self.reader,
                            self.clock.now(),
                            &projects,
                            &preventions,
                        )
                        .await?;
                    records.unavailable.extend(unavailable);
                    Ok::<_, BoardError>((projects, preventions, records))
                }
                BoardAvailability::Unavailable => Ok((
                    Vec::new(),
                    Vec::new(),
                    ReaderSubscriptionSnapshot::default(),
                )),
            }
        }
        .await;
        let (projects, preventions, loaded) = match result {
            Ok(loaded) => loaded,
            Err(error) => {
                self.pending_scope = Some(scope);
                return Err(error);
            }
        };
        self.pending_restoration = None;
        self.rescan = false;
        self.pass_projects = projects;
        self.pass_preventions = preventions;
        let mut current = match scope {
            ReaderScanScope::All => ReaderSubscriptionSnapshot::default(),
            ReaderScanScope::Projects(projects) => ReaderSubscriptionSnapshot {
                resolved_view_candidates: self
                    .prior_records
                    .resolved_view_candidates
                    .iter()
                    .filter(|prior| !projects.contains(&prior.project))
                    .cloned()
                    .collect(),
                records: self
                    .prior_records
                    .records
                    .iter()
                    .filter(|prior| !projects.contains(&prior.project))
                    .cloned()
                    .collect(),
                unavailable: self
                    .prior_records
                    .unavailable
                    .iter()
                    .filter(|project| !projects.contains(project))
                    .cloned()
                    .collect(),
            },
        };
        current.records.extend(loaded.records);
        current
            .resolved_view_candidates
            .extend(loaded.resolved_view_candidates);
        current.unavailable.extend(loaded.unavailable);
        current.unavailable.sort();
        current.unavailable.dedup();
        Ok(current)
    }
    pub(super) async fn activate_pending_waiters(&mut self, records: &[ProjectSubscriptionRecord]) {
        while let Some(waiter) = self.pending_waiters.pop_front() {
            let mut admitted = self.validate_poll_wait(&waiter.filter, records);
            if admitted.is_err()
                && let Ok(target) = target_session(&self.reader)
            {
                admitted = match self.push.store.pending_poll_mail(&target).await {
                    Ok(rows) if !rows.is_empty() => Ok(()),
                    Ok(_) => admitted,
                    Err(_) => Err(BoardError::board_unavailable()),
                };
            }
            match admitted {
                Ok(()) => {
                    self.waiters.push_back(waiter);
                    #[cfg(test)]
                    self.observe(OwnerObservation::WaitQueued);
                }
                Err(error) => {
                    let _ = waiter.reply.send(Err(error));
                }
            }
        }
    }
}
