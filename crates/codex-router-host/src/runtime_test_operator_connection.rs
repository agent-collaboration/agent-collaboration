//! Test-only readiness through the existing owned native filesystem observer.
use crate::native_file_events::NativeFileEvents;
use std::{io, path::Path};
use tokio::{net::UnixStream, time::Instant};

pub async fn runtime_test_connect_operator(
    socket: &Path,
    deadline: Instant,
) -> io::Result<UnixStream> {
    let mut readiness = NativeFileEvents::open(socket)?;
    tokio::time::timeout_at(deadline, async {
        loop {
            match UnixStream::connect(socket).await {
                Ok(stream) => return Ok(stream),
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                    ) =>
                {
                    readiness.wait_for_change().await?
                }
                Err(error) => return Err(error),
            }
        }
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "operator request timed out"))?
}
