use super::SubscriptionWindowStore;
use message_board::BoardError;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Board failure disables subscriptions while direct-message delivery stays available.
#[derive(Clone)]
pub enum BoardAvailability {
    Available(Arc<Mutex<SubscriptionWindowStore>>),
    Unavailable,
}

impl BoardAvailability {
    pub(super) fn require_store(&self) -> Result<&Arc<Mutex<SubscriptionWindowStore>>, BoardError> {
        match self {
            Self::Available(store) => Ok(store),
            Self::Unavailable => Err(BoardError::board_unavailable()),
        }
    }
}
