//! The Registry supplies forwarding provenance, holder presence and installation peers.
use crate::{
    CallerKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo, RegistryKind,
    caller_context::CallAuthority,
    forward_calls::{
        ForwardRoute, ForwardRouteRequest, ForwardRouteResult, InstallationPeerRegistry,
    },
    generated_enrollment,
    installation_peer_reads::{
        InstallationPeerDirectory, InstallationPeerPageRead, InstallationPeerPageRequest,
    },
    machine_enrollment::{EnrollmentIdentity, identity_from_stored_enrollment},
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
    registry_peer_lookup::service_for_peer,
    step_driver::{StepCallback, StepDriver, StepResult},
};
use durable_steps::{
    AtomicStore, BoxFuture, CallFailure, CallTarget, NotSentReason, StoreError,
    storage::TransactionScope,
};
use fleet_sync_protocol::{DatabaseId, PeerId};
use message_board::ServiceId;
use runtime_primitives::{Clock, SyncTime};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::convert::Infallible;

enum RegistryMachineRequest {
    Forward {
        source: ServiceId,
        request: ForwardRouteRequest,
    },
    Installation(ServiceId),
    InstallationPage(Option<PeerId>),
}
enum RegistryMachineReply {
    Forward(ForwardRouteResult),
    Installation(Option<PeerId>),
    InstallationPage(InstallationPeerPageRead),
    Refused,
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<RegistryKind, Host, Io>
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
    pub(crate) async fn registry_machine_read(
        &self,
        invocation: ObjectCallEnvelope,
    ) -> Result<Vec<u8>, CallFailure> {
        if invocation.source_face != CallerKind::Session
            || !matches!(invocation.authority, CallAuthority::Object)
        {
            return Err(refused());
        }
        let ObjectAddress::Connection(source) = invocation.source else {
            return Err(refused());
        };
        let request = match invocation.kind.as_str() {
            ForwardRoute::KIND => {
                let _: ForwardRoute =
                    serde_json::from_value(invocation.target).map_err(encode_refusal)?;
                RegistryMachineRequest::Forward {
                    source,
                    request: serde_json::from_value(invocation.request).map_err(encode_refusal)?,
                }
            }
            InstallationPeerRegistry::KIND => {
                let target: InstallationPeerRegistry =
                    serde_json::from_value(invocation.target).map_err(encode_refusal)?;
                let (): () = serde_json::from_value(invocation.request).map_err(encode_refusal)?;
                // Authentication happened at the session. This read has no
                // additional authority or revocation predicate (decision520).
                RegistryMachineRequest::Installation(target.installation)
            }
            InstallationPeerDirectory::KIND => {
                let _: InstallationPeerDirectory =
                    serde_json::from_value(invocation.target).map_err(encode_refusal)?;
                let request: InstallationPeerPageRequest =
                    serde_json::from_value(invocation.request).map_err(encode_refusal)?;
                RegistryMachineRequest::InstallationPage(request.after)
            }
            _ => return Err(refused()),
        };
        let ObjectWork::Registry {
            host, shared_host, ..
        } = &self.work
        else {
            return Err(refused());
        };
        let (reply, _) = StepDriver::new(host.clone())
            .run(RegistryMachineRead {
                request,
                now: shared_host.now(),
            })
            .await
            .map_err(|_| CallFailure::AnswerLost)?;
        match reply {
            RegistryMachineReply::Forward(answer) => serde_json::to_vec(&answer),
            RegistryMachineReply::Installation(peer) => serde_json::to_vec(&peer),
            RegistryMachineReply::InstallationPage(page) => serde_json::to_vec(&page),
            RegistryMachineReply::Refused => return Err(refused()),
        }
        .map_err(|_| CallFailure::AnswerLost)
    }
}

