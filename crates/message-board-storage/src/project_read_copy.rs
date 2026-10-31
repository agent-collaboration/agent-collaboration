//! Board queries over a verified follower generation, without migration history.
use crate::{ProjectReadStore, ProjectStorageError, StoreSchemaState};
use fleet_sync_writer::ReadCopy;
use message_board::{ProjectId, SchemaFingerprint, SchemaRefusal};

pub struct ProjectReadCopy {
    project: ProjectId,
    copy: ReadCopy,
    schema: StoreSchemaState,
}

impl ProjectReadCopy {
    /// Validate the installed identity and actual layout without opening a writer.
    pub async fn from_verified(
        project: ProjectId,
        copy: ReadCopy,
    ) -> Result<Self, ProjectStorageError> {
        if copy.installed().database.as_uuid().to_string() != project.as_str() {
            return Err(ProjectStorageError::InvalidSchema);
        }
        let installed = &copy.installed().schema;
        if installed.version > crate::PROJECT_SCHEMA_VERSION {
            return Err(ProjectStorageError::SchemaNotReady(SchemaRefusal::TooNew {
                store: installed.version,
                supported: crate::PROJECT_SCHEMA_VERSION,
            }));
        }
        let expected = crate::expected_project_fingerprints()?
            .remove(&installed.version)
            .ok_or(ProjectStorageError::InvalidSchema)?;
        let declared = SchemaFingerprint::try_from(installed.fingerprint.clone())
            .map_err(|_| ProjectStorageError::InvalidSchema)?;
        let mut read = copy.reader().begin_read().await?;
        let actual = crate::schema_fingerprint::schema_fingerprint(&mut read).await?;
        read.close().await?;
        if declared != expected || actual != expected {
            return Err(ProjectStorageError::SchemaNotReady(
                SchemaRefusal::Mismatch {
                    version: installed.version,
                    expected,
                    actual,
                },
            ));
        }
        Ok(Self {
            project,
            schema: StoreSchemaState::Ready {
                version: installed.version,
                fingerprint: actual,
            },
            copy,
        })
    }

    pub fn reader(&self) -> ProjectReadStore<'_> {
        ProjectReadStore::new(&self.project, self.copy.reader(), &self.schema)
    }

    pub fn installed(&self) -> &fleet_sync_receiver::InstalledSnapshot {
        self.copy.installed()
    }

    pub async fn close(self) -> Result<(), fleet_sync_writer::ReadCopyError> {
        self.copy.close().await
    }
}
