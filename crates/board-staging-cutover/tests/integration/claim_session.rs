//! A real SDK machine session obtains the bootstrap grant from TestCentral.
use board_fleet_sync::{
    BoardProjectSchema, BoardSessionInputs, MachineSessionStore, ReqwestSessionTransport,
};
use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol::{
    DatabaseId, HolderChangeAnswer, LeaseAnswer, LeaseRequest, LeaseState, LeaseStatus,
    OperationAnswer, OperationRequest, RequestId, SchemaSupport,
};
use fleet_sync_session::{ClientEvent, SessionClient};
use fleet_sync_tokio_runner::{SessionPump, SessionRequests};
use fleet_sync_writer::BootstrapGrant;
use runtime_primitives::{Backoff, Clock, SystemClock, UuidV7Ids};
use std::{path::Path, time::Duration};
use tokio::{
    sync::{oneshot, watch},
    task::JoinSet,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
const BOUND: Duration = Duration::from_secs(20);

pub struct ClaimSession {
    requests: SessionRequests<BoardConsumerProtocol>,
    peer: fleet_sync_protocol::PeerId,
    stop: watch::Sender<bool>,
    tasks: JoinSet<TestResult>,
}
impl ClaimSession {
    pub async fn start(directory: &Path, inputs: BoardSessionInputs) -> TestResult<Self> {
        let schema = BoardProjectSchema::new()?;
        let max_version = schema
            .supported_schemas()
            .iter()
            .map(|schema| schema.version)
            .max()
            .ok_or("schema support absent")?;
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
                    let _sent = ready.send(());
                }
            }
            Ok(())
        });
        let owner = Self {
            requests,
            peer: inputs.peer,
            stop,
            tasks,
        };
        tokio::time::timeout(BOUND, waiting).await??;
        Ok(owner)
    }

    pub async fn claim(&self, database: DatabaseId) -> TestResult<BootstrapGrant> {
        let request_started = SystemClock.now();
        let attempt = RequestId::new(uuid::Uuid::now_v7())?;
        tokio::time::timeout(BOUND, async {
            loop {
                let answer = self
                    .requests
                    .request(OperationRequest::Lease(LeaseRequest::Claim {
                        attempt,
                        database,
                    }))
                    .await?;
                match answer {
                    OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Granted {
                        epoch,
                        expires_at,
                        start,
                    })) => {
                        let remaining_ms = u64::try_from(
                            expires_at
                                .unix_millis()
                                .checked_sub(request_started.unix_millis())
                                .ok_or("grant duration overflow")?,
                        )?;
                        return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(BootstrapGrant {
                            lease: LeaseStatus {
                                database,
                                holder: self.peer,
                                epoch,
                                state: LeaseState::Held,
                                expires_at,
                                remaining_ms,
                            },
                            request_started,
                            start,
                        });
                    }
                    OperationAnswer::Lease(LeaseAnswer::Claim(HolderChangeAnswer::Pending {
                        ..
                    })) => tokio::task::yield_now().await,
                    _ => return Err("real registry did not grant the imported project".into()),
                }
            }
        })
        .await?
    }

    pub async fn shutdown(mut self) -> TestResult {
        self.stop.send_replace(true);
        while let Some(result) = self.tasks.join_next().await {
            result??;
        }
        Ok(())
    }
}
impl Drop for ClaimSession {
    fn drop(&mut self) {
        self.stop.send_replace(true);
        self.tasks.abort_all();
    }
}
