//! A recipient's command channel and completion jointly define its reader lifetime.
use super::{
    BoardAvailability, BoardError, CancellationToken, Identity, ReaderDeliveryCommand,
    ReaderDeliveryOwner, ReaderDeliveryOwnerProps, ReaderScanScope, SubscriptionDeliveryService,
    WithSubscriber, mpsc,
};

#[derive(Clone)]
pub(super) struct ReaderOwnerHandle {
    pub(super) commands: mpsc::Sender<ReaderDeliveryCommand>,
    pub(super) completed: CancellationToken,
}

impl SubscriptionDeliveryService {
    pub(super) async fn ensure_owner(
        &self,
        reader: Identity,
    ) -> Result<mpsc::Sender<ReaderDeliveryCommand>, BoardError> {
        self.ensure_owner_input(reader, ReaderScanScope::All, None)
            .await
    }
    pub(super) async fn submit_reader_command(
        &self,
        reader: Identity,
        scope: ReaderScanScope,
        command: ReaderDeliveryCommand,
    ) -> Result<(), BoardError> {
        self.ensure_owner_input(reader, scope, Some(command))
            .await
            .map(|_| ())
    }
    pub(super) async fn ensure_owner_input(
        &self,
        reader: Identity,
        scope: ReaderScanScope,
        mut command: Option<ReaderDeliveryCommand>,
    ) -> Result<mpsc::Sender<ReaderDeliveryCommand>, BoardError> {
        loop {
            if self.inner.shutdown.is_cancelled() {
                return Err(BoardError::board_unavailable());
            }
            // Keep the map gate through spawn: no Reader can acquire two selection owners.
            let mut owners = self.inner.owners.lock().await;
            if let Some(handle) = owners.get(&reader).cloned() {
                if handle.commands.is_closed() {
                    if !handle.completed.is_cancelled() {
                        // Closing admission precedes draining accepted native I/O.
                        // Release the map gate: unrelated recipients keep progressing.
                        drop(owners);
                        tokio::select! {
                            () = self.inner.shutdown.cancelled() => return Err(BoardError::board_unavailable()),
                            () = handle.completed.cancelled() => {},
                        }
                        continue;
                    }
                } else {
                    let sender = handle.commands;
                    drop(owners);
                    if let Some(input) = command.take()
                        && let Err(error) = sender.send(input).await
                    {
                        command = Some(error.0);
                        continue;
                    }
                    return Ok(sender);
                }
            }
            let activity = match &self.inner.board_availability {
                BoardAvailability::Available(store) => {
                    Some(store.lock().await.subscribe_activity())
                }
                BoardAvailability::Unavailable => None,
            };
            let (sender, commands) = mpsc::channel(64);
            if let Some(input) = command.take() {
                sender
                    .send(input)
                    .await
                    .map_err(|_| BoardError::board_unavailable())?;
            }
            let owner = ReaderDeliveryOwner::new(ReaderDeliveryOwnerProps {
                notification_startup_attempts: self
                    .inner
                    .notification_startup_attempts
                    .get()
                    .cloned()
                    .ok_or_else(BoardError::board_unavailable)?,
                #[cfg(test)]
                poll_handover_probe: self.inner.poll_handover_probe.clone(),
                #[cfg(test)]
                idle_owner_close_probe: self.inner.idle_owner_close_probe.clone(),
                native_mail: self.inner.native_mail.clone(),
                follow_admission: self.inner.follow_admission.clone(),
                reader: reader.clone(),
                initial_scope: scope.clone(),
                board_availability: self.inner.board_availability.clone(),
                push: self.inner.push.clone(),
                presence: self.inner.presence.clone(),
                clock: self.inner.clock.clone(),
                shutdown: self.inner.shutdown.clone(),
                commands,
                activity,
                #[cfg(test)]
                observations: self.inner.observations.clone(),
            });
            let completed = CancellationToken::new();
            let completion = completed.clone().drop_guard();
            owners.insert(
                reader.clone(),
                ReaderOwnerHandle {
                    commands: sender.clone(),
                    completed,
                },
            );
            let inner = self.inner.clone();
            let owner_sender = sender.clone();
            self.inner.tasks.spawn(
                async move {
                    // Completion is signalled on normal exit, cancellation or panic.
                    let _completion = completion;
                    Box::pin(owner.run()).await;
                    let mut owners = inner.owners.lock().await;
                    if owners
                        .get(&reader)
                        .is_some_and(|current| current.commands.same_channel(&owner_sender))
                    {
                        owners.remove(&reader);
                    }
                }
                .with_current_subscriber(),
            );
            return Ok(sender);
        }
    }
}
