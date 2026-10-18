use collaboration_central_core::*;
use serde::{Deserialize, Serialize};
use std::rc::Rc;
use worker::{Env, Storage, WebSocket};
#[derive(Clone)]
pub(crate) struct ObjectContext {
    pub address: ObjectAddress,
    pub face: CallerKind,
    pub registry_run: Option<RegistryRunId>,
    #[cfg(feature = "conformance")]
    pub signals: Rc<crate::conformance_control::SignalSlots>,
    pub env: Env,
    pub storage: Rc<Storage>,
    pub sockets: Vec<WebSocket>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SocketAttachment {
    pub installation: collaboration_replication_protocol::InstallationId,
    pub connection_id: ConnectionId,
    pub session_id: Option<collaboration_replication_protocol::SessionId>,
    pub credential: CredentialStatus,
}
pub(crate) fn ids() -> worker::Result<IdBatch> {
    Ok(IdBatch {
        registry_run: None,
        call_ids: (0..8)
            .map(|_| {
                ObjectCallId::new(uuid::Uuid::now_v7().to_string())
                    .map_err(|error| worker::Error::RustError(error.to_string()))
            })
            .collect::<worker::Result<_>>()?,
    })
}
impl ObjectContext {
    pub async fn now(&self) -> worker::Result<RegistryTime> {
        #[cfg(feature = "conformance")]
        if let Some(time) = self.storage.get::<RegistryTime>("_cf_test_clock").await? {
            return Ok(time);
        }
        Ok(RegistryTime(chrono::Utc::now()))
    }
    pub async fn step(&self, input: WireInput) -> worker::Result<ObjectEffects> {
        self.step_at(input, self.now().await?).await
    }
    pub async fn step_at(
        &self,
        input: WireInput,
        now: RegistryTime,
    ) -> worker::Result<ObjectEffects> {
        if input.face() != self.face {
            return Err(worker::Error::RustError(
                "input face differs from context".into(),
            ));
        }
        #[cfg(feature = "conformance")]
        if self.address == ObjectAddress::Fleet
            && let WireInput::Call { call, .. } = &input
            && let WireCall::Fleet(FleetCall::DirectoryUpdate(update)) = call.as_ref()
        {
            if crate::conformance_control::consume(self, "drop_directory").await? {
                return Err(worker::Error::RustError(
                    "test-only directory delivery loss".into(),
                ));
            }
            let mut received = self
                .storage
                .get::<Vec<DirectoryUpdate>>("_cf_test_directory_updates")
                .await?
                .unwrap_or_default();
            received.push(update.clone());
            self.storage
                .put("_cf_test_directory_updates", received)
                .await?;
        }
        #[cfg(feature = "conformance")]
        let full = self.storage.get::<u64>("_cf_test_full").await?;
        #[cfg(not(feature = "conformance"))]
        let full = None;
        #[cfg(feature = "conformance")]
        if full.is_some() {
            self.storage.delete("_cf_test_full").await?;
        }
        #[cfg(feature = "conformance-faults")]
        let panic_after_writes = self.storage.get::<u64>("_cf_test_panic").await?;
        #[cfg(feature = "conformance-faults")]
        if panic_after_writes.is_some() {
            self.storage.delete("_cf_test_panic").await?;
        }
        let mut batch = ids()?;
        batch.registry_run = self.registry_run.clone();
        crate::cloudflare_storage::storage_transaction(
            &self.storage,
            &self.address,
            input,
            now,
            batch,
            crate::cloudflare_storage::TransactionFaults {
                full_after_writes: full,
                #[cfg(feature = "conformance-faults")]
                panic_after_writes,
            },
        )
        .await
    }
}
