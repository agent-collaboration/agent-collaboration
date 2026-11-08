//! Refuse new shared mutations in the transaction selecting their dispatch.
use super::{
    AtomicStore, ColumnIndex, CoreSessionScope, Database, Decode, Encode, IntoArguments,
    ObjectPlatform, PeerId, ReborrowScope, StoreError, SyncTime, TransactionScope, Type,
};
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardPayload, ForwardOutcome,
};
use fleet_protocol::{
    DeliveryAnswer, DeliveryRefusal, DeliveryRequest, GateReason, GateReasonCode,
    HolderChangeAnswer, HolderChangeRefusal, HookRefusal, HookRefusalCode, LeaseAnswer,
    LeaseRefusal, LeaseRequest, OperationAnswer, OperationRequest, PieceRefusal,
    RegistrationAnswer, RegistrationRefusal, RenewAnswer, ReplicationAnswer, ReplicationRequest,
    Submitter,
};

impl<Host: ObjectPlatform> CoreSessionScope<'_, Host>
where
    Host::Store: ReborrowScope,
    for<'t> <Host::Store as AtomicStore>::Scope<'t>: TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'v> String: Type<Host::Database> + Encode<'v, Host::Database> + Decode<'v, Host::Database>,
    for<'v> Option<String>:
        Type<Host::Database> + Decode<'v, Host::Database> + Encode<'v, Host::Database>,
    for<'r> &'r str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(super) async fn refuse_unenrolled_mutation(
        &mut self,
        peer: PeerId,
        operation: &OperationRequest<BoardConsumerProtocol>,
        now: SyncTime,
    ) -> Result<Option<OperationAnswer<BoardConsumerProtocol>>, StoreError> {
        let reason = || -> Result<GateReason, StoreError> {
            Ok(GateReason {
                code: GateReasonCode::new("callerNotEnrolled").map_err(StoreError::backend)?,
                detail: "Machine is not enrolled or its enrollment has expired.".into(),
            })
        };
        let answer = match operation {
            OperationRequest::NoticesApplied { .. } => None,
            // D2 owns Fleet-originated sends; enrollment never grants Fleet settlement authority.
            OperationRequest::Delivery(DeliveryRequest::Settle {
                submitter: Submitter::Fleet(_),
                ..
            }) => {
                return Ok(Some(OperationAnswer::Delivery(
                    DeliveryAnswer::DeliveryUnavailable {
                        reason: DeliveryRefusal::NotSubmitter,
                    },
                )));
            }
            OperationRequest::Lease(LeaseRequest::Register(_)) => Some(OperationAnswer::Lease(
                LeaseAnswer::Register(RegistrationAnswer::Refused(
                    RegistrationRefusal::DatabaseRejected { reason: reason()? },
                )),
            )),
            OperationRequest::Lease(LeaseRequest::Renew(_)) => Some(OperationAnswer::Lease(
                LeaseAnswer::Renew(RenewAnswer::Refused(LeaseRefusal::HolderNotEnrolled)),
            )),
            OperationRequest::Lease(LeaseRequest::Claim { .. }) => Some(OperationAnswer::Lease(
                LeaseAnswer::Claim(HolderChangeAnswer::Refused(
                    HolderChangeRefusal::GateRefused { reason: reason()? },
                )),
            )),
            OperationRequest::Replication(
                ReplicationRequest::Piece(_)
                | ReplicationRequest::SeedBegin { .. }
                | ReplicationRequest::SeedPage(_)
                | ReplicationRequest::SeedComplete(_),
            ) => Some(OperationAnswer::Replication(ReplicationAnswer::Refused(
                PieceRefusal::Hook(HookRefusal {
                    code: HookRefusalCode::new("callerNotEnrolled").map_err(StoreError::backend)?,
                    detail: "Machine is not enrolled or its enrollment has expired.".into(),
                }),
            ))),
            OperationRequest::Consumer(BoardConsumerRequest::Fleet(request)) => {
                Some(OperationAnswer::Consumer(BoardConsumerAnswer::Fleet {
                    request_id: request.request_id.clone(),
                    decision: BoardPayload::encode(
                        &fleet_admission::FleetRequestResult::CallerNotEnrolled,
                    )
                    .map_err(StoreError::backend)?,
                }))
            }
            OperationRequest::Consumer(BoardConsumerRequest::ArchiveSeen(_)) => {
                Some(OperationAnswer::Consumer(BoardConsumerAnswer::ArchiveSeen(
                    BoardPayload::encode(&fleet_admission::ArchiveReportOutcome::Refused(
                        fleet_admission::ArchiveReportRefusal::CallerNotEnrolled,
                    ))
                    .map_err(StoreError::backend)?,
                )))
            }
            OperationRequest::Consumer(BoardConsumerRequest::ForwardedPost { .. }) => {
                Some(OperationAnswer::Consumer(
                    BoardConsumerAnswer::ForwardedPost(ForwardOutcome::CallerNotEnrolled),
                ))
            }
            OperationRequest::Delivery(DeliveryRequest::Settle {
                submitter: Submitter::Project(_),
                ..
            }) => Some(OperationAnswer::Delivery(
                DeliveryAnswer::DeliveryUnavailable {
                    reason: DeliveryRefusal::SenderNotEnrolled,
                },
            )),
            OperationRequest::Consumer(
                BoardConsumerRequest::Read { .. } | BoardConsumerRequest::MailSubmit { .. },
            )
            | OperationRequest::Delivery(
                DeliveryRequest::Submit { .. }
                | DeliveryRequest::Report { .. }
                | DeliveryRequest::StatusFetch { .. }
                | DeliveryRequest::StatusAck { .. }
                | DeliveryRequest::Query { .. }
                | DeliveryRequest::Settle {
                    submitter: Submitter::Machine(_),
                    ..
                },
            )
            | OperationRequest::Replication(
                ReplicationRequest::SnapshotManifest { .. }
                | ReplicationRequest::SnapshotPage { .. }
                | ReplicationRequest::CopyStatus { .. },
            ) => None,
        };
        if answer.is_none() {
            return Ok(None);
        }
        if crate::machine_enrollment::admission::read_current_enrolled_machine(self, peer, now)
            .await?
            .is_some()
        {
            Ok(None)
        } else {
            Ok(answer)
        }
    }
}
