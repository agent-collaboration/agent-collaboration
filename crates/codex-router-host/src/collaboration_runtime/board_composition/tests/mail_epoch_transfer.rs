//! Establish real registry grants and schema support for the epoch replay proof.
use super::*;
use board_fleet_sync::{MachineSessionStore, ReqwestSessionTransport};
use fleet_sync_protocol::{
    ActorRef, Epoch, HolderChangeAnswer, Origin, Registration, RegistrationAnswer, RequestId,
    SchemaSupport, TransferId,
};
use fleet_sync_session::{PeerCredential, SessionClient};
use fleet_sync_tokio_runner::SessionPump;
use runtime_primitives::{Backoff, UuidV7Ids};

type ProofResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub(crate) struct EpochTransfer {
    peer: PeerId,
    stop: watch::Sender<bool>,
    task: JoinHandle<io::Result<()>>,
}

impl EpochTransfer {
    pub(crate) async fn close(self) -> ProofResult<()> {
        self.stop.send_replace(true);
        timeout(BOUND, self.task).await???;
        Ok(())
    }
}

impl TestFleet {
    pub async fn establish_epoch_two(
        &self,
        root: &Path,
        project: &message_board::ProjectId,
    ) -> ProofResult<EpochTransfer> {
        let inputs = self.inputs()?;
        let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
            ReqwestSessionTransport::new(inputs.access, BOUND, BOUND)?,
            MachineSessionStore::open(root).await?,
            &UuidV7Ids,
            SchemaSupport { max_version: 1 },
            Backoff {
                initial: Duration::from_millis(100),
                max: BOUND,
                factor: 2,
            },
        )
        .await?;
        let (_requests, mut events, pump) = SessionPump::new(
            client,
            inputs.endpoint,
            PeerCredential::new("Bearer board-epoch-successor-fixture")?,
            BOUND,
        );
        let (stop, stopped) = watch::channel(false);
        let task = tokio::spawn(async move { pump.run(stopped).await.map_err(io::Error::other) });
        timeout(BOUND, async {
            while let Some(event) = events.recv().await {
                if matches!(event, fleet_sync_session::ClientEvent::Welcome { .. }) {
                    return Ok::<_, io::Error>(());
                }
            }
            Err(io::Error::other("successor schema-support Welcome missing"))
        })
        .await??;
        let transfer = EpochTransfer {
            peer: self.auth.successor_peer,
            stop,
            task,
        };
        let database = DatabaseId::new(project.as_str().parse()?)?;
        let registered = self
            .handle
            .register(Registration {
                attempt: RequestId::new(durable_steps::Uuid::now_v7())?,
                database,
                writer: transfer.peer,
                origin: Origin::Empty,
            })
            .await?;
        if !matches!(registered, RegistrationAnswer::Registered { lease, .. } if lease.epoch.value() == 1)
        {
            return Err(io::Error::other("initial registry grant was not epoch 1").into());
        }
        let epoch = self.take_over(database, self.peer).await?;
        if epoch.value() != 2 {
            return Err(io::Error::other("mail origin grant was not epoch 2").into());
        }
        Ok(transfer)
    }

    pub async fn move_to_epoch_three(
        &self,
        project: &message_board::ProjectId,
        transfer: &EpochTransfer,
    ) -> ProofResult<()> {
        let database = DatabaseId::new(project.as_str().parse()?)?;
        let epoch = self.take_over(database, transfer.peer).await?;
        if epoch.value() != 3 {
            return Err(io::Error::other("successor registry grant was not epoch 3").into());
        }
        Ok(())
    }

    async fn take_over(&self, database: DatabaseId, to: PeerId) -> ProofResult<Epoch> {
        let key = durable_steps::RequestKey::new(durable_steps::Uuid::now_v7().to_string())?;
        let preview = self.handle.preview_takeover(key, database).await?;
        let attempt = TransferId::new(durable_steps::Uuid::now_v7())?;
        let request = HolderChangeRequest::Takeover {
            attempt,
            database,
            preview,
            to,
            reason: fleet_sync_protocol::HolderChangeReason::new(
                "native mail historical-epoch proof",
            )?,
        };
        timeout(BOUND, async {
            loop {
                match self
                    .handle
                    .holder_change(request.clone(), ActorRef::new("mail-epoch-fixture")?)
                    .await?
                {
                    HolderChangeAnswer::Granted { epoch, .. } => {
                        if self
                            .handle
                            .holder_change_view(attempt)
                            .await?
                            .is_some_and(|view| !view.settlement_owed)
                        {
                            return Ok(epoch);
                        }
                        tokio::task::yield_now().await;
                    }
                    HolderChangeAnswer::Pending { .. } => tokio::task::yield_now().await,
                    answer => {
                        return Err(io::Error::other(format!(
                            "native fixture holder change refused: {answer:?}"
                        ))
                        .into());
                    }
                }
            }
        })
        .await?
    }
}
