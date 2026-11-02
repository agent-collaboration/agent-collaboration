//! Custody is acknowledged only after every entry has a durable local receipt.
use crate::{CentralAutomationClock, CentralAutomationError, CentralAutomationTransport};
use automation_storage::{AutomationStore, CentralFiringRequest};
use collaboration_protocol::{
    AckAutomationBatches, AutomationAnswer, AutomationBatch, AutomationCall, BatchAcknowledgement,
    ContentReference, FetchContents, FiringWork, FrozenAutomationBatch, MachineWorkProtocol,
    PullAutomationBatches, UuidIdentity, WorkEntry,
};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::sync::Mutex;

const CUSTODY_RETRY_MILLIS: i64 = 1000;

pub struct CentralAutomationReceiver {
    store: Arc<Mutex<AutomationStore>>,
    transport: Arc<dyn CentralAutomationTransport>,
    clock: Arc<dyn CentralAutomationClock>,
    service_id: UuidIdentity,
    receiving: Mutex<()>,
    content_cache:
        Mutex<std::collections::BTreeMap<String, collaboration_protocol::AutomationContent>>,
}
impl CentralAutomationReceiver {
    pub fn new(
        store: Arc<Mutex<AutomationStore>>,
        transport: Arc<dyn CentralAutomationTransport>,
        clock: Arc<dyn CentralAutomationClock>,
        service_id: UuidIdentity,
    ) -> Self {
        Self {
            store,
            transport,
            clock,
            service_id,
            receiving: Mutex::new(()),
            content_cache: Mutex::new(std::collections::BTreeMap::new()),
        }
    }
    /// Called at connect/reconnect, batch hints and notice-cursor recovery. Hints never supply work bytes.
    pub async fn receive_pending(&self) -> Result<(), CentralAutomationError> {
        let _receiving = self.receiving.lock().await;
        loop {
            match self.receive_once().await {
                Err(
                    error @ (CentralAutomationError::Unavailable
                    | CentralAutomationError::OutcomeUnknown),
                ) => {
                    tracing::warn!(%error, "central custody remains owed");
                    // Pull, content fetch and ACK reconcile the same frozen work.
                    // Reopening the traversal also covers an ACK applied with its reply lost.
                    self.clock
                        .wait_until(self.clock.now_ms().saturating_add(CUSTODY_RETRY_MILLIS))
                        .await;
                }
                result => return result,
            }
        }
    }

