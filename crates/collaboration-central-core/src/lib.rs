//! **Superseded structure (2026-10-16).** Two parts of this crate's design are superseded:
//! - its synchronous storage boundary (`SqlTx`, and `step_*` functions over `&mut dyn SqlTx`);
//! - the per-runtime application drivers built around it in `collaboration-central-cloudflare`
//!   and `collaboration-central-local`.
//!
//! Together they violate the owner's requirement: one asynchronous core, injected into every runtime
//! through a common trait, with SQLx as the database API (requirements U69 to U75). The rules in
//! this crate carry over; the boundary does not. Don't extend it. See
//! `docs/specs/2026-10-16-central-one-core/program-design.md` and `docs/specs/2026-10-16-central-one-core/what-went-wrong.md`.
#![forbid(unsafe_code)]

mod access_contract;
mod edge_contract;
pub use edge_contract::*;
mod access_rules;
mod bookkeeping_migrations;
mod bound_sql;
pub use bookkeeping_migrations::{
    COPY_BOOKKEEPING_VERSION, CopyMigrationFailure, CopyMigrationReply, CopyMigrationResult,
    migrate_copy_bookkeeping, record_copy_migration_failure,
};
mod import_contract;
pub use import_contract::*;
mod lease_admission;
mod lease_incarnation;
mod lease_publication;
mod lease_publish_storage;
mod lease_version;
pub use lease_version::*;
mod archive_receiver;
mod claim_cleanup;
mod claim_reservation;
mod claim_reservation_contract;
mod claim_rules;
pub use claim_reservation_contract::*;
mod delivery_rules;
mod directory_producer;
mod forward_contract;
mod forward_delivery;
mod forward_rules;
mod import_rules;
pub use access_contract::*;
pub use access_rules::*;
pub use forward_contract::{DispatchCarry, ForwardDeliveryResult, ForwardRouteResult};
mod copy_bookkeeping;
mod copy_operations;
mod copy_piece_time;
mod copy_retirement;
pub use copy_operations::{
    CopyAdminFailure, CopyHealth, CopyHealthRead, CopyMigrationFailureRecord, CopyOpsCall,
    LiveCopyHealth, StagedPartHealth,
};
mod copy_receipts;
pub use copy_bookkeeping::{CopyBookkeepingMetadata, read_copy_bookkeeping_metadata};
mod copy_rules;
mod copy_storage;
mod core_contract;
mod core_kind;
mod core_steps;
mod effect_execution;
pub use core_kind::*;
pub use core_steps::*;
pub use effect_execution::*;
mod object_calls;
mod object_inputs;
pub use object_inputs::*;
mod alarm_plan;
mod call_edge;
mod call_graph;
mod object_effects;
pub use alarm_plan::*;
pub use call_edge::*;
mod wire_contract;
pub use call_graph::*;
pub use object_calls::*;
pub use object_effects::*;
pub use wire_contract::*;
mod directory_read;
mod directory_receiver;
mod fence_rules;
mod fleet_feed;
mod fleet_notice_outbox;
mod fleet_request_admission;
mod fleet_rules;
mod fleet_snapshot;
mod fleet_storage;
mod fleet_tail_reports;
mod part_assembly;
mod registry_contract;
mod registry_dispatch;
mod registry_rules;
mod registry_storage;
mod registry_tail_verification;
mod row_apply;
mod session_notices;
mod tail_verification;
pub use tail_verification::{TailVerificationRefusal, TailVerificationResult};
mod session_reply;
mod session_rules;
mod session_tail_report;
mod snapshot_reads;
mod storage_port;
mod transfer_completion;
mod transfer_rules;

pub use core_contract::*;
pub use registry_contract::*;
pub use storage_port::*;

/// Decode only at the actor/serialized-control boundary. Rule handlers below it
/// take a per-face input and return that face's effect type.
pub fn step(
    tx: &mut dyn SqlTx,
    input: WireInput,
    now: RegistryTime,
    ids: IdBatch,
) -> Result<ObjectEffects, CentralError> {
    if matches!(&input,WireInput::Call { call,.. } if matches!(call.as_ref(),WireCall::Copy(CopyCall::Ops(CopyOpsCall::Health {}))))
    {
        return Err(CentralError::InvalidInput {
            message: "Health requires the target shell's measurement".into(),
        });
    }
    let face = match &input {
        WireInput::Call { call, .. } => call.kind(),
        WireInput::Session(_) => CallerKind::Session,
        WireInput::CallResult { source, .. } | WireInput::CallFailed { source, .. } => *source,
        WireInput::Alarm { face } => *face,
    };
    macro_rules! run {
        ($kind:ty) => {
            <$kind>::step(tx, <$kind>::decode(input)?, now, ids).map(<$kind>::pack)
        };
    }
    match face {
        CallerKind::Session => run!(SessionKind),
        CallerKind::Registry => run!(RegistryKind),
        CallerKind::Copy => run!(CopyKind),
        CallerKind::Fleet => run!(FleetKind),
        CallerKind::Delivery => run!(DeliveryKind),
        CallerKind::Edge => Err(CentralError::ReplyContract),
    }
}
