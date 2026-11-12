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
    ObjectHost, PlatformIo,
    session_store::{PendingWelcome, SessionCommit},
    step_driver::CommittedEffects,
};
use board_sync_protocol::BoardConsumerProtocol;
use fleet_sync_protocol::{CloseReason, ConnectionId, ServerFrame};
use runtime_primitives::WakeScheduler;
use std::{collections::BTreeSet, sync::Arc};

pub(crate) struct EffectDriver<Host: ObjectHost, Io: PlatformIo> {
    host: Arc<Host>,
    marker: std::marker::PhantomData<fn() -> Io>,
}
impl<Host: ObjectHost, Io: PlatformIo> Clone for EffectDriver<Host, Io> {
    fn clone(&self) -> Self {
        Self {
            host: self.host.clone(),
            marker: std::marker::PhantomData,
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
    pub(crate) fn new(host: Arc<Host>) -> Self {
        Self {
            host,
            marker: std::marker::PhantomData,
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
        for<'scalar> i64: sqlx_core::types::Type<Host::Database>
            + sqlx_core::encode::Encode<'scalar, Host::Database>
            + sqlx_core::decode::Decode<'scalar, Host::Database>,
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
            let _ = self
                .host
                .close_connection(*connection, CloseReason::ClientClosed)
                .await;
        }
        for effect in published.transport {
            match effect {
                SessionTransportEffect::Frame { connection, bytes } => {
                    if !published.close.contains(&connection)
                        && self.host.send_frame(connection, bytes).await.is_err()
                    {
                        self.fail_connection(connection).await;
                    }
                }
                SessionTransportEffect::Close { connection, reason } => {
                    let _ = self.host.close_connection(connection, reason).await;
                }
            }
        }
        for PendingWelcome {
            connection,
            session_id,
        } in published.welcomes
        {
            if published.close.contains(&connection) {
                continue;
            }
            let sent = {
                {
                    match serde_json::to_vec(&ServerFrame::<BoardConsumerProtocol>::Welcome {
                        session_id,
                        databases: Vec::new(),
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
            };
            if !sent {
                self.fail_connection(connection).await;
            }
        }
    }
    async fn fail_connection(&self, connection: ConnectionId) {
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
