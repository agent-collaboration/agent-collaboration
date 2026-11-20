//! The capabilities a host supplies to FleetRuntime. Traits and the leaf values
//! they carry only: no behavior, no Tokio and no Cloudflare type.
#![forbid(unsafe_code)]

mod central_observer;
mod central_store;
mod issuer_key_source;
mod signature_crypto;
mod time_and_ids;
mod workspace_stores;

pub use central_engine_model::EngineTime;
pub use central_observer::Observer;
pub use central_store::CentralStore;
pub use issuer_key_source::{IssuerKeySource, KeyFetchFailure, RawJwks};
pub use signature_crypto::{
    Ed25519Verification, Es256Verification, ProofJwk, SignatureValidity, VerifyFailure,
};
pub use time_and_ids::{Clock, IdSource, SyncTime, canonical_uuid_v7, uuid_v7};
pub use workspace_stores::WorkspaceStores;

pub mod guarded_statements;
pub mod stored_values;
pub mod workspace_decode_error;
