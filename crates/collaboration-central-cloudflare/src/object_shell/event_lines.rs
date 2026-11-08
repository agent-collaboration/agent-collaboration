//! Conformance only: each event's S1 record and each kernel construction go to
//! stderr as one line. Cloudflare keeps no ring and has no barriers; the
//! Tokio-only seams answer `unsupported` here.

use collaboration_central_core::ObjectAddress;
use collaboration_central_core::event_trace::{
    BarrierPoint, ConformanceLine, ConstructionCause, ConstructionLine, EventRecord, KernelObserver,
};
use durable_steps::{BoxFuture, Uuid};
use std::sync::Arc;

/// Storage key for the `alarmDelivery` switch, kept so it survives eviction.
pub(crate) const ALARM_DELIVERY_KEY: &str = "_cf_composed_alarm_delivery";

struct LineObserver {
    object: ObjectAddress,
    construction: Uuid,
}

/// The observer for one kernel construction; emits its construction line.
/// Every kernel on Cloudflare is built in a fresh Rust `DurableObject::new`.
pub(super) fn observer(object: ObjectAddress, isolate: Uuid) -> Arc<dyn KernelObserver> {
    let construction = Uuid::now_v7();
    ConformanceLine::CentralConstruction(&ConstructionLine {
        object: object.clone(),
        construction,
        cause: ConstructionCause::Isolate,
        isolate,
    })
    .emit();
    Arc::new(LineObserver {
        object,
        construction,
    })
}

impl KernelObserver for LineObserver {
    fn object(&self) -> ObjectAddress {
        self.object.clone()
    }
    fn construction(&self) -> Uuid {
        self.construction
    }
    fn emit(&self, record: EventRecord) {
        ConformanceLine::CentralEvent(&record).emit();
    }
    fn barrier(
        &self,
        _point: BarrierPoint,
        _unit_kind: Option<&'static str>,
        _event: Uuid,
    ) -> BoxFuture<'static, ()> {
        Box::pin(std::future::ready(()))
    }
}
