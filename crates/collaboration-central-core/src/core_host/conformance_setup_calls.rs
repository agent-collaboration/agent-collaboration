//! Conformance setup that writes central state runs as a Fleet call event.
//!
//! The board-import fixtures used to call the Fleet face directly, outside the
//! event pipeline: work they owed (notices, socket requests) then had no event
//! to send it after commit, and owing it outside an event is refused
//! (`event_owed::NoEventScope`). Each shell now sends these as ordinary owner
//! calls ([`encode_conformance_setup`]), so they enter the kernel like any
//! other call: recorded in S1, owed work sent after the event.

use crate::{
    CallerContext, EdgeKind, FleetKind, ObjectRuntime, OwnerIdentity, SerializedObjectCall,
    caller_context::CallAuthority, object_call_envelope::ObjectCallEnvelope,
};
use durable_steps::{
    AtomicStore, CallFailure, CallId, CallTarget, NotSentReason, storage::TransactionScope,
};
use fleet_protocol::{DatabaseId, ReplicationPosition, SnapshotManifest, SnapshotPage};
use serde::{Deserialize, Serialize};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

/// Import a captured board snapshot through the real import and seed paths.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceImportBoardSnapshot;
impl CallTarget for ConformanceImportBoardSnapshot {
    const KIND: &'static str = "central.conformance-import-board-snapshot";
    type Request = ImportBoardSnapshotRequest;
    type Answer = ReplicationPosition;
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportBoardSnapshotRequest {
    pub board_state: crate::ConformanceBoardState,
    pub pages: Vec<SnapshotPage>,
    pub manifest: SnapshotManifest,
}

/// Import a project that no Router holds, with one seeded row.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConformanceImportedUnheld;
impl CallTarget for ConformanceImportedUnheld {
    const KIND: &'static str = "central.conformance-imported-unheld";
    type Request = ImportedUnheldRequest;
    type Answer = ReplicationPosition;
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportedUnheldRequest {
    pub database: DatabaseId,
    pub board_state: crate::ConformanceBoardState,
}

/// The owner call a shell sends to the Fleet object to run `target`.
pub fn encode_conformance_setup<Call: CallTarget>(
    owner: OwnerIdentity,
    target: Call,
    request: Call::Request,
    identity: CallId,
) -> Result<SerializedObjectCall, CallFailure> {
    crate::object_call_envelope::encode_shared_call::<EdgeKind, FleetKind, Call>(
        &CallerContext::<EdgeKind>::for_owner(owner),
        (),
        target,
        request,
        identity,
    )
}

/// Whether `kind` names a conformance setup call.
pub(crate) fn is_setup(kind: &str) -> bool {
    kind == ConformanceImportBoardSnapshot::KIND || kind == ConformanceImportedUnheld::KIND
}

impl<Host: crate::ObjectPlatform> ObjectRuntime<FleetKind, Host>
where
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    /// Run a conformance setup call from an owner.
    pub(crate) async fn conformance_setup_call(
        &self,
        call: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        let CallAuthority::Owner(owner) = call.authority else {
            return Err(refused());
        };
        let position = if call.kind == ConformanceImportBoardSnapshot::KIND {
            let request: ImportBoardSnapshotRequest =
                serde_json::from_value(call.request).map_err(|_| refused())?;
            self.conformance_import_board_snapshot(
                owner,
                request.board_state,
                request.pages,
                request.manifest,
            )
            .await
        } else {
            let request: ImportedUnheldRequest =
                serde_json::from_value(call.request).map_err(|_| refused())?;
            self.conformance_imported_unheld(request.database, owner, request.board_state)
                .await
        };
        let position = position.map_err(|failure| {
            eprintln!("conformance setup {} failed: {failure}", call.kind);
            CallFailure::AnswerLost
        })?;
        serde_json::to_vec(&position).map_err(|_| CallFailure::AnswerLost)
    }
}

fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

pub use crate::object_transport::encode_edge_registry_call;
