use crate::{LocalResult, ObjectActors};
use collaboration_central_core::{ObjectAddress, RegistryTime};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};
#[derive(Clone, Debug, Serialize, Deserialize)]
struct AlarmRecord {
    address: ObjectAddress,
    at: RegistryTime,
}
pub(crate) struct PlatformAlarms {
    path: PathBuf,
    records: BTreeMap<String, AlarmRecord>,
}
impl PlatformAlarms {
    pub(crate) fn load(actors: &ObjectActors) -> LocalResult<Self> {
        let path = actors.root().join("platform-alarms.json");
        let records = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Vec<AlarmRecord>>(&bytes)?
                .into_iter()
                .map(|record| (crate::object_actors::address_key(&record.address), record))
                .collect(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.into()),
        };
        Ok(Self { path, records })
    }
    pub(crate) fn pending(&self) -> Vec<(ObjectAddress, RegistryTime)> {
        self.records
            .values()
            .map(|record| (record.address.clone(), record.at))
            .collect()
    }
    pub(crate) async fn set(
        &mut self,
        address: ObjectAddress,
        at: RegistryTime,
    ) -> LocalResult<()> {
        self.records.insert(
            crate::object_actors::address_key(&address),
            AlarmRecord { address, at },
        );
        self.persist().await
    }
    pub(crate) async fn clear(&mut self, address: &ObjectAddress) -> LocalResult<()> {
        self.records
            .remove(&crate::object_actors::address_key(address));
        self.persist().await
    }
    async fn persist(&self) -> LocalResult<()> {
        let bytes = serde_json::to_vec(&self.records.values().collect::<Vec<_>>())?;
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || -> LocalResult<()> {
            // Alarm state belongs to the platform, outside every object's SQL
            // and snapshot. Atomic replacement keeps restart recovery readable.
            let temporary = path.with_extension("json.pending");
            let mut file = std::fs::File::create(&temporary)?;
            use std::io::Write;
            file.write_all(&bytes)?;
            file.sync_all()?;
            std::fs::rename(temporary, path)?;
            Ok(())
        })
        .await?
    }
}
