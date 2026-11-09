//! A failed preparation retains its final observation independently of a running pump.
use crate::FleetLinkReader;
use board_sync_protocol::FleetLinkRecord;
use fleet_sync_session::StoreError;

#[derive(Clone)]
pub enum FleetLinkObservation {
    Unconfigured,
    Live(FleetLinkReader),
    Failed(Box<FleetLinkRecord>),
    Unavailable { message: String },
}

impl FleetLinkObservation {
    pub async fn read(&self) -> Result<Option<FleetLinkRecord>, StoreError> {
        match self {
            Self::Unconfigured => Ok(None),
            Self::Live(reader) => reader.read().await.map(Some),
            Self::Failed(record) => Ok(Some(record.as_ref().clone())),
            Self::Unavailable { message } => Err(StoreError::Unavailable {
                message: message.clone(),
            }),
        }
    }
}
