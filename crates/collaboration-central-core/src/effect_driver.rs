//! Local registration publication precedes external session effects.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::arithmetic_side_effects,
    clippy::unimplemented,
    clippy::unreachable
)]

use crate::session_store::SessionTransportEffect;
use crate::{
    CoreHost, ObjectHost, PlatformIo,
    session_store::{PendingWelcome, SessionCommit},
    step_driver::CommittedEffects,
};
use board_sync_protocol::BoardConsumerProtocol;
use durable_steps::Host as _;
use fleet_sync_protocol::{CloseReason, ConnectionId, ServerFrame};
use runtime_primitives::{Clock, WakeScheduler};
use std::{collections::BTreeSet, sync::Arc};
mod archive_reports;
mod automation_requests;
mod copy_reads;
mod copy_writes;
mod fleet_reads;
mod fleet_requests;
mod forwards;
mod installation_reads;
mod mail_submit;
mod mailbox_requests;

pub(crate) struct EffectDriver<Host: ObjectHost, Io: PlatformIo> {
    host: Arc<Host>,
    shared_host: Arc<CoreHost<Io>>,
    replies: Arc<crate::holder_reply_hub::HolderReplyHub>,
}
impl<Host: ObjectHost, Io: PlatformIo> Clone for EffectDriver<Host, Io> {
    fn clone(&self) -> Self {
        Self {
            host: self.host.clone(),
            shared_host: self.shared_host.clone(),
            replies: self.replies.clone(),
        }
    }
}

/// Constructed after SQL commit and local publication, while exclusion held.
pub(crate) struct PublishedSessionEffects {
    welcomes: Vec<PendingWelcome>,
    transport: Vec<crate::session_store::SessionTransportEffect>,
    close: BTreeSet<ConnectionId>,
    wake_at: Option<runtime_primitives::SyncTime>,
}

impl<Host: ObjectHost, Io: PlatformIo> EffectDriver<Host, Io> {
    pub(crate) fn new(
        host: Arc<Host>,
        shared_host: Arc<CoreHost<Io>>,
        replies: Arc<crate::holder_reply_hub::HolderReplyHub>,
    ) -> Self {
        Self {
            host,
            shared_host,
            replies,
        }
    }

    pub(crate) async fn publish_registrations(
        &self,
        committed: CommittedEffects<SessionCommit>,
    ) -> PublishedSessionEffects {
        let mut published = PublishedSessionEffects {
            welcomes: Vec::new(),
            transport: Vec::new(),
            close: BTreeSet::new(),
            wake_at: None,
        };
        for commit in committed.into_effects() {
            for (connection, _) in self.host.publish_registrations(commit.registrations).await {
                published.close.insert(connection);
            }
            published.welcomes.extend(commit.effects);
            published.transport.extend(commit.transport);
            if let Some(at) = commit.wake_at {
                published.wake_at = Some(published.wake_at.map_or(at, |existing| existing.min(at)));
            }
        }
        self.host
            .quarantine_connections(published.close.iter().copied().collect())
            .await;
        published
    }

