use crate::{LocalResult, ObjectActors};
use collaboration_central_core::*;
use collaboration_replication_protocol::CentralFrame;
use futures_util::future::BoxFuture;
use std::{collections::BTreeMap, sync::Arc};
use tokio::sync::{Mutex, mpsc};
#[derive(Clone)]
pub struct SessionRuntime {
    pub(crate) actors: ObjectActors,
    pub(crate) tasks: tokio_util::task::TaskTracker,
    #[cfg(feature = "conformance")]
    pub(crate) faults: Arc<TransportFaults>,
    pub(crate) stop: tokio_util::sync::CancellationToken,
    pub(crate) sockets: Arc<Mutex<BTreeMap<String, mpsc::UnboundedSender<CentralFrame>>>>,
    alarms: Arc<Mutex<BTreeMap<String, tokio::task::JoinHandle<()>>>>,
    pub(crate) platform_alarms: Arc<Mutex<crate::platform_alarms::PlatformAlarms>>,
    #[cfg(feature = "conformance")]
    clock: Arc<std::sync::atomic::AtomicI64>,
    #[cfg(feature = "conformance")]
    pub(crate) stop_after_fence: Arc<std::sync::atomic::AtomicBool>,
}
#[cfg(feature = "conformance")]
#[derive(Default)]
pub(crate) struct TransportFaults {
    pub hold_next_fence: std::sync::atomic::AtomicBool,
    pub fence_held: tokio::sync::Notify,
    pub resume_fence: tokio::sync::Notify,
    pub drop_directory: std::sync::atomic::AtomicBool,
    pub directory_updates: tokio::sync::Mutex<Vec<DirectoryUpdate>>,
    sessions:
        Mutex<BTreeMap<collaboration_replication_protocol::InstallationId, Arc<SessionFaults>>>,
}
#[cfg(feature = "conformance")]
#[derive(Default)]
pub(crate) struct SessionFaults {
    pub drop_next_answer: std::sync::atomic::AtomicBool,
    pub answer_dropped: tokio::sync::Notify,
    pub hold_next_dispatch: std::sync::atomic::AtomicBool,
    pub held_dispatch: tokio::sync::Mutex<Option<WireCall>>,
    pub dispatch_held: tokio::sync::Notify,
    pub resume_dispatch: tokio::sync::Notify,
    pub hold_forward_delivery: std::sync::atomic::AtomicBool,
    pub forward_delivery_held: tokio::sync::Notify,
    pub resume_forward_delivery: tokio::sync::Notify,
}
#[cfg(feature = "conformance")]
impl TransportFaults {
    pub(crate) async fn session(
        &self,
        installation: &collaboration_replication_protocol::InstallationId,
    ) -> Arc<SessionFaults> {
        self.sessions
            .lock()
            .await
            .entry(installation.clone())
            .or_default()
            .clone()
    }
}
impl SessionRuntime {
    pub(crate) async fn cancel_alarms(&self) {
        let alarms = std::mem::take(&mut *self.alarms.lock().await);
        for alarm in alarms.values() {
            alarm.abort();
        }
        for (_, alarm) in alarms {
            let _ = alarm.await;
        }
    }
    pub fn new(actors: ObjectActors) -> LocalResult<Self> {
        let platform_alarms = crate::platform_alarms::PlatformAlarms::load(&actors)?;
        Ok(Self {
            actors,
            tasks: tokio_util::task::TaskTracker::new(),
            #[cfg(feature = "conformance")]
            faults: Arc::new(TransportFaults::default()),
            stop: tokio_util::sync::CancellationToken::new(),
            sockets: Arc::new(Mutex::new(BTreeMap::new())),
            alarms: Arc::new(Mutex::new(BTreeMap::new())),
            platform_alarms: Arc::new(Mutex::new(platform_alarms)),
            #[cfg(feature = "conformance")]
            clock: Arc::new(std::sync::atomic::AtomicI64::new(i64::MIN)),
            #[cfg(feature = "conformance")]
            stop_after_fence: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
    }
    pub(crate) fn now(&self) -> RegistryTime {
        #[cfg(feature = "conformance")]
        if let Some(fixed) = chrono::DateTime::from_timestamp_millis(
            self.clock.load(std::sync::atomic::Ordering::SeqCst),
        ) {
            return RegistryTime(fixed);
        }
        RegistryTime(chrono::Utc::now())
    }
    #[cfg(feature = "conformance")]
    pub async fn set_time(&self, time: RegistryTime) -> LocalResult<()> {
        self.clock.store(
            time.0.timestamp_millis(),
            std::sync::atomic::Ordering::SeqCst,
        );
        self.fire_due_alarms().await
    }
    #[cfg(feature = "conformance")]
    pub fn interrupt_after_fence(&self) {
        self.stop_after_fence
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    pub async fn pending_alarms(&self) -> Vec<(ObjectAddress, RegistryTime)> {
        self.platform_alarms.lock().await.pending()
    }
    pub(crate) async fn restore_alarms(&self) -> LocalResult<()> {
        for (address, at) in self.pending_alarms().await {
            self.arm_alarm(address, at).await;
        }
        Ok(())
    }
    async fn fire_due_alarms(&self) -> LocalResult<()> {
        for (address, at) in self.pending_alarms().await {
            if at.0 <= self.now().0 {
                let plan = self.actors.alarm_plan(address.clone()).await?;
                for owned in plan {
                    if owned.at.0 <= self.now().0 {
                        self.drive(address.clone(), WireInput::Alarm { face: owned.face })
                            .await?;
                    }
                }
                self.refresh_alarm(address).await?;
            }
        }
        Ok(())
    }
    pub(crate) fn refresh_alarm<'a>(
        &'a self,
        address: ObjectAddress,
    ) -> BoxFuture<'a, LocalResult<()>> {
        Box::pin(async move {
            let plan = self.actors.alarm_plan(address.clone()).await?;
            if let Some(at) = plan.iter().map(|owned| owned.at).min_by_key(|at| at.0) {
                self.platform_alarms
                    .lock()
                    .await
                    .set(address.clone(), at)
                    .await?;
                self.arm_alarm(address, at).await;
            } else {
                self.platform_alarms.lock().await.clear(&address).await?;
            }
            Ok(())
        })
    }
    pub(crate) async fn arm_alarm(&self, address: ObjectAddress, at: RegistryTime) {
        let key = crate::object_actors::address_key(&address);
        let delay = std::time::Duration::from_millis(
            (at.0 - self.now().0)
                .num_milliseconds()
                .max(0)
                .unsigned_abs(),
        );
        let runtime = self.clone();
        let mut alarms = self.alarms.lock().await;
        if let Some(previous) = alarms.remove(&key) {
            previous.abort();
            let _ = previous.await;
        }
        let timer_key = key.clone();
        let task = self.tasks.spawn(async move {
            tokio::select! {_=runtime.stop.cancelled()=>return, _=tokio::time::sleep(delay)=>{}}
            runtime.alarms.lock().await.remove(&timer_key);
            if let Err(error) = runtime.fire_due_alarms().await {
                eprintln!("local alarm failed: {error}");
            }
        });
        alarms.insert(key, task);
    }
    pub fn invoke<'a>(
        &'a self,
        address: ObjectAddress,
        call: WireCall,
    ) -> BoxFuture<'a, LocalResult<WireResult>> {
        Box::pin(async move {
            match (address, call) {
                (ObjectAddress::Registry, WireCall::Registry(call)) => Ok(RegistryKind::result(
                    self.invoke_edge::<RegistryKind>((), call).await?,
                )),
                (ObjectAddress::Copy(project), WireCall::Copy(call)) => Ok(CopyKind::result(
                    self.invoke_edge::<CopyKind>(project, call).await?,
                )),
                (ObjectAddress::Fleet, WireCall::Fleet(call)) => Ok(FleetKind::result(
                    self.invoke_edge::<FleetKind>((), call).await?,
                )),
                (ObjectAddress::Connection(installation), WireCall::Session(call)) => Ok(
                    SessionKind::result(self.invoke_edge::<SessionKind>(installation, call).await?),
                ),
                (_, call) => Err(CentralError::CallEdgeRefused {
                    from: CallerKind::Edge,
                    to: call.kind(),
                }
                .into()),
            }
        })
    }
    pub(crate) fn invoke_edge<'a, To>(
        &'a self,
        to: To::Address,
        call: To::Call,
    ) -> BoxFuture<'a, LocalResult<To::Result>>
    where
        EdgeKind: MayCall<To>,
        To: WireKind + Send + 'a,
        To::Address: Send,
        To::Call: Send,
        To::Result: Send,
    {
        Box::pin(async move {
            let call_id = ObjectCallId::new(uuid::Uuid::now_v7().to_string())?;
            let result = self
                .invoke_identified(
                    CallerKind::Edge,
                    ObjectAddress::Registry,
                    To::address(&to),
                    call_id,
                    To::call(call),
                )
                .await?;
            Ok(To::decode_result(result)?)
        })
    }
    pub(crate) fn invoke_typed<'a, From, To>(
        &'a self,
        from: ObjectAddress,
        call: Dispatch<To>,
    ) -> BoxFuture<'a, LocalResult<To::Result>>
    where
        From: MayCall<To>,
        To: WireKind + Send + 'a,
        To::Address: Send,
        To::Call: Send,
        To::Result: Send,
    {
        Box::pin(async move {
            if !From::KIND.may_call(To::KIND) {
                return Err(CentralError::CallEdgeRefused {
                    from: From::KIND,
                    to: To::KIND,
                }
                .into());
            }
            let result = self
                .invoke_identified(
                    From::KIND,
                    from,
                    To::address(&call.to),
                    call.call_id,
                    To::call(call.call),
                )
                .await?;
            Ok(To::decode_result(result)?)
        })
    }
    pub(crate) fn invoke_identified<'a>(
        &'a self,
        caller_kind: CallerKind,
        from: ObjectAddress,
        address: ObjectAddress,
        call_id: ObjectCallId,
        call: WireCall,
    ) -> BoxFuture<'a, LocalResult<WireResult>> {
        Box::pin(async move {
            validate_call_edge(caller_kind, call.kind())?;
            #[cfg(feature = "conformance")]
            if address == ObjectAddress::Fleet
                && let WireCall::Fleet(FleetCall::DirectoryUpdate(update)) = &call
            {
                if self
                    .faults
                    .drop_directory
                    .swap(false, std::sync::atomic::Ordering::SeqCst)
                {
                    return Err("test-only directory delivery loss".into());
                }
                self.faults
                    .directory_updates
                    .lock()
                    .await
                    .push(update.clone());
            }
            let input = WireInput::Call {
                from,
                call_id: call_id.clone(),
                call: Box::new(call),
            };
            let effects = self.drive(address, input.clone()).await?;
            validate_call_returns(&input, &effects)?;
            effects
                .into_iter()
                .find_map(|effect| match effect {
                    WireEffect::Return {
                        call_id: returned,
                        result,
                    } if returned == call_id => Some(result),
                    _ => None,
                })
                .ok_or_else(|| "missing object Return".into())
        })
    }
    pub fn drive<'a>(
        &'a self,
        address: ObjectAddress,
        input: WireInput,
    ) -> BoxFuture<'a, LocalResult<Vec<WireEffect>>> {
        Box::pin(async move {
            let first = self.actors.step(address.clone(), input, self.now()).await?;
            self.carry(address, first).await
        })
    }
}
