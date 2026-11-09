//! Notices go out after the transaction that issued them commits.
//!
//! Producers only append to a machine's stream. After each Fleet event this
//! step compares the in-memory cursor of each machine the event appended to
//! (see `event_owed`), or whose push is unfinished, with what is committed
//! and sends the gap, at most 32 notices per machine per event. A connection that just opened
//! is caught up once its Hello has attached a session: from what its Router
//! acknowledged, or from the retained floor for a released Router. A Router
//! that negotiated NoticesApplied is owed its acknowledgment: while it is
//! behind, its notice_delivery row resends from what it applied on the 30 s
//! backoff; offline when the retry comes due, the row is parked until the
//! Router reconnects acknowledging. A released Router gets live pushes only
//! and keeps polling.
use crate::{
    FleetKind, ObjectPlatform, ObjectRuntime,
    notice_push::NoticeCursor,
    object_runtime::ObjectWork,
    work_attempts::{AttemptFailure, AttemptKind, ErrorClass},
};
use durable_steps::{AtomicStore, ReborrowScope, StoreError, storage::TransactionScope};
use fleet_protocol::{ConnectionId, PeerId};
use runtime_primitives::{Clock, IdSource, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};

/// A connection attached right now and whether its Router negotiated
/// NoticesApplied.
#[derive(Clone, Copy, Debug)]
struct Recipient {
    connection: ConnectionId,
    peer: PeerId,
    acknowledges: bool,
}

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
    async fn notice_recipients(&self) -> Vec<Recipient> {
        let ObjectWork::Fleet { host, .. } = &self.work else {
            return Vec::new();
        };
        let registrations = match host.registrations().await {
            Ok(registrations) => registrations,
            Err(failure) => {
                eprintln!("Fleet notice push could not read connections: {failure}");
                return Vec::new();
            }
        };
        let mut recipients = Vec::new();
        for (connection, registration) in registrations {
            let fleet_session::ConnectionRegistration::Attached { capabilities, .. } = registration
            else {
                continue;
            };
            let Ok(binding) = host.admission_binding(connection).await else {
                continue;
            };
            // The registration holds what was negotiated: offered and honored.
            recipients.push(Recipient {
                connection,
                peer: binding.peer,
                acknowledges: capabilities.is_some_and(|negotiated| negotiated.notices_applied),
            });
        }
        recipients
    }

    /// Give every machine's session the committed notices it hasn't been given.
    /// `publish_notice_to_peer` sends to an attached connection or keeps the
    /// notice in the session's transport log for a disconnected one.
    pub(crate) async fn push_notices(&self, appended: std::collections::BTreeSet<PeerId>) {
        let ObjectWork::Fleet {
            io, notice_cursors, ..
        } = &self.work
        else {
            return;
        };
        let attached = self.notice_recipients().await;
        let (store, shared_host) = match self.session_store() {
            Ok(handles) => handles,
            Err(failure) => {
                eprintln!("Fleet notice push has no session store: {failure}");
                return;
            }
        };
        let cursors = notice_cursors.clone();
        let seeded = notice_cursors.seeded();
        // Only machines this event appended to, plus unfinished pushes (a
        // connection waiting for its catch-up, a backlog over 32): an idle
        // event reads nothing.
        let mut candidates = appended;
        candidates.extend(notice_cursors.unfinished());
        if candidates.is_empty() {
            return;
        }
        let id = io.new_id();
        let pushed = store
            .transaction(move |scope| {
                Box::pin(async move {
                    let now = shared_host.now();
                    let mut pushed = Vec::new();
                    for peer in candidates {
                        let Some(progress) =
                            crate::machine_notice_progress::progress(scope, peer).await?
                        else {
                            continue;
                        };
                        let cursor = cursors.get(peer);
                        // A just-opened connection is caught up once its own Hello has
                        // attached a session. An older connection of the same machine
                        // that is still registered doesn't count.
                        let connected = match cursor {
                            Some(NoticeCursor::CatchUp(opened)) => {
                                let Some(recipient) = attached
                                    .iter()
                                    .find(|recipient| recipient.connection == opened)
                                else {
                                    continue;
                                };
                                Some(recipient)
                            }
                            _ => attached.iter().find(|recipient| recipient.peer == peer),
                        };
                        let acknowledges =
                            connected.is_some_and(|recipient| recipient.acknowledges);
                        // Caught up from what the Router acknowledged (or, released, from
                        // the retained floor).
                        let cursor = match (cursor, acknowledges) {
                            (Some(NoticeCursor::Sent(position)), _) => position,
                            (Some(NoticeCursor::CatchUp(_)) | None, true) => {
                                progress.applied.unwrap_or(0)
                            }
                            (Some(NoticeCursor::CatchUp(_)), false) => 0,
                            // Seeded: the stream began in this incarnation. Unseeded (the
                            // seed read failed): start at the head, a released Router polls.
                            (None, false) if seeded => 0,
                            (None, false) => progress.issued,
                        };
                        if cursor >= progress.issued {
                            pushed.push((peer, cursor, progress.issued));
                            continue;
                        }
                        let through = scope.publish_notices_after(peer, cursor, now).await?;
                        pushed.push((peer, through, progress.issued));
                        let owed = AttemptKind::NoticeDelivery;
                        if acknowledges && progress.behind() {
                            crate::work_attempts::owe(
                                scope,
                                id,
                                peer.as_uuid(),
                                owed,
                                now.after(crate::work_attempts::FIRST_RETRY),
                                now,
                            )
                            .await?;
                        } else if connected.is_some() && !acknowledges {
                            // A released Router is never resent to.
                            crate::work_attempts::succeeded(scope, peer.as_uuid(), owed).await?;
                        }
                    }
                    Ok(pushed)
                })
            })
            .await;
        match pushed {
            // Cursors move only once the sends are committed.
            Ok(pushed) => {
                for (peer, through, issued) in pushed {
                    notice_cursors.set(peer, through, issued);
                }
            }
            Err(failure) => eprintln!("Fleet notice push failed: {failure}"),
        }
    }

    /// Seed the in-memory cursors once per runtime from the committed heads.
    pub(crate) async fn seed_notice_cursors(&self) {
        let ObjectWork::Fleet { notice_cursors, .. } = &self.work else {
            return;
        };
        if notice_cursors.seeded() {
            return;
        }
        let (store, _) = match self.session_store() {
            Ok(handles) => handles,
            Err(failure) => {
                eprintln!("Fleet notice cursors have no session store: {failure}");
                return;
            }
        };
        let heads = store
            .transaction(|scope| {
                Box::pin(async move {
                    let mut heads = Vec::new();
                    for peer in crate::machine_notice_store::peers(scope).await? {
                        if let Some(progress) =
                            crate::machine_notice_progress::progress(scope, peer).await?
                        {
                            heads.push((peer, progress.issued));
                        }
                    }
                    Ok(heads)
                })
            })
            .await;
        match heads {
            Ok(heads) => notice_cursors.seed(heads),
            Err(failure) => eprintln!("Fleet notice cursors could not be seeded: {failure}"),
        }
    }

    /// Due notice_delivery rows: resend from what each Router acknowledged.
    pub(crate) async fn resend_notices(&self, now: SyncTime) {
        let ObjectWork::Fleet { io, .. } = &self.work else {
            return;
        };
        let recipients = self.notice_recipients().await;
        let (store, _) = match self.session_store() {
            Ok(handles) => handles,
            Err(failure) => {
                eprintln!("Fleet notice resend has no session store: {failure}");
                return;
            }
        };
        let id = io.new_id();
        let resent = store
            .transaction(move |scope| {
                Box::pin(async move {
                    let owed = AttemptKind::NoticeDelivery;
                    for subject in crate::work_attempts::due_subjects(scope, owed, now).await? {
                        let peer = PeerId::new(subject).map_err(StoreError::backend)?;
                        let negotiated = recipients
                            .iter()
                            .any(|recipient| recipient.peer == peer && recipient.acknowledges);
                        let progress =
                            crate::machine_notice_progress::progress(scope, peer).await?;
                        let Some(progress) = progress.filter(|progress| progress.behind()) else {
                            crate::work_attempts::succeeded(scope, subject, owed).await?;
                            continue;
                        };
                        if !negotiated {
                            if recipients.iter().any(|recipient| recipient.peer == peer) {
                                // Back as a released Router: never resent to.
                                crate::work_attempts::succeeded(scope, subject, owed).await?;
                            } else {
                                // Offline: parked, so it is no longer due (nor arms the
                                // alarm) until it reconnects acknowledging, which arms it.
                                crate::work_attempts::park(scope, subject, owed, now).await?;
                            }
                            continue;
                        }
                        let applied = progress.applied.unwrap_or(0);
                        let through = scope.publish_notices_after(peer, applied, now).await?;
                        crate::work_attempts::failed(
                            scope,
                            id,
                            subject,
                            owed,
                            &AttemptFailure {
                                class: ErrorClass::Unanswered,
                                message: format!(
                                    "resent notices {} through {through}; the Router has applied through {applied}",
                                    applied + 1
                                ),
                            },
                            now,
                        )
                        .await?;
                    }
                    Ok(())
                })
            })
            .await;
        if let Err(failure) = resent {
            eprintln!("Fleet notice resend failed: {failure}");
        }
    }
}