    async fn receive_once(&self) -> Result<(), CentralAutomationError> {
        let mut after = None;
        loop {
            let AutomationAnswer::Batches(page) = self
                .transport
                .call(AutomationCall::PullAutomationBatches(
                    PullAutomationBatches {
                        after: after.clone(),
                        max_bytes: u32::try_from(collaboration_protocol::MAX_CONTROL_FRAME_BYTES)
                            .map_err(|_| CentralAutomationError::InvalidWork)?,
                        work_protocols: vec![MachineWorkProtocol::MachineWorkV1],
                    },
                ))
                .await?
            else {
                return Err(CentralAutomationError::InvalidReply);
            };
            let mut acknowledgements = Vec::new();
            for frozen in &page.batches {
                let batch = self.decode_batch(frozen)?;
                self.admit_batch(&batch).await?;
                acknowledgements.push(BatchAcknowledgement {
                    batch_id: frozen.batch_id.clone(),
                    digest: frozen.digest.clone(),
                });
            }
            if !acknowledgements.is_empty() {
                let AutomationAnswer::Acknowledged = self
                    .transport
                    .call(AutomationCall::AckAutomationBatches(AckAutomationBatches {
                        batches: acknowledgements,
                    }))
                    .await?
                else {
                    return Err(CentralAutomationError::InvalidReply);
                };
            }
            match page.next {
                Some(next) if after.as_ref() != Some(&next) => after = Some(next),
                Some(_) => return Err(CentralAutomationError::InvalidReply),
                None => return Ok(()),
            }
        }
    }
    fn decode_batch(
        &self,
        frozen: &FrozenAutomationBatch,
    ) -> Result<AutomationBatch, CentralAutomationError> {
        if format!("{:x}", Sha256::digest(&frozen.frozen_bytes)) != frozen.digest.as_str() {
            return Err(CentralAutomationError::InvalidWork);
        }
        let batch: AutomationBatch = serde_json::from_slice(&frozen.frozen_bytes)
            .map_err(|_| CentralAutomationError::InvalidWork)?;
        if batch.batch_id != frozen.batch_id || batch.service_id != self.service_id {
            return Err(CentralAutomationError::InvalidWork);
        }
        Ok(batch)
    }
    async fn admit_batch(&self, batch: &AutomationBatch) -> Result<(), CentralAutomationError> {
        let mut references = Vec::<ContentReference>::new();
        for entry in batch.entries.entries() {
            match entry {
                WorkEntry::Firing(firing) => {
                    let service = match &firing.work {
                        FiringWork::Wake { target, .. } => &target.endpoint.service_id,
                        FiringWork::Schedule { definition } => match &definition.destination {
                            collaboration_protocol::ExecutionDestination::OwnedThread {
                                target,
                                ..
                            } => &target.endpoint.service_id,
                            collaboration_protocol::ExecutionDestination::FreshEachRun {
                                endpoint,
                                ..
                            } => &endpoint.service_id,
                            _ => return Err(CentralAutomationError::InvalidWork),
                        },
                    };
                    if service != &self.service_id {
                        return Err(CentralAutomationError::InvalidWork);
                    }
                    if !references.contains(&firing.content) {
                        references.push(firing.content.clone());
                    }
                }
                WorkEntry::SessionCreate(_) => {
                    return Err(CentralAutomationError::SessionCreateUnavailable);
                }
            }
        }
        let missing = {
            let cache = self.content_cache.lock().await;
            references
                .iter()
                .filter(|reference| {
                    cache
                        .get(reference.content_id.as_str())
                        .is_none_or(|content| content.reference != **reference)
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        if !missing.is_empty() {
            let AutomationAnswer::Contents(contents) = self
                .transport
                .call(AutomationCall::FetchContents(FetchContents {
                    contents: missing.clone(),
                }))
                .await?
            else {
                return Err(CentralAutomationError::InvalidReply);
            };
            let mut cache = self.content_cache.lock().await;
            for content in contents {
                if !missing.contains(&content.reference)
                    || format!("{:x}", Sha256::digest(content.text.as_str().as_bytes()))
                        != content.reference.digest.as_str()
                {
                    return Err(CentralAutomationError::InvalidWork);
                }
                cache.insert(content.reference.content_id.as_str().to_owned(), content);
            }
        }
        let contents = {
            let cache = self.content_cache.lock().await;
            references
                .iter()
                .map(|reference| {
                    cache
                        .get(reference.content_id.as_str())
                        .filter(|content| content.reference == *reference)
                        .cloned()
                        .ok_or(CentralAutomationError::InvalidReply)
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        #[cfg(debug_assertions)]
        self.hold(
            automation_storage::AutomationFaultPoint::BatchFetchedBeforeAdmission,
            automation_storage::AutomationFaultEvidence {
                batch_id: Some(batch.batch_id.clone()),
                ..Default::default()
            },
        )
        .await;
        for (_entry_index, entry) in batch.entries.entries().iter().enumerate() {
            let WorkEntry::Firing(firing) = entry else {
                return Err(CentralAutomationError::SessionCreateUnavailable);
            };
            let content = contents
                .iter()
                .find(|content| content.reference == firing.content)
                .ok_or(CentralAutomationError::InvalidReply)?;
            if format!("{:x}", Sha256::digest(content.text.as_str().as_bytes()))
                != content.reference.digest.as_str()
            {
                return Err(CentralAutomationError::InvalidWork);
            }
            self.store
                .lock()
                .await
                .admit_central_firing(&CentralFiringRequest {
                    batch_id: batch.batch_id.clone(),
                    firing: firing.clone(),
                    content: content.clone(),
                    now_ms: self.clock.now_ms(),
                })
                .await?;
            #[cfg(debug_assertions)]
            self.hold(
                automation_storage::AutomationFaultPoint::EntryAdmissionCommitted,
                automation_storage::AutomationFaultEvidence {
                    batch_id: Some(batch.batch_id.clone()),
                    firing_id: Some(firing.firing_id.clone()),
                    completed_entry_ordinal: Some(
                        u32::try_from(_entry_index + 1)
                            .map_err(|_| CentralAutomationError::InvalidWork)?,
                    ),
                    ..Default::default()
                },
            )
            .await;
        }
        #[cfg(debug_assertions)]
        self.hold(
            automation_storage::AutomationFaultPoint::BatchAdmissionCommittedBeforeAck,
            automation_storage::AutomationFaultEvidence {
                batch_id: Some(batch.batch_id.clone()),
                ..Default::default()
            },
        )
        .await;
        Ok(())
    }
    #[cfg(debug_assertions)]
    async fn hold(
        &self,
        point: automation_storage::AutomationFaultPoint,
        evidence: automation_storage::AutomationFaultEvidence,
    ) {
        let faults = self.store.lock().await.automation_faults();
        if let Some(faults) = faults {
            faults.hold(point, evidence).await;
        }
    }
}
