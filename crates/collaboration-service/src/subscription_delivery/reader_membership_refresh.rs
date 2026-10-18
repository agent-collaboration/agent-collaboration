//! Refresh one explicit project set while retaining unaffected delivery records.
use super::*;

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
                    let mut store = store.lock().await;
                    let (projects, unavailable) = match &scope {
                        ReaderScanScope::All => {
                            let selected =
                                store.select_membership_projects(Some(&self.reader)).await?;
                            (selected.projects, selected.unavailable)
                        }
                        ReaderScanScope::Projects(projects) => {
                            (projects.iter().cloned().collect(), Vec::new())
                        }
                    };
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
                            )
                            .await?;
                    }
                    if self.rescan {
                        store
                            .rescan_project_subscription_windows(
                                &self.reader,
                                self.clock.now(),
                                &projects,
                            )
                            .await?;
                    }
                    let mut records = store
                        .project_subscription_records(&self.reader, self.clock.now(), &projects)
                        .await?;
                    records.unavailable.extend(unavailable);
                    Ok::<_, BoardError>((projects, records))
                }
                BoardAvailability::Unavailable => {
                    Ok((Vec::new(), ReaderSubscriptionSnapshot::default()))
                }
            }
        }
        .await;
        let (projects, loaded) = match result {
            Ok(loaded) => loaded,
            Err(error) => {
                self.pending_scope = Some(scope);
                return Err(error);
            }
        };
        self.pending_restoration = None;
        self.rescan = false;
        self.pass_projects = projects;
        let mut current = match scope {
            ReaderScanScope::All => ReaderSubscriptionSnapshot::default(),
            ReaderScanScope::Projects(projects) => ReaderSubscriptionSnapshot {
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
        current.unavailable.extend(loaded.unavailable);
        current.unavailable.sort();
        current.unavailable.dedup();
        Ok(current)
    }
    pub(super) fn activate_pending_waiters(&mut self, records: &[ProjectSubscriptionRecord]) {
        while let Some(waiter) = self.pending_waiters.pop_front() {
            match self.validate_poll_wait(&waiter.filter, records) {
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
