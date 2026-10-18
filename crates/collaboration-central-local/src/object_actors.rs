use crate::{LocalResult, turso_storage::TursoStorage};
use collaboration_central_core::*;
use collaboration_central_core::{IdBatch, ObjectAddress, ObjectCallId, RegistryTime, WireInput};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::{Mutex, mpsc, oneshot};
#[derive(Clone)]
pub struct ObjectActors {
    root: Arc<PathBuf>,
    actors: Arc<Mutex<BTreeMap<String, ActorHandle>>>,
}
struct ActorHandle {
    sender: mpsc::Sender<ActorMessage>,
    task: tokio::task::JoinHandle<()>,
}
enum ActorMessage {
    AlarmPlan {
        address: ObjectAddress,
        reply: oneshot::Sender<LocalResult<Vec<FaceAlarm>>>,
    },
    #[cfg(feature = "conformance")]
    Corrupt {
        mutation: collaboration_central_conformance::StorageCorruption,
        reply: oneshot::Sender<LocalResult<()>>,
    },
    #[cfg(feature = "conformance")]
    Full {
        after_writes: u64,
        reply: oneshot::Sender<()>,
    },
    Step {
        input: Box<WireInput>,
        now: RegistryTime,
        ids: IdBatch,
        reply: oneshot::Sender<LocalResult<ObjectEffects>>,
    },
    Stop {
        reply: oneshot::Sender<()>,
    },
}
pub(crate) fn address_key(address: &ObjectAddress) -> String {
    match address {
        ObjectAddress::Registry => "registry".into(),
        ObjectAddress::Fleet => "fleet".into(),
        ObjectAddress::Copy(project) => format!("copy-{}", project.as_str()),
        ObjectAddress::Connection(installation) => format!("connection-{}", installation.as_str()),
    }
}
impl ObjectActors {
    pub(crate) fn root(&self) -> &Path {
        self.root.as_path()
    }
    pub fn new(root: &Path) -> LocalResult<Self> {
        std::fs::create_dir_all(root)?;
        Ok(Self {
            root: Arc::new(root.into()),
            actors: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }
    async fn actor(&self, address: &ObjectAddress) -> LocalResult<mpsc::Sender<ActorMessage>> {
        let key = address_key(address);
        let mut actors = self.actors.lock().await;
        if let Some(actor) = actors.get(&key) {
            return Ok(actor.sender.clone());
        }
        let (tx, mut rx) = mpsc::channel::<ActorMessage>(64);
        let (started, start_rx) = oneshot::channel();
        let path = self.root.join(format!("{key}.sqlite"));
        let registry_run = if matches!(address, ObjectAddress::Registry) {
            Some(RegistryRunId::new(uuid::Uuid::now_v7().to_string())?)
        } else {
            None
        };
        let task = tokio::task::spawn_blocking(move || {
            let mut storage = match TursoStorage::open(&path) {
                Ok(storage) => {
                    let _ = started.send(Ok(()));
                    storage
                }
                Err(error) => {
                    let _ = started.send(Err(error));
                    return;
                }
            };
            while let Some(message) = rx.blocking_recv() {
                match message {
                    ActorMessage::AlarmPlan { address, reply } => {
                        let _ = reply.send(storage.alarm_plan(&address));
                    }
                    #[cfg(feature = "conformance")]
                    ActorMessage::Corrupt { mutation, reply } => {
                        let _ = reply.send(storage.corrupt(mutation));
                    }
                    #[cfg(feature = "conformance")]
                    ActorMessage::Full {
                        after_writes,
                        reply,
                    } => {
                        storage.inject_full(after_writes);
                        let _ = reply.send(());
                    }
                    ActorMessage::Step {
                        input,
                        now,
                        mut ids,
                        reply,
                    } => {
                        ids.registry_run = registry_run.clone();
                        let _ = reply.send(storage.transact(*input, now, ids));
                    }
                    ActorMessage::Stop { reply } => {
                        drop(storage);
                        let _ = reply.send(());
                        return;
                    }
                }
            }
        });
        start_rx.await??;
        actors.insert(
            key,
            ActorHandle {
                sender: tx.clone(),
                task,
            },
        );
        Ok(tx)
    }
    pub(crate) async fn alarm_plan(&self, address: ObjectAddress) -> LocalResult<Vec<FaceAlarm>> {
        let actor = self.actor(&address).await?;
        let (reply, response) = oneshot::channel();
        actor
            .send(ActorMessage::AlarmPlan { address, reply })
            .await
            .map_err(|_| "object actor stopped")?;
        response.await?
    }
    pub async fn step(
        &self,
        address: ObjectAddress,
        input: WireInput,
        now: RegistryTime,
    ) -> LocalResult<ObjectEffects> {
        let actor = self.actor(&address).await?;
        let (reply, response) = oneshot::channel();
        let mut call_ids = Vec::with_capacity(8);
        for _ in 0..8 {
            call_ids.push(ObjectCallId::new(uuid::Uuid::now_v7().to_string())?);
        }
        actor
            .send(ActorMessage::Step {
                input: Box::new(input),
                now,
                ids: IdBatch {
                    call_ids,
                    registry_run: None,
                },
                reply,
            })
            .await
            .map_err(|_| "object actor stopped")?;
        response.await?
    }
    #[cfg(feature = "conformance")]
    pub async fn inject_full(&self, address: ObjectAddress, after_writes: u64) -> LocalResult<()> {
        let actor = self.actor(&address).await?;
        let (reply, response) = oneshot::channel();
        actor
            .send(ActorMessage::Full {
                after_writes,
                reply,
            })
            .await
            .map_err(|_| "actor stopped")?;
        response.await?;
        Ok(())
    }
    #[cfg(feature = "conformance")]
    pub(crate) async fn corrupt(
        &self,
        address: ObjectAddress,
        mutation: collaboration_central_conformance::StorageCorruption,
    ) -> LocalResult<()> {
        let actor = self.actor(&address).await?;
        let (reply, response) = oneshot::channel();
        actor
            .send(ActorMessage::Corrupt { mutation, reply })
            .await
            .map_err(|_| "actor stopped")?;
        response.await?
    }
    pub async fn evict(&self, address: ObjectAddress) -> LocalResult<()> {
        let mut actors = self.actors.lock().await;
        if let Some(actor) = actors.remove(&address_key(&address)) {
            let (reply, response) = oneshot::channel();
            actor
                .sender
                .send(ActorMessage::Stop { reply })
                .await
                .map_err(|_| "object actor stopped")?;
            response.await?;
            actor.task.await?;
        }
        Ok(())
    }
    pub async fn shutdown(&self) -> LocalResult<()> {
        let mut actors = self.actors.lock().await;
        let retiring = std::mem::take(&mut *actors);
        for (_, actor) in retiring {
            let (reply, response) = oneshot::channel();
            actor
                .sender
                .send(ActorMessage::Stop { reply })
                .await
                .map_err(|_| "object actor stopped")?;
            response.await?;
            actor.task.await?;
        }
        Ok(())
    }
}
