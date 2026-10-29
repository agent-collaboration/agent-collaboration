//! Reuse the existing Host native watcher for exact fixture-file acknowledgements.
#[path = "../../src/native_file_events.rs"]
mod native_file_events;

pub(crate) struct NativeFileObservation(native_file_events::NativeFileEvents);

impl NativeFileObservation {
    pub(crate) fn subscribe(path: &std::path::Path) -> std::io::Result<Self> {
        native_file_events::NativeFileEvents::open(path).map(Self)
    }

    pub(crate) async fn wait_for_contents(
        &mut self,
        expected: &str,
        bound: std::time::Duration,
    ) -> std::io::Result<()> {
        tokio::time::timeout(bound, async {
            loop {
                match std::fs::read_to_string(self.0.path()) {
                    Ok(contents) if contents.contains(expected) => return Ok(()),
                    Ok(_) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
                self.0.wait_for_change().await?;
            }
        })
        .await
        .map_err(std::io::Error::other)?
    }
}
