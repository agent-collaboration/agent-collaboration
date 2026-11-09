//! One object's slot: its Io and host, its alarm loop, and the kernel that
//! handles its events.
//!
//! Every event and call enters the kernel through `KernelCell`, which counts
//! what is running. In conformance builds that count lets a rebuild switch
//! replace the kernel and its handler caches only while nothing runs in it
//! (S4). The store, the sockets and the timer belong to the slot and survive.

use super::{CopyRuntime, FleetRuntime, NativeHost, TokioObjectIo};
use collaboration_central_core::{
    EventReply, KernelFace, ObjectAddress, ObjectEvent, ObjectKernel, ObjectRuntime,
};
use durable_steps::StoreError;
use fleet_project_lease::RegistryConfig;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;
use tokio::sync::OnceCell;

pub(super) enum RuntimeFace {
    Fleet(ObjectKernel<FleetRuntime>),
    Copy(ObjectKernel<CopyRuntime>),
}

impl RuntimeFace {
    pub(super) async fn initialize(&self) -> Result<(), StoreError> {
        match self {
            Self::Fleet(kernel) => kernel.face().initialize().await.map(|_| ()),
            // Copy's core call/alarm prelude runs after Health classification.
            Self::Copy(_) => Ok(()),
        }
    }

    // Keep each event's SQL and dispatched effects in one observable scope.
    #[tracing::instrument(level = "trace", name = "central_event", skip_all, fields(object_kind = match self {
        Self::Fleet(_) => "fleet",
        Self::Copy(_) => "copy",
    }))]
    async fn on_event(
        &self,
        event: ObjectEvent,
        #[cfg(feature = "conformance")]
        context: collaboration_central_core::event_trace::EventContext,
    ) -> EventReply {
        #[cfg(feature = "conformance")]
        match self {
            Self::Fleet(kernel) => kernel.on_event_with(event, context).await,
            Self::Copy(kernel) => kernel.on_event_with(event, context).await,
        }
        #[cfg(not(feature = "conformance"))]
        match self {
            Self::Fleet(kernel) => kernel.on_event(event).await,
            Self::Copy(kernel) => kernel.on_event(event).await,
        }
    }
}

/// Everything a kernel is built from, kept so a rebuild builds the same one.
pub(super) struct KernelRecipe {
    pub(super) address: ObjectAddress,
    pub(super) host: Arc<NativeHost>,
    pub(super) config: RegistryConfig,
    pub(super) delivery: crate::DeliveryConfig,
    pub(super) holder_answer_timeout: Duration,
    pub(super) copy_ack_wait: Duration,
}

impl KernelRecipe {
    pub(super) fn build(
        &self,
        #[cfg(feature = "conformance")] io: &TokioObjectIo,
        #[cfg(feature = "conformance")]
        cause: collaboration_central_core::event_trace::ConstructionCause,
    ) -> Result<RuntimeFace, StoreError> {
        #[cfg(feature = "conformance")]
        let observer = io.seams.observer(cause);
        Ok(match &self.address {
            ObjectAddress::Copy(project) => RuntimeFace::Copy(kernel(
                ObjectRuntime::copy(
                    self.host.clone(),
                    self.config.authoritative_receiver,
                    *project,
                    self.copy_ack_wait,
                ),
                #[cfg(feature = "conformance")]
                observer,
            )),
            ObjectAddress::Fleet => RuntimeFace::Fleet(kernel(
                ObjectRuntime::fleet(
                    self.host.clone(),
                    self.config,
                    self.delivery,
                    self.holder_answer_timeout,
                    collaboration_central_core::CENTRAL_FRAME_CAPABILITIES,
                )?,
                #[cfg(feature = "conformance")]
                observer,
            )),
        })
    }
}

fn kernel<Face: KernelFace>(
    face: Face,
    #[cfg(feature = "conformance")] observer: Arc<
        dyn collaboration_central_core::event_trace::KernelObserver,
    >,
) -> ObjectKernel<Face> {
    #[cfg(feature = "conformance")]
    {
        ObjectKernel::observed(face, observer)
    }
    #[cfg(not(feature = "conformance"))]
    {
        ObjectKernel::new(face)
    }
}

/// The slot's current kernel and how many events and calls are inside it.
pub(super) struct KernelCell {
    entry: Mutex<KernelEntry>,
    #[cfg(feature = "conformance")]
    recipe: KernelRecipe,
}

struct KernelEntry {
    current: Arc<RuntimeFace>,
    active: u64,
}

/// Counts one event or call as running until dropped.
struct Active<'cell>(&'cell KernelCell);

impl Drop for Active<'_> {
    fn drop(&mut self) {
        let mut entry = self.0.entry();
        entry.active = entry.active.saturating_sub(1);
    }
}

impl KernelCell {
    pub(super) fn new(
        current: RuntimeFace,
        #[cfg(feature = "conformance")] recipe: KernelRecipe,
    ) -> Self {
        Self {
            entry: Mutex::new(KernelEntry {
                current: Arc::new(current),
                active: 0,
            }),
            #[cfg(feature = "conformance")]
            recipe,
        }
    }

    fn entry(&self) -> MutexGuard<'_, KernelEntry> {
        self.entry.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(super) fn current(&self) -> Arc<RuntimeFace> {
        self.entry().current.clone()
    }

    fn enter(
        &self,
        #[cfg(feature = "conformance")] io: &TokioObjectIo,
    ) -> (Arc<RuntimeFace>, Active<'_>) {
        let mut entry = self.entry();
        #[cfg(feature = "conformance")]
        if entry.active == 0 && io.seams.rebuild_at_quiescence() {
            match self.recipe.build(
                io,
                collaboration_central_core::event_trace::ConstructionCause::Rebuild,
            ) {
                Ok(rebuilt) => entry.current = Arc::new(rebuilt),
                Err(failure) => eprintln!("central kernel rebuild failed: {failure}"),
            }
        }
        entry.active += 1;
        (entry.current.clone(), Active(self))
    }

    /// Events and calls currently inside the kernel.
    #[cfg(feature = "conformance")]
    pub(super) fn active(&self) -> u64 {
        self.entry().active
    }
}

pub(super) struct ObjectSlot {
    pub(super) io: TokioObjectIo,
    pub(super) host: OnceCell<Arc<NativeHost>>,
    pub(super) kernel: OnceCell<KernelCell>,
    pub(super) alarm_task: OnceLock<()>,
    #[cfg(test)]
    pub(super) alarm_owner_generation: tokio::sync::watch::Sender<u64>,
}

impl ObjectSlot {
    pub(super) fn current_runtime(&self) -> Option<Arc<RuntimeFace>> {
        self.kernel.get().map(KernelCell::current)
    }

    /// Runs one event in the slot's current kernel, or `None` before it is built.
    pub(super) async fn on_event(
        &self,
        event: ObjectEvent,
        #[cfg(feature = "conformance")]
        context: collaboration_central_core::event_trace::EventContext,
    ) -> Option<EventReply> {
        let cell = self.kernel.get()?;
        let (runtime, _active) = cell.enter(
            #[cfg(feature = "conformance")]
            &self.io,
        );
        Some(
            runtime
                .on_event(
                    event,
                    #[cfg(feature = "conformance")]
                    context,
                )
                .await,
        )
    }
}