struct RegistryMachineRead {
    request: RegistryMachineRequest,
    now: SyncTime,
}
impl<Store: AtomicStore, Driver: Database> StepCallback<Store, Driver> for RegistryMachineRead
where
    for<'transaction> Store::Scope<'transaction>: TransactionScope<Database = Driver>,
    Driver::Arguments: IntoArguments<Driver>,
    for<'value> String: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'value> Option<String>: Type<Driver> + Encode<'value, Driver> + Decode<'value, Driver>,
    for<'row> &'row str: ColumnIndex<Driver::Row>,
{
    type Output = RegistryMachineReply;
    type Effect = Infallible;
    fn run<'scope, 'transaction>(
        self,
        scope: &'scope mut Store::Scope<'transaction>,
    ) -> BoxFuture<'scope, StepResult<Self::Output, Self::Effect>> {
        Box::pin(async move {
            let answer = match self.request {
                RegistryMachineRequest::InstallationPage(after) => {
                    RegistryMachineReply::InstallationPage(
                        installation_peer_page(scope, after).await?,
                    )
                }
                RegistryMachineRequest::Installation(service) => {
                    RegistryMachineReply::Installation(
                        enrollment(scope, EnrollmentKey::Service(service))
                            .await?
                            .map(|identity| identity.peer),
                    )
                }
                RegistryMachineRequest::Forward { source, request } => {
                    if service_for_peer(scope, request.session.peer)
                        .await?
                        .as_ref()
                        != Some(&source)
                    {
                        return Ok((RegistryMachineReply::Refused, Vec::new()));
                    }
                    let database = DatabaseId::try_from(request.project.as_str().to_owned())
                        .map_err(StoreError::backend)?;
                    let lease =
                        match fleet_sync_lease::lease_status(scope, database, self.now).await {
                            Ok(lease) => lease,
                            Err(fleet_sync_lease::LeaseError::Retired) => None,
                            Err(error) => return Err(StoreError::backend(error)),
                        };
                    let route = match lease {
                        None => ForwardRouteResult::NotHeld,
                        Some(lease) if lease.holder == request.session.peer => {
                            ForwardRouteResult::SenderHolds
                        }
                        Some(lease) => {
                            let sender = enrollment(scope, EnrollmentKey::Service(source))
                                .await?
                                .ok_or_else(|| corrupt("service_id"))?;
                            let holder = enrollment(scope, EnrollmentKey::Peer(lease.holder))
                                .await?
                                .ok_or_else(|| corrupt("peer_id"))?;
                            ForwardRouteResult::Route {
                                holder: holder.peer,
                                installation: holder.origin.installation,
                                origin: sender.origin,
                                holder_last_seen: holder.last_seen,
                            }
                        }
                    };
                    RegistryMachineReply::Forward(route)
                }
            };
            Ok((answer, Vec::new()))
        })
    }
}

async fn installation_peer_page<Scope: TransactionScope>(
    scope: &mut Scope,
    after: Option<PeerId>,
) -> Result<InstallationPeerPageRead, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    use board_sync_protocol::{InstallationPeerPage, MAX_INSTALLATION_PEERS_PAGE_ROWS};
    let limit = MAX_INSTALLATION_PEERS_PAGE_ROWS
        .checked_add(1)
        .ok_or_else(|| corrupt("page.limit"))?;
    let after_text = after.map(|peer| peer.to_string());
    let rows = generated_enrollment::read_installation_peer_page(
        scope,
        after_text.clone(),
        after_text,
        limit.to_string(),
    )
    .await
    .map_err(StoreError::backend)?;
    if after.is_some() && rows.is_empty() {
        return Ok(InstallationPeerPageRead::CursorInvalid);
    }
    let mut peers = Vec::with_capacity(rows.len());
    for row in rows {
        let service = ServiceId::try_from(row.service_id.ok_or_else(|| corrupt("service_id"))?)
            .map_err(|_| corrupt("service_id"))?;
        let peer_text = row.peer_id.ok_or_else(|| corrupt("peer_id"))?;
        let peer = PeerId::try_from(peer_text.clone()).map_err(|_| corrupt("peer_id"))?;
        if peer.to_string() != peer_text {
            return Err(corrupt("peer_id"));
        }
        peers.push((service, peer));
    }
    let next = if peers.len() > MAX_INSTALLATION_PEERS_PAGE_ROWS {
        peers.truncate(MAX_INSTALLATION_PEERS_PAGE_ROWS);
        peers.last().map(|(_, peer)| *peer)
    } else {
        None
    };
    let page = InstallationPeerPage::new(peers, next).map_err(|_| corrupt("page"))?;
    page.validate_after(after)
        .map_err(|_| corrupt("page.after"))?;
    Ok(InstallationPeerPageRead::Page(page))
}

enum EnrollmentKey {
    Service(ServiceId),
    Peer(PeerId),
}
async fn enrollment<Scope: TransactionScope>(
    scope: &mut Scope,
    key: EnrollmentKey,
) -> Result<Option<EnrollmentIdentity>, StoreError>
where
    <Scope::Database as Database>::Arguments: IntoArguments<Scope::Database>,
    for<'value> String:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'value> Option<String>:
        Type<Scope::Database> + Encode<'value, Scope::Database> + Decode<'value, Scope::Database>,
    for<'row> &'row str: ColumnIndex<<Scope::Database as Database>::Row>,
{
    let (service, peer, field) = match &key {
        EnrollmentKey::Service(service) => (Some(service.as_str().to_owned()), None, "service_id"),
        EnrollmentKey::Peer(peer) => (None, Some(peer.to_string()), "peer_id"),
    };
    let mut rows = generated_enrollment::read_enrollments(scope, service, None, None, peer)
        .await
        .map_err(StoreError::backend)?;
    if rows.len() > 1 {
        return Err(corrupt(field));
    }
    let Some(row) = rows.pop() else {
        return Ok(None);
    };
    let identity = identity_from_stored_enrollment(row)?;
    let exact = match key {
        EnrollmentKey::Service(service) => identity.origin.installation == service,
        EnrollmentKey::Peer(peer) => identity.peer == peer,
    };
    if !exact {
        return Err(corrupt(field));
    }
    Ok(Some(identity))
}
fn corrupt(field: &str) -> StoreError {
    StoreError::Corrupt {
        field: format!("machine_enrollments.{field}"),
        message: "missing or conflicting enrollment identity".into(),
    }
}
fn refused() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
