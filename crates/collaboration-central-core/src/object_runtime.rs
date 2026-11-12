//! One physical Fleet owner.
use crate::{CoreHost, ObjectHost, ObjectKind, PlatformIo};
use std::{marker::PhantomData, sync::Arc};

pub struct ObjectRuntime<Face: ObjectKind, Host: ObjectHost, Io: PlatformIo> {
    pub(crate) work: ObjectWork<Host, Io>,
    pub(crate) face: PhantomData<fn() -> Face>,
}

pub(crate) enum ObjectWork<Host: ObjectHost, Io: PlatformIo> {
    Fleet {
        host: Arc<Host>,
        io: Arc<Io>,
        engine: Arc<dyn crate::fleet_task_engine::FleetTaskEngine>,
        shared_host: Arc<CoreHost<Io>>,
        delivery: crate::DeliveryConfig,
    },
}
