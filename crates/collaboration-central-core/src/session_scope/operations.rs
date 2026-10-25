//! An admitted frame stages exactly one operation; consumer cases are exhaustive.
use super::*;
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardConsumerRequest, BoardReadCursor,
    BoardReadPage, BoardReadScope,
};
use fleet_sync_protocol::{FrameId, OperationAnswer, OperationRequest, ServerFrame};
use fleet_sync_session::SessionKey;

impl<Host: ObjectHost, Io: PlatformIo> CoreSessionScope<'_, Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub(super) async fn stage_operation(
        &mut self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        operation: OperationRequest<BoardConsumerProtocol>,
        now: SyncTime,
        forward_frame: FrameId,
    ) -> Result<(), StoreError> {
        if let Some(reason) =
            crate::session_request_admission::refuse_register_from_another_peer(session, &operation)
        {
            return self.stage_operation_answer(
                connection,
                frame,
                OperationAnswer::Lease(fleet_sync_protocol::LeaseAnswer::Register(
                    fleet_sync_protocol::RegistrationAnswer::Refused(reason),
                )),
            );
        }
        match operation {
            OperationRequest::Consumer(request) => {
                self.stage_consumer_request(connection, session, frame, request, now, forward_frame)
                    .await?
            }
            OperationRequest::Lease(request) => {
                self.transport.push(SessionTransportEffect::LeaseRequest {
                    connection,
                    session,
                    frame,
                    request,
                })
            }
            OperationRequest::Delivery(request) => {
                use fleet_sync_protocol::DeliveryRequest;
                match request {
                    request @ (DeliveryRequest::Fetch { .. }
                    | DeliveryRequest::Received { .. }
                    | DeliveryRequest::Report { .. }
                    | DeliveryRequest::Query { .. }
                    | DeliveryRequest::Settle { .. }
                    | DeliveryRequest::Retire { .. }) => {
                        self.transport.push(SessionTransportEffect::MailboxRequest {
                            connection,
                            session,
                            frame,
                            request: Box::new(request),
                        })
                    }
                    DeliveryRequest::StatusAck { .. } | DeliveryRequest::StatusFetch { .. } => {
                        return Err(StoreError::Aborted {
                            reason: "status-stream request bypassed its local frame handler".into(),
                        });
                    }
                    // Board mail must enter MailSubmit's provenance/epoch gate.
                    // Raw Submit is a permanent designed refusal (decision523).
                    DeliveryRequest::Submit { .. } => self.stage_operation_answer(
                        connection,
                        frame,
                        OperationAnswer::InvalidShape {
                            field: "operation".into(),
                        },
                    )?,
                }
            }
            OperationRequest::Replication(request) => {
                use crate::copy_read_calls::CopyReadRequest;
                use fleet_sync_protocol::ReplicationRequest;
                let (database, request) = match request {
                    ReplicationRequest::CopyStatus { database } => {
                        (database, CopyReadRequest::Status)
                    }
                    ReplicationRequest::SnapshotManifest { database, position } => {
                        (database, CopyReadRequest::Manifest { position })
                    }
                    ReplicationRequest::SnapshotPage { database, cursor } => {
                        (database, CopyReadRequest::Page { cursor })
                    }
                    request => {
                        let database = crate::copy_write_calls::write_database(&request)
                            .ok_or_else(|| StoreError::Aborted {
                                reason: "unknown replication write request".into(),
                            })?;
                        self.transport.push(SessionTransportEffect::CopyWrite {
                            connection,
                            session,
                            frame,
                            database,
                            request: Box::new(crate::copy_write_calls::WriterCopyRequest {
                                sender: session.peer,
                                request,
                            }),
                        });
                        return Ok(());
                    }
                };
                self.transport.push(SessionTransportEffect::CopyRead {
                    connection,
                    session,
                    frame,
                    database,
                    request,
                });
            }
        }
        Ok(())
    }
    async fn stage_consumer_request(
        &mut self,
        connection: ConnectionId,
        session: SessionKey,
        frame: FrameId,
        request: BoardConsumerRequest,
        now: SyncTime,
        forward_frame: FrameId,
    ) -> Result<(), StoreError> {
        match request {
            BoardConsumerRequest::ForwardedPost { project, post } => {
                let pending = crate::forward_store::PendingForward {
                    forward_frame,
                    request_frame: frame,
                    connection,
                    session,
                    project,
                    phase: crate::forward_store::ForwardPhase::Routing,
                    deadline: now.after(std::time::Duration::from_secs(30)),
                };
                self.stage_forward(pending, post).await?;
            }
            BoardConsumerRequest::Fleet(request) => {
                self.transport.push(SessionTransportEffect::FleetRequest {
                    connection,
                    session,
                    frame,
                    request,
                })
            }
            BoardConsumerRequest::MailSubmit { item, origin_epoch } => {
                self.transport.push(SessionTransportEffect::MailSubmit {
                    connection,
                    session,
                    frame,
                    item,
                    origin_epoch,
                })
            }
            BoardConsumerRequest::ArchiveSeen(report) => {
                self.transport.push(SessionTransportEffect::ArchiveReport {
                    connection,
                    session,
                    frame,
                    report,
                })
            }
            BoardConsumerRequest::Read { scope, cursor } => match scope {
                BoardReadScope::InstallationPeer { service } => {
                    if cursor.is_some() {
                        return self.stage_operation_answer(
                            connection,
                            frame,
                            OperationAnswer::InvalidShape {
                                field: "cursor".into(),
                            },
                        );
                    }
                    self.transport
                        .push(SessionTransportEffect::InstallationRead {
                            connection,
                            session,
                            frame,
                            request:
                                crate::installation_peer_reads::InstallationReadRequest::Lookup(
                                    service,
                                ),
                        });
                }
                BoardReadScope::InstallationPeers { after } => {
                    if cursor.is_some() {
                        return self.stage_operation_answer(
                            connection,
                            frame,
                            OperationAnswer::InvalidShape {
                                field: "cursor".into(),
                            },
                        );
                    }
                    self.transport
                        .push(SessionTransportEffect::InstallationRead {
                            connection,
                            session,
                            frame,
                            request: crate::installation_peer_reads::InstallationReadRequest::Page(
                                after,
                            ),
                        });
                }
                scope @ (BoardReadScope::FleetRecords
                | BoardReadScope::FleetSnapshot
                | BoardReadScope::Directory { .. }) => {
                    self.stage_fleet_read(connection, session, frame, scope, cursor)?
                }
                BoardReadScope::RequestEffects {
                    workspace,
                    request,
                    after,
                } => {
                    if cursor.is_some() {
                        return self.stage_operation_answer(
                            connection,
                            frame,
                            OperationAnswer::InvalidShape {
                                field: "cursor".into(),
                            },
                        );
                    }
                    self.transport.push(SessionTransportEffect::FleetEffects {
                        connection,
                        session,
                        frame,
                        request: message_board::RequestEffectsRequest {
                            scope: message_board::RequestEffectsScope::Fleet {
                                workspace_id: workspace,
                            },
                            request,
                            after,
                        },
                    });
                }
                BoardReadScope::Notices => {
                    let after = match cursor {
                        None => None,
                        Some(BoardReadCursor::Notices(after)) => Some(after),
                        Some(
                            BoardReadCursor::Fleet(_)
                            | BoardReadCursor::FleetSnapshot(_)
                            | BoardReadCursor::Snapshot(_),
                        ) => {
                            return self.stage_operation_answer(
                                connection,
                                frame,
                                OperationAnswer::InvalidShape {
                                    field: "cursor".into(),
                                },
                            );
                        }
                    };
                    let page = crate::machine_notice_store::read(self, after, now).await?;
                    self.stage_operation_answer(
                        connection,
                        frame,
                        OperationAnswer::Consumer(BoardConsumerAnswer::Read(
                            BoardReadPage::Notices(page),
                        )),
                    )?;
                }
            },
        }
        Ok(())
    }
    pub(super) fn stage_operation_answer(
        &mut self,
        connection: ConnectionId,
        frame: FrameId,
        body: OperationAnswer<BoardConsumerProtocol>,
    ) -> Result<(), StoreError> {
        self.transport.push(SessionTransportEffect::Frame {
            connection,
            bytes: serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Answer {
                frame,
                body,
            })
            .map_err(StoreError::backend)?
            .into(),
        });
        Ok(())
    }
}
