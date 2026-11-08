//! Owed socket requests go out after the event that owed them commits.
//!
//! After each Fleet event this step sends, within each connection's window,
//! what the event owed (machines named by `event_owed`), plus what a machine
//! whose connection just attached is owed (its Welcome catch-up). The due
//! batch resends requests that went unanswered past their retry time. Both run
//! in a session-store transaction, so each frame leaves only after the row
//! records its send.
use crate::{
    FleetKind, ObjectPlatform, ObjectRuntime,
    object_runtime::ObjectWork,
    work_attempts::{AttemptKind, socket_requests::RequestDestination},
};
use durable_steps::{AtomicStore, ReborrowScope, storage::TransactionScope};
use fleet_protocol::PeerId;
use runtime_primitives::{Clock, IdSource, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::collections::BTreeSet;

impl<Host: ObjectPlatform> ObjectRuntime<FleetKind, Host>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'blob> Vec<u8>: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'blob, Host::Database>
        + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'blob> Option<Vec<u8>>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'blob, Host::Database>,
    for<'scalar> Option<i64>:
        sqlx_core::types::Type<Host::Database> + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'scalar> i64: sqlx_core::types::Type<Host::Database>
        + sqlx_core::encode::Encode<'scalar, Host::Database>
        + sqlx_core::decode::Decode<'scalar, Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    /// Every attached connection, as a place to send a machine's requests.
    async fn request_destinations(&self) -> Vec<RequestDestination> {
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Vec::new();
        };
        let registrations = match host.registrations().await {
            Ok(registrations) => registrations,
            Err(failure) => {
                eprintln!("Fleet request send could not read connections: {failure}");
                return Vec::new();
            }
        };
        let mut destinations = Vec::new();
        for (connection, registration) in registrations {
            let fleet_session::ConnectionRegistration::Attached { session_id, .. } = registration
            else {
                continue;
            };
            let Ok(binding) = host.admission_binding(connection).await else {
                continue;
            };
            destinations.push(RequestDestination {
                peer: binding.peer,
                connection,
                session: session_id,
            });
        }
        destinations
    }

    /// Send what `machines` are owed, each within its connection's window.
    pub(crate) async fn send_owed_requests(&self, machines: BTreeSet<PeerId>) {
        if machines.is_empty() {
            return;
        }
        let destinations: Vec<_> = self
            .request_destinations()
            .await
            .into_iter()
            .filter(|destination| machines.contains(&destination.peer))
            .collect();
        if destinations.is_empty() {
            return;
        }
        let (store, shared_host) = match self.session_store() {
            Ok(handles) => handles,
            Err(failure) => {
                eprintln!("Fleet request send has no session store: {failure}");
                return;
            }
        };
        let sent = store
            .transaction(move |scope| {
                Box::pin(async move {
                    let now = shared_host.now();
                    for destination in destinations {
                        scope.send_owed_requests(destination, now).await?;
                    }
                    Ok(())
                })
            })
            .await;
        if let Err(failure) = sent {
            eprintln!("Fleet request send failed: {failure}");
        }
    }

    /// Pushes whose item's `valid_until` passed before they were handed off: an
    /// item no push (possibly) reached expires, one that may have reached waits
    /// for the recipient's report; either way the sender is owed its status and
    /// the push is no longer owed. At most 100 per batch.
    pub(crate) async fn run_push_expiry(&self, now: SyncTime) {
        let ObjectWork::Fleet { host, io, .. } = &self.work else {
            return;
        };
        let subject = match self.fleet_subject().await {
            Ok(subject) => subject,
            Err(failure) => {
                eprintln!("Fleet push expiry has no subject: {failure}");
                return;
            }
        };
        crate::due_unit::run_due_unit(
            host.store().as_ref(),
            crate::due_unit::DueItem {
                subject,
                kind: AttemptKind::PushExpiry,
                now,
                attempt_id: io.new_id(),
            },
            move |scope| Box::pin(crate::central_mail_sink::expire_owed_pushes(scope, now)),
        )
        .await;
    }

    /// Resend requests that went unanswered past their retry time.
    pub(crate) async fn resend_due_requests(&self, now: SyncTime) {
        let (store, _) = match self.session_store() {
            Ok(handles) => handles,
            Err(failure) => {
                eprintln!("Fleet request resend has no session store: {failure}");
                return;
            }
        };
        let attached = self.request_destinations().await;
        let resent = store
            .transaction(move |scope| {
                Box::pin(async move {
                    for kind in [AttemptKind::Status, AttemptKind::Push] {
                        for subject in crate::work_attempts::due_subjects(scope, kind, now).await? {
                            scope
                                .resend_due_request(subject, kind, &attached, now)
                                .await?;
                        }
                    }
                    Ok(())
                })
            })
            .await;
        if let Err(failure) = resent {
            eprintln!("Fleet request resend failed: {failure}");
        }
    }
}
