//! Session preparation may fail while its independent link observation remains readable.
use board_fleet_sync::{
    BoardSessionInputs, FleetLinkObservation, FleetLinkOwner, FleetLinkPublisher, FleetLinkReader,
};
use message_board_storage::FleetCache;
use std::{io, path::Path, sync::Arc};
use tokio::task::JoinSet;

pub(super) struct PreparedMachineLink {
    pub cache: Arc<FleetCache>,
    pub publisher: FleetLinkPublisher,
    pub reader: FleetLinkReader,
    pub tasks: JoinSet<io::Result<()>>,
}
impl PreparedMachineLink {
    pub async fn prepare(directory: &Path, inputs: &BoardSessionInputs) -> io::Result<Self> {
        tokio::fs::create_dir_all(directory).await?;
        let cache = Arc::new(
            FleetCache::open(&directory.join("fleet-cache.sqlite"))
                .await
                .map_err(io::Error::other)?,
        );
        let (publisher, reader, owner) =
            FleetLinkOwner::prepare(cache.clone(), inputs.peer, inputs.endpoint.as_str())
                .await
                .map_err(io::Error::other)?;
        let mut tasks = JoinSet::new();
        tasks.spawn(async move { owner.run().await.map_err(io::Error::other) });
        Ok(Self {
            cache,
            publisher,
            reader,
            tasks,
        })
    }
    pub async fn failed(self, error: &io::Error) -> FleetLinkObservation {
        let Self {
            publisher,
            reader,
            mut tasks,
            ..
        } = self;
        match error
            .get_ref()
            .and_then(|error| error.downcast_ref::<fleet_sync_session::ClientError>())
        {
            Some(error) => publisher.session_failed(error),
            None => match error
                .get_ref()
                .and_then(|error| error.downcast_ref::<fleet_sync_session::ConnectFailure>())
            {
                Some(error) => publisher
                    .session_stopped(&fleet_sync_session::ClientError::Connect(error.clone())),
                None => match error.get_ref().and_then(|error| {
                    error.downcast_ref::<message_board_storage::RouterLocalStorageError>()
                }) {
                    Some(message_board_storage::RouterLocalStorageError::InvalidRecordKey {
                        field,
                    }) => publisher.session_failed(&fleet_sync_session::ClientError::Store(
                        fleet_sync_session::StoreError::Corrupt {
                            field: (*field).to_owned(),
                        },
                    )),
                    Some(error) => {
                        publisher.session_failed(&fleet_sync_session::ClientError::Store(
                            fleet_sync_session::StoreError::Unavailable {
                                message: error.to_string(),
                            },
                        ))
                    }
                    None => publisher.preparation_failed(error.to_string()),
                },
            },
        }
        let observed = reader.read().await;
        drop(publisher);
        let mut drain_error = None;
        while let Some(result) = tasks.join_next().await {
            if let Err(error) = result.map_err(io::Error::other).and_then(|result| result) {
                drain_error = Some(error);
            }
        }
        match (observed, drain_error) {
            (Ok(record), None) => FleetLinkObservation::Failed(Box::new(record)),
            (Err(error), _) => FleetLinkObservation::Unavailable {
                message: error.to_string(),
            },
            (_, Some(error)) => FleetLinkObservation::Unavailable {
                message: error.to_string(),
            },
        }
    }
}
