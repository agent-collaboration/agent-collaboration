use super::{CollaborationFailure, CollaborationRuntime, io};

impl CollaborationRuntime {
    /// The lifecycle event loop must monitor this future; listener death is not healthy readiness.
    pub async fn listener_failure(&mut self) -> CollaborationFailure {
        loop {
            let failure = tokio::select! {
                task = self.tasks.join_next() => CollaborationFailure::LocalListener(match task {
                    Some(Ok(Err(error))) => error,
                    Some(Err(_)) => io::Error::other("collaboration listener task failed"),
                    _ => io::Error::other("collaboration listener stopped unexpectedly"),
                }),
                task = self.provider_retirement_tasks.join_next(), if !self.provider_retirement_tasks.is_empty() => CollaborationFailure::LocalListener(match task {
                    Some(Ok(Ok(()))) => continue,
                    Some(Ok(Err(error))) => error,
                    Some(Err(_)) => io::Error::other("provider retirement publication task failed"),
                    None => continue,
                }),
                failure = self.collaboration_api.failure() => CollaborationFailure::LocalListener(failure),
                failure = async {
                    match &mut self.tailscale_exposure {
                        Some(exposure) => exposure.failure().await,
                        None => std::future::pending().await,
                    }
                } => CollaborationFailure::TailnetExposure(failure),
            };
            self.manifest.take();
            self.shutdown.cancel();
            if let Some(publisher) = &self.notification_publisher {
                publisher.cancel();
            }
            if let Some(service) = &self.subscription_delivery {
                service.cancel();
            }
            let _retire = self.publication.admission_gate().retire();
            return failure;
        }
    }
    pub async fn shutdown(mut self) -> io::Result<()> {
        self.manifest.take();
        self.shutdown.cancel();
        let mut failure = None;
        if let Some(mut exposure) = self.tailscale_exposure.take()
            && let Err(error) = exposure.shutdown().await
        {
            failure.get_or_insert(error);
        }
        if let Some(publisher) = self.notification_publisher.take() {
            publisher.shutdown().await;
        }
        if let Some(service) = self.subscription_delivery.take() {
            service.shutdown().await;
        }
        if let Err(error) = self.collaboration_api.stopped().await {
            failure.get_or_insert(error);
        }
        if let Err(error) = self.publication.admission_gate().retire() {
            failure.get_or_insert(error);
        }
        if let Some(session) = self.board_session.take()
            && let Err(error) = session.shutdown().await
        {
            let refusal_observed =
                if super::board_composition::RunningMachineSession::is_expected_refusal_error(
                    &error,
                ) {
                    match self.fleet_link.read().await {
                        Ok(Some(record)) => {
                            super::board_composition::RunningMachineSession::is_observed_refusal(
                                &error, &record,
                            )
                        }
                        Err(observation_error) => {
                            tracing::warn!(%observation_error, session_error = %error, "Fleet link observation failed during shutdown; retaining session failure");
                            false
                        }
                        Ok(None) => false,
                    }
                } else {
                    false
                };
            if refusal_observed {
                tracing::info!(event = "fleet.link.refusalObservedAtShutdown", %error, "Fleet refusal was already observed; Host cleanup completed");
            } else {
                failure.get_or_insert(error);
            }
        }
        if let Some(route) = self.provider_delivery_route.take() {
            route.shutdown_queue().await;
        }
        if let Some(supervisor) = self.external_provider_supervisor.take()
            && let Err(message) = supervisor.shutdown().await
        {
            failure.get_or_insert(io::Error::other(message));
        }
        while let Some(result) = self.provider_retirement_tasks.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    failure.get_or_insert(error);
                }
                Err(_) => {
                    failure.get_or_insert(io::Error::other(
                        "provider retirement publication task failed",
                    ));
                }
            }
        }
        self.drain_native_observer().await;
        while let Some(result) = self.tasks.join_next().await {
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    failure.get_or_insert(error);
                }
                Err(_) => {
                    failure.get_or_insert(io::Error::other("collaboration task shutdown failed"));
                }
            }
        }
        if let Some(maintenance) = self.maintenance.take() {
            match maintenance.await {
                Ok(Ok(())) => {}
                _ => {
                    failure.get_or_insert(io::Error::other("lifecycle maintenance failed"));
                }
            }
        }
        if let Some(task) = self.automation_task.take()
            && task.await.is_err()
        {
            failure.get_or_insert(io::Error::other("automation worker shutdown failed"));
        }
        if let Some(task) = self.schedule_task.take()
            && task.await.is_err()
        {
            failure.get_or_insert(io::Error::other("schedule worker shutdown failed"));
        }
        if let Some(task) = self.automation_maintenance.take()
            && task.await.is_err()
        {
            failure.get_or_insert(io::Error::other("automation maintenance shutdown failed"));
        }
        if let Some(task) = self.fleet_outbox_maintenance.take()
            && task.await.is_err()
        {
            failure.get_or_insert(io::Error::other("fleet outbox maintenance shutdown failed"));
        }
        if let Some(task) = self.provider_retention.take()
            && task.await.is_err()
        {
            failure.get_or_insert(io::Error::other("provider retention maintenance failed"));
        }
        if let Some(task) = self.scratch_sweep.take()
            && task.await.is_err()
        {
            failure.get_or_insert(io::Error::other("scratch sweep shutdown failed"));
        }
        if let Some(store) = self.automation_store.take() {
            match std::sync::Arc::try_unwrap(store) {
                Ok(store) => {
                    if store.into_inner().close().await.is_err() {
                        failure.get_or_insert(io::Error::other("automation storage close failed"));
                    }
                }
                Err(_) => {
                    failure.get_or_insert(io::Error::other(
                        "automation storage still owned after listener shutdown",
                    ));
                }
            }
        }
        if let Some(store) = self.provider_store.take() {
            match std::sync::Arc::try_unwrap(store) {
                Ok(store) => {
                    if store.into_inner().close().await.is_err() {
                        failure.get_or_insert(io::Error::other(
                            "provider operation storage close failed",
                        ));
                    }
                }
                Err(_) => {
                    failure.get_or_insert(io::Error::other(
                        "provider operation storage still owned after listener shutdown",
                    ));
                }
            }
        }
        if let Some(store) = self.project_admission.take() {
            match std::sync::Arc::try_unwrap(store) {
                Ok(store) => {
                    if store.close().await.is_err() {
                        failure.get_or_insert(io::Error::other("board storage close failed"));
                    }
                }
                Err(_) => {
                    failure.get_or_insert(io::Error::other(
                        "board storage still owned after listener shutdown",
                    ));
                }
            }
        }
        // Last user gone: the link owner ended with the board session and the Fleet
        // client with admission, so nothing can still write through a view.
        if let Some(fleet_store) = self.fleet_store.take() {
            fleet_store.close().await;
        }
        if let Some(journal) = self.journal.take() {
            match std::sync::Arc::try_unwrap(journal) {
                Ok(journal) => {
                    journal.close().await;
                }
                Err(_) => {
                    failure.get_or_insert(io::Error::other(
                        "lifecycle storage still owned after listener shutdown",
                    ));
                }
            }
        }
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}
