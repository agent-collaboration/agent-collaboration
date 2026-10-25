//! The target object's physical file sample belongs to its Health request.
use super::{CopyRuntime, NativeObjects, ObjectSlot, no_route};
#[cfg(feature = "conformance")]
use collaboration_central_core::ObjectAddress;
use durable_steps::CallFailure;

/// Actual metadata outcomes captured for the most recent Health call on a Copy.
/// This observation exists only in conformance builds and never changes files.
#[cfg(feature = "conformance")]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HealthFileSample {
    Present {
        target: std::path::PathBuf,
        length: u64,
    },
    Missing {
        target: std::path::PathBuf,
    },
}

impl NativeObjects {
    pub(super) async fn read_health(
        &self,
        slot: &ObjectSlot,
        runtime: &CopyRuntime,
        body: Vec<u8>,
        #[cfg(feature = "conformance")] address: ObjectAddress,
    ) -> Result<Vec<u8>, CallFailure> {
        let host = slot.host.get().ok_or_else(no_route)?;
        let path = host.object.path();
        let mut wal = path.as_os_str().to_os_string();
        wal.push("-wal");
        let mut bytes = 0_u64;
        #[cfg(feature = "conformance")]
        let mut sample = Vec::with_capacity(2);
        for file in [path.to_owned(), std::path::PathBuf::from(wal)] {
            #[cfg(feature = "conformance")]
            let target = file.clone();
            match tokio::fs::metadata(file).await {
                Ok(metadata) => {
                    #[cfg(feature = "conformance")]
                    sample.push(HealthFileSample::Present {
                        target,
                        length: metadata.len(),
                    });
                    bytes = bytes
                        .checked_add(metadata.len())
                        .ok_or(CallFailure::AnswerLost)?;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    #[cfg(feature = "conformance")]
                    sample.push(HealthFileSample::Missing { target });
                }
                Err(_) => return Err(CallFailure::AnswerLost),
            }
        }
        #[cfg(feature = "conformance")]
        if let ObjectAddress::Copy(database) = address {
            self.health_samples.lock().await.insert(database, sample);
        }
        runtime.on_health(body, bytes).await
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn health_sample(
        &self,
        database: fleet_sync_protocol::DatabaseId,
    ) -> Option<Vec<HealthFileSample>> {
        self.health_samples.lock().await.get(&database).cloned()
    }
}
