//! Registration starts and readiness observers share the existing project-writer owner.
use super::*;

impl ProjectRegistration for ProjectWriters {
    fn start<'runtime>(
        &'runtime self,
        project: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async move {
            self.start_registration(project).await.map_err(|error| {
                ProjectRegistrationError::Runtime {
                    message: error.to_string(),
                }
            })
        })
    }

    fn wait_ready<'runtime>(
        &'runtime self,
        project: &'runtime ProjectId,
    ) -> ProjectRegistrationFuture<'runtime> {
        Box::pin(async move {
            // Subscribe before inspecting the slot so an owner's completed
            // start cannot race past the observation. This future never starts
            // bootstrap; dropping a caller's timeout drops only its receiver.
            let mut starts = self.registration_starts.subscribe();
            #[cfg(test)]
            let _ = self.registration_waits.send(project.clone());
            loop {
                let slot = self.writers.lock().await.get(project).cloned();
                let writer = if let Some(slot) = slot {
                    let owner = slot.lock().await;
                    match &*owner {
                        ProjectWriterSlot::Ready(writer)
                            if writer.registration_watch.get().is_some() =>
                        {
                            Some(writer.clone())
                        }
                        ProjectWriterSlot::Terminal { .. } => {
                            return Err(ProjectRegistrationError::Runtime {
                                message: "project registration owner is terminal".to_owned(),
                            });
                        }
                        _ => None,
                    }
                } else {
                    None
                };
                if let Some(writer) = writer {
                    return writer
                        .runtime
                        .wait_registration()
                        .await
                        .map(ProjectRegistrationState::Outcome)
                        .map_err(|error| ProjectRegistrationError::Runtime {
                            message: error.to_string(),
                        });
                }
                match starts.recv().await {
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(error) => {
                        return Err(ProjectRegistrationError::Runtime {
                            message: error.to_string(),
                        });
                    }
                }
            }
        })
    }
}

impl ProjectWriters {
    async fn start_registration(
        &self,
        project: &ProjectId,
    ) -> io::Result<ProjectRegistrationState> {
        if self
            .registry()?
            .is_superseded(project)
            .await
            .map_err(io::Error::other)?
        {
            return Ok(ProjectRegistrationState::Superseded);
        }
        if let Some(retained) = self
            .registry()?
            .rejected_project(project)
            .await
            .map_err(io::Error::other)?
        {
            return Ok(ProjectRegistrationState::Outcome(
                retained.registration_outcome().clone(),
            ));
        }
        if *self.stopped.borrow() || self.requests.is_closed() {
            return Err(io::Error::other("machine session is unavailable"));
        }
        if !*self.welcomed.borrow() {
            // The durable settled directory outbox is replayed on Welcome.
            return Ok(ProjectRegistrationState::Pending);
        }
        // Fleet calls this port only after the directory acceptance is durable.
        // The Host owns bootstrap; admission cannot write before registration.
        self.registry()?
            .open_project(project)
            .await
            .map_err(io::Error::other)?;
        let writer = self.ensure(project).await?;
        writer.activate(project).await?;
        writer
            .runtime
            .start_registration()
            .await
            .map_err(io::Error::other)?;
        let outcome = writer
            .store
            .registration_outcome()
            .await
            .map_err(io::Error::other)?;
        self.watch_registration(project, writer).await;
        Ok(match outcome {
            WriterRegistrationOutcome::Pending => ProjectRegistrationState::Pending,
            outcome => ProjectRegistrationState::Outcome(outcome),
        })
    }

    async fn watch_registration(&self, project: &ProjectId, writer: Arc<ProjectWriter>) {
        writer
            .registration_watch
            .get_or_init(|| async {
                let project = project.clone();
                let watched = writer.clone();
                let registry = self.registry.clone();
                let lifecycle = self.lifecycle_signals.clone();
                let mut stopped = self.stopped.clone();
                let slot = self.writer_slot(&project).await;
                self.operations.lock().await.spawn(async move {
                    let outcome = tokio::select! {
                        result = watched.runtime.wait_registration() => result,
                        _ = stopped.changed() => return Ok(()),
                    };
                    match outcome {
                        Ok(WriterRegistrationOutcome::Ready { .. }) => {
                            lifecycle
                                .send(ProjectLifecycleSignal::RegistrationReady(project))
                                .map_err(io::Error::other)?;
                        }
                        Ok(WriterRegistrationOutcome::Refused {
                            reason: RegistrationRefusal::DatabaseRejected { .. },
                        }) => {
                            stop_writer_slot(&slot).await?;
                            drop(watched);
                            let registry = registry
                                .upgrade()
                                .ok_or_else(|| io::Error::other("project registry has closed"))?;
                            registry
                                .orphan_project(&project)
                                .await
                                .map_err(io::Error::other)?;
                        }
                        Ok(outcome) => tracing::debug!(
                            ?project,
                            ?outcome,
                            "native project registration settled"
                        ),
                        Err(error) => return Err(io::Error::other(error)),
                    }
                    Ok(())
                });
            })
            .await;
        let _ = self.registration_starts.send(project.clone());
    }
}
