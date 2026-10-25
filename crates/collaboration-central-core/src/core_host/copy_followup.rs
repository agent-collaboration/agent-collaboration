//! The copy's journal invokes the board follow-up after its publication commits.
use super::*;
use crate::{CopyDirectorySummary, copy_directory_summary::DirectorySummaryUpdate};
use fleet_sync_receiver::{
    FollowUpAnswer, FollowUpCall, FollowUpDone, FollowUpFailure, FollowUpHandler,
};

impl<Io: PlatformIo> CoreHost<Io> {
    pub(crate) fn for_copy(
        io: std::sync::Arc<Io>,
        receiver: ReceiverId,
        database: DatabaseId,
    ) -> Result<Self, CallFailure> {
        Ok(Self {
            io,
            caller: SharedTaskCaller::Copy {
                caller: CallerContext::for_object(database),
                database,
            },
            publication: None,
            authoritative_receiver: receiver,
            #[cfg(feature = "conformance")]
            dispatches: Default::default(),
            #[cfg(feature = "conformance")]
            forward_route_probe: Default::default(),
            #[cfg(feature = "conformance")]
            revocation_probe: Default::default(),
        })
    }
    pub(super) async fn copy_follow_up(
        &self,
        target: serde_json::Value,
        request: serde_json::Value,
        identity: CallId,
    ) -> Result<serde_json::Value, CallFailure> {
        let SharedTaskCaller::Copy { database, .. } = &self.caller else {
            return Err(no_route());
        };
        let target: FollowUpCall<CopyDirectorySummary> = decode(target)?;
        let item: CopyDirectorySummary = decode(request)?;
        if target.database != *database || item.database != *database {
            return Err(no_route());
        }
        let answer = match self.follow_up(&item, identity).await {
            Ok(FollowUpDone) => FollowUpAnswer::Done,
            Err(FollowUpFailure::Retry) => return Err(CallFailure::AnswerLost),
            Err(FollowUpFailure::Failed { reason }) => FollowUpAnswer::Failed { reason },
        };
        serde_json::to_value(answer).map_err(encode_refusal)
    }
}
impl<Io: PlatformIo> FollowUpHandler for CoreHost<Io> {
    type Item = CopyDirectorySummary;
    async fn follow_up(
        &self,
        item: &CopyDirectorySummary,
        identity: CallId,
    ) -> Result<FollowUpDone, FollowUpFailure> {
        let SharedTaskCaller::Copy { caller, database } = &self.caller else {
            return Err(FollowUpFailure::Retry);
        };
        if item.database != *database {
            return Err(FollowUpFailure::Retry);
        }
        let call = encode_shared_call::<CopyKind, FleetKind, DirectorySummaryUpdate>(
            caller,
            (),
            DirectorySummaryUpdate,
            item.clone(),
            identity,
        )
        .map_err(|_| FollowUpFailure::Retry)?;
        let response = self
            .io
            .object_call(call.destination, call.bytes)
            .await
            .map_err(|_| FollowUpFailure::Retry)?;
        let (): () = serde_json::from_slice(&response).map_err(|_| FollowUpFailure::Retry)?;
        Ok(FollowUpDone)
    }
}
