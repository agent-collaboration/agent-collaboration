//! The fixture keeps one real pump alive beside the caller-owned FleetClient.
use crate::{BOUND, FixtureResult, TestCentral};
use board_fleet_sync::{BoardProjectSchema, MachineSessionStore, ReqwestSessionTransport};
use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol::SchemaSupport;
use fleet_sync_session::{ClientEvent, SessionClient};
use fleet_sync_tokio_runner::{SessionPump, SessionRequests};
use runtime_primitives::{Backoff, UuidV7Ids};
use std::{path::Path, time::Duration};
use tokio::{
    sync::{oneshot, watch},
    task::JoinSet,
};

enum CentralOwner {
    Running(Box<TestCentral>),
    Stopped,
}
pub struct FleetTestRuntime {
    pub(super) requests: SessionRequests<BoardConsumerProtocol>,
    stop: watch::Sender<bool>,
    tasks: JoinSet<FixtureResult>,
    central: CentralOwner,
}
impl FleetTestRuntime {
    /// Count opaque bodies on the real Access bridge while one operation runs.
    #[cfg(feature = "conformance")]
    pub fn observe_payload_input_bodies(&self) -> FixtureResult<crate::PayloadInputObservation> {
        match &self.central {
            CentralOwner::Running(central) => central.observe_payload_input_bodies(),
            CentralOwner::Stopped => Err("fixture central already stopped".into()),
        }
    }

    /// Prune the real owner's feed in one fixture transaction, retaining receipts.
    #[cfg(feature = "conformance")]
    pub async fn prune_fleet_feed(&self) -> FixtureResult<(u64, i64)> {
        match &self.central {
            CentralOwner::Running(central) => central.prune_fleet_feed().await,
            CentralOwner::Stopped => Err("fixture central already stopped".into()),
        }
    }

    pub async fn shutdown(mut self) -> FixtureResult {
        self.disconnect().await
    }

    /// Stop the real transport and central, preserving the caller's cache and outbox.
    pub async fn disconnect(&mut self) -> FixtureResult {
        self.stop.send_replace(true);
        while let Some(result) = self.tasks.join_next().await {
            result??;
        }
        if let CentralOwner::Running(central) =
            std::mem::replace(&mut self.central, CentralOwner::Stopped)
        {
            central.shutdown().await?;
        }
        Ok(())
    }
}
impl Drop for FleetTestRuntime {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        self.tasks.abort_all();
    }
}
pub(crate) async fn connect(
    directory: &Path,
    central: TestCentral,
) -> FixtureResult<(SessionRequests<BoardConsumerProtocol>, FleetTestRuntime)> {
    let inputs = central.session_inputs()?;
    let max_version = BoardProjectSchema::new()?
        .supported_schemas()
        .iter()
        .map(|schema| schema.version)
        .max()
        .ok_or("board schema support is empty")?;
    let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
        ReqwestSessionTransport::new(inputs.access, BOUND, BOUND)?,
        MachineSessionStore::open(directory).await?,
        &UuidV7Ids,
        SchemaSupport { max_version },
        Backoff {
            initial: Duration::from_millis(10),
            max: Duration::from_millis(100),
            factor: 2,
        },
    )
    .await?;
    let (requests, mut events, pump) =
        SessionPump::new(client, inputs.endpoint, inputs.credential, BOUND);
    let (stop, stopped) = watch::channel(false);
    let (ready, waiting) = oneshot::channel();
    let mut tasks = JoinSet::new();
    tasks.spawn(async move {
        pump.run(stopped).await?;
        Ok(())
    });
    tasks.spawn(async move {
        let mut ready = Some(ready);
        while let Some(event) = events.recv().await {
            if matches!(event, ClientEvent::Welcome { .. })
                && let Some(ready) = ready.take()
            {
                let _ = ready.send(());
            }
        }
        Ok(())
    });
    let runtime = FleetTestRuntime {
        requests: requests.clone(),
        stop,
        tasks,
        central: CentralOwner::Running(Box::new(central)),
    };
    tokio::time::timeout(BOUND, waiting).await??;
    Ok((requests, runtime))
}
