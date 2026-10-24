//! Explicit registry stand-in behind actual session requests; no custody policy.
use board_sync_protocol::{InstallationPeerLookup, InstallationPeerPage};
use fleet_sync_protocol::PeerId;
use fleet_sync_service::ServiceError;
use message_board::ServiceId;
use std::collections::HashMap;
use tokio::sync::{Mutex, oneshot};

struct LookupGate {
    entered: oneshot::Sender<()>,
    release: oneshot::Receiver<()>,
}
#[derive(Default)]
pub(in crate::collaboration_runtime::board_composition) struct DirectoryProbe {
    answers: Mutex<HashMap<ServiceId, InstallationPeerLookup>>,
    lookups: Mutex<Vec<ServiceId>>,
    gate: Mutex<Option<LookupGate>>,
    pages: Mutex<HashMap<Option<PeerId>, InstallationPeerPage>>,
}
impl DirectoryProbe {
    pub async fn answer_page(&self, after: Option<PeerId>, answer: InstallationPeerPage) {
        self.pages.lock().await.insert(after, answer);
    }
    pub async fn page(&self, after: Option<PeerId>) -> Result<InstallationPeerPage, ServiceError> {
        match self.pages.lock().await.get(&after) {
            Some(page) => Ok(page.clone()),
            None => {
                InstallationPeerPage::new(Vec::new(), None).map_err(|error| ServiceError::Rule {
                    message: error.to_string(),
                })
            }
        }
    }
    pub async fn answer(&self, service: ServiceId, answer: InstallationPeerLookup) {
        self.answers.lock().await.insert(service, answer);
    }
    pub async fn hold_next(&self) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (entered, observed) = oneshot::channel();
        let (release, released) = oneshot::channel();
        *self.gate.lock().await = Some(LookupGate {
            entered,
            release: released,
        });
        (observed, release)
    }
    pub async fn lookup(
        &self,
        service: &ServiceId,
    ) -> Result<InstallationPeerLookup, ServiceError> {
        self.lookups.lock().await.push(service.clone());
        if let Some(gate) = self.gate.lock().await.take() {
            let failed = || ServiceError::Rule {
                message: "directory fixture gate closed".to_owned(),
            };
            gate.entered.send(()).map_err(|_| failed())?;
            gate.release.await.map_err(|_| failed())?;
        }
        Ok(self
            .answers
            .lock()
            .await
            .get(service)
            .cloned()
            .unwrap_or(InstallationPeerLookup::Unknown))
    }
    pub async fn read_count(&self, service: &ServiceId) -> usize {
        self.lookups
            .lock()
            .await
            .iter()
            .filter(|seen| *seen == service)
            .count()
    }
}
