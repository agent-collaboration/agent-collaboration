//! Reconnects read the enrollment journal before opening the machine socket.
use board_fleet_sync::ReqwestSessionTransport;
use fleet_sync_session::{ConnectFailure, Endpoint, PeerCredential, SessionTransport};
use std::path::{Path, PathBuf};

pub(crate) struct EnrollmentSessionTransport {
    inner: ReqwestSessionTransport,
    journal_directory: PathBuf,
}

impl EnrollmentSessionTransport {
    pub(crate) fn new(inner: ReqwestSessionTransport, journal_directory: &Path) -> Self {
        Self {
            inner,
            journal_directory: journal_directory.to_owned(),
        }
    }
}

impl SessionTransport for EnrollmentSessionTransport {
    type Connection = <ReqwestSessionTransport as SessionTransport>::Connection;

    async fn connect(
        &self,
        endpoint: &Endpoint,
        credential: &PeerCredential,
    ) -> Result<Self::Connection, ConnectFailure> {
        let directory = self.journal_directory.clone();
        let pending = tokio::task::spawn_blocking(move || {
            crate::enrollment_reset::reset_pending_in_directory(&directory)
        })
        .await
        .map_err(|_| ConnectFailure::Unavailable)?
        .map_err(|_| ConnectFailure::Unavailable)?;
        if pending {
            tracing::warn!(
                reason = crate::ENROLLMENT_RESTART_INSTRUCTION,
                "central attach blocked by enrollment change"
            );
            return Err(ConnectFailure::Unavailable);
        }
        self.inner.connect(endpoint, credential).await
    }
}

#[cfg(test)]
#[path = "enrollment_session_transport_test.rs"]
mod tests;
