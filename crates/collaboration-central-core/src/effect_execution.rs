//! The edge's typed request families are encoded once by the application.
use crate::{ObjectKind, RegistryKind, WireCall};
pub trait WireKind: ObjectKind {
    fn call(value: Self::Call) -> WireCall;
}
impl WireKind for RegistryKind {
    fn call(value: Self::Call) -> WireCall {
        WireCall::Registry(value)
    }
}