    /// Network waits and physical closes occur after the local section releases.
    pub(crate) async fn finish(&self, published: PublishedSessionEffects)
    where
        Host::Store: durable_steps::ReborrowScope,
        for<'transaction> <Host::Store as durable_steps::AtomicStore>::Scope<'transaction>:
            durable_steps::storage::TransactionScope<Database = Host::Database>,
        <Host::Database as sqlx_core::database::Database>::Arguments:
            sqlx_core::arguments::IntoArguments<Host::Database>,
        for<'value> String: sqlx_core::types::Type<Host::Database>
            + sqlx_core::encode::Encode<'value, Host::Database>
            + sqlx_core::decode::Decode<'value, Host::Database>,
        for<'value> Option<String>: sqlx_core::types::Type<Host::Database>
            + sqlx_core::encode::Encode<'value, Host::Database>
            + sqlx_core::decode::Decode<'value, Host::Database>,
        for<'row> &'row str:
            sqlx_core::column::ColumnIndex<<Host::Database as sqlx_core::database::Database>::Row>,
    {
        if let Some(at) = published.wake_at {
            // The journal already owns this deadline. Failed activation cannot
            // turn committed admission into rollback; restart drives due.
            let _ = self.host.wake().wake_at(at).await;
        }
        for connection in &published.close {
            self.replies.fail_connection(*connection);
            let _ = self
                .host
                .close_connection(*connection, CloseReason::ClientClosed)
                .await;
        }
        for effect in published.transport {
            match effect {
                SessionTransportEffect::ForwardRoute { pending, post } => {
                    if !published.close.contains(&pending.connection) {
                        self.route_forward(pending, post).await;
                    }
                }
                SessionTransportEffect::ForwardDelivery {
                    installation,
                    request,
                    last_seen,
                } => self.deliver_forward(installation, request, last_seen).await,
                SessionTransportEffect::ForwardReply {
                    installation,
                    request,
                } => self.reply_forward(installation, request).await,
                SessionTransportEffect::ForwardFrame {
                    connection,
                    session,
                    bytes,
                } => {
                    if published.close.contains(&connection)
                        || !self.is_attached(connection, session.session_id).await
                    {
                        continue;
                    }
                    let live = matches!(self.host.admission_binding(connection).await,Ok(binding) if binding.connection==connection && binding.peer==session.peer && binding.expires_at>self.shared_host.now());
                    if live && self.host.send_frame(connection, bytes).await.is_err() {
                        self.fail_connection(connection).await;
                    }
                }
                SessionTransportEffect::FleetRead {
                    connection,
                    session,
                    frame,
                    request,
                } => {
                    if !published.close.contains(&connection) {
                        self.answer_fleet_read(connection, session, frame, request)
                            .await;
                    }
                }
                SessionTransportEffect::InstallationRead {
                    connection,
                    session,
                    frame,
                    request,
                } => {
                    if !published.close.contains(&connection) {
                        self.answer_installation_read(connection, session, frame, request)
                            .await;
                    }
                }
                SessionTransportEffect::MailboxRequest {
                    connection,
                    session,
                    frame,
                    request,
                } => {
                    if !published.close.contains(&connection) {
                        self.answer_mailbox_request(connection, session, frame, request)
                            .await;
                    }
                }
                SessionTransportEffect::FleetRequest {
                    connection,
                    session,
                    frame,
                    request,
                } => {
                    if !published.close.contains(&connection) {
                        self.answer_fleet_request(connection, session, frame, request)
                            .await;
                    }
                }
                SessionTransportEffect::AutomationRequest {
                    connection,
                    session,
                    frame,
                    request,
                } => {
                    if !published.close.contains(&connection) {
                        self.answer_automation_request(connection, session, frame, request)
                            .await;
                    }
                }
                SessionTransportEffect::ArchiveReport {
                    connection,
                    session,
                    frame,
                    report,
                } => {
                    if !published.close.contains(&connection) {
                        self.answer_archive_report(connection, session, frame, report)
                            .await;
                    }
                }
                SessionTransportEffect::MailSubmit {
                    connection,
                    session,
                    frame,
                    item,
                    origin_epoch,
                } => {
                    if !published.close.contains(&connection) {
                        self.answer_mail_submit(connection, session, frame, item, origin_epoch)
                            .await;
                    }
                }
                SessionTransportEffect::FleetEffects {
                    connection,
                    session,
                    frame,
                    request,
                } => {
                    if !published.close.contains(&connection) {
                        self.answer_fleet_effects(connection, session, frame, request)
                            .await;
                    }
                }
                SessionTransportEffect::CopyWrite {
                    connection,
                    session,
                    frame,
                    database,
                    request,
                } => {
                    if !published.close.contains(&connection) {
                        self.answer_copy_write(connection, session, frame, database, *request)
                            .await;
                    }
                }
                SessionTransportEffect::CopyRead {
                    connection,
                    session,
                    frame,
                    database,
                    request,
                } => {
                    if !published.close.contains(&connection) {
                        self.answer_copy_read(connection, session, frame, database, request)
                            .await;
                    }
                }
                SessionTransportEffect::PeerAnswer {
                    connection,
                    session,
                    frame,
                    answer,
                } => {
                    if !published.close.contains(&connection) {
                        self.replies.resolve(
                            session,
                            connection,
                            frame,
                            answer,
                            self.shared_host.now(),
                        );
                    }
                }
                SessionTransportEffect::LeaseRequest {
                    connection,
                    session,
                    frame,
                    request,
                } => {
                    if published.close.contains(&connection)
                        || !self.is_attached(connection, session.session_id).await
                    {
                        continue;
                    }
                    let identity = self.shared_host.call_identity("session_lease");
                    let Ok(identity) = identity else {
                        self.fail_connection(connection).await;
                        continue;
                    };
                    let answer = self
                        .shared_host
                        .dispatch(
                            crate::session_calls::SessionLeaseRegistry,
                            crate::session_calls::SessionLeaseRequest { session, request },
                            identity,
                        )
                        .await;
                    let Ok(answer) = answer else {
                        self.fail_connection(connection).await;
                        continue;
                    };
                    if !self.is_attached(connection, session.session_id).await {
                        continue;
                    }
                    let reply = ServerFrame::<BoardConsumerProtocol>::Answer {
                        frame,
                        body: fleet_sync_protocol::OperationAnswer::Lease(answer),
                    };
                    match serde_json::to_vec(&reply) {
                        Ok(bytes) => {
                            if self
                                .host
                                .send_frame(connection, bytes.into())
                                .await
                                .is_err()
                            {
                                self.fail_connection(connection).await;
                            }
                        }
                        Err(_) => self.fail_connection(connection).await,
                    }
                }
                SessionTransportEffect::Frame { connection, bytes } => {
                    if !published.close.contains(&connection)
                        && self.host.send_frame(connection, bytes).await.is_err()
                    {
                        self.fail_connection(connection).await;
                    }
                }
                SessionTransportEffect::Close { connection, reason } => {
                    self.replies.fail_connection(connection);
                    let _ = self.host.close_connection(connection, reason).await;
                }
            }
        }
        for PendingWelcome {
            connection,
            peer,
            session_id,
        } in published.welcomes
        {
            if published.close.contains(&connection) {
                continue;
            }
            let sent = match self.shared_host.gather_welcome(peer).await {
                Ok(databases) => {
                    match serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Welcome {
                        session_id,
                        databases,
                    }) {
                        Ok(bytes) => {
                            // Gathering may yield to a replacement connection. Recheck
                            // the registration after that wait, before using its result.
                            let host = self.host.clone();
                            let current = self
                                .host
                                .with_local_section(async move {
                                    host.registrations().await.ok().and_then(|registrations| {
                                        registrations.get(&connection).copied()
                                    })
                                })
                                .await;
                            if !matches!(current, Ok(Some(fleet_sync_session::ConnectionRegistration::Attached { session_id: current, .. })) if current == session_id)
                            {
                                continue;
                            }
                            self.host.send_frame(connection, bytes.into()).await.is_ok()
                        }
                        Err(_) => false,
                    }
                }
                Err(_) => false,
            };
            if !sent {
                self.fail_connection(connection).await;
            }
        }
    }
    async fn is_attached(
        &self,
        connection: ConnectionId,
        session: fleet_sync_protocol::SessionId,
    ) -> bool {
        let host = self.host.clone();
        matches!(self.host.with_local_section(async move {
            host.registrations().await.ok().and_then(|registrations| registrations.get(&connection).copied())
        }).await, Ok(Some(fleet_sync_session::ConnectionRegistration::Attached { session_id, .. })) if session_id == session)
    }
    async fn fail_connection(&self, connection: ConnectionId) {
        self.replies.fail_connection(connection);
        let host = self.host.clone();
        let _ = self
            .host
            .with_local_section(async move { host.quarantine_connections(vec![connection]).await })
            .await;
        let _ = self
            .host
            .close_connection(connection, CloseReason::ClientClosed)
            .await;
    }
}

/// A test-only sink for observing the minimal real rule's released effects.
#[cfg(feature = "conformance")]
pub(crate) fn conformance_effects<Effect>(committed: CommittedEffects<Effect>) -> Vec<Effect> {
    committed.into_effects()
}
