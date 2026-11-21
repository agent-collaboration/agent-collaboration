//! One typed exchange with an addressed service. Hosts own the transport.
#![forbid(unsafe_code)]

mod rpc_exchange;

pub use rpc_exchange::{
    CallError, CallFailure, Envelope, Rpc, RpcClient, TargetMismatch, Transport, dispatch,
};
