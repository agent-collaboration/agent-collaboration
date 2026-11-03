//! Normal enrollment and Fleet-create/Register arrange the shared forwarding oracle.

use super::{
    creation_scenarios as creation, forwarded_post_oracles as oracle, forwarded_post_wire as wire,
};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_admitted_peer,
};
use durable_steps::Uuid;
use fleet_admission::FleetOperation;
use fleet_sync_protocol::{DatabaseId, PeerId, RequestId};
use message_board::{ActorOrigin, BoardId, Description, ResourceName, ServiceId, WorkspaceId};
use std::future::Future;

pub(super) const SENDER_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440476";
pub(super) const SENDER_PEER: &str = "019f0000-0000-7000-8000-000000000476";
pub(super) const SENDER_SESSION: &str = "019f0000-0000-7000-8000-000000004760";
pub(super) const SENDER_CREDENTIAL: &str = "forward sender";
pub(super) const HOLDER_SERVICE: &str = "550e8400-e29b-41d4-a716-446655440477";
pub(super) const HOLDER_PEER: &str = "019f0000-0000-7000-8000-000000000477";
pub(super) const HOLDER_CREDENTIAL: &str = "forward holder";

#[derive(Clone, Copy)]
pub(super) enum ForwardCase {
    OriginAndReply,
    Disconnected,
    Reconnected,
    Capacity,
    Deadline,
    ExactBoundary,
    AlarmEvent,
    TerminalRelease,
    RouteOverlap,
    OwnState,
}

pub(super) struct RegisteredForwardingPeers {
    pub database: DatabaseId,
    pub sender_socket: reqwest_websocket::WebSocket,
    pub holder_socket: reqwest_websocket::WebSocket,
}

pub(super) async fn registered_forwarding_peers(
    base: &str,
    now: Option<collaboration_central_core::RegistryTime>,
) -> ConformanceResult<RegisteredForwardingPeers> {
    let client = ConformanceClient::new(base)?;
    if let Some(now) = now {
        client
            .set_object_time(collaboration_central_core::ObjectAddress::Registry, now)
            .await?;
    }
    let sender = enroll_admitted_peer(
        &client,
        SENDER_SERVICE,
        SENDER_PEER,
        SENDER_CREDENTIAL,
        SENDER_SESSION,
    )
    .await?;
    let holder = enroll_admitted_peer(
        &client,
        HOLDER_SERVICE,
        HOLDER_PEER,
        HOLDER_CREDENTIAL,
        &Uuid::now_v7().to_string(),
    )
    .await?;
    let sender = sender.socket;
    let mut holder = Some(holder.socket);
    let arranged = async {
        if let Some(now) = now {
            for service in [SENDER_SERVICE, HOLDER_SERVICE] {
                client
                    .set_object_time(
                        collaboration_central_core::ObjectAddress::Connection(ServiceId::try_from(
                            service.to_owned(),
                        )?),
                        now,
                    )
                    .await?;
            }
        }
        let socket = holder.as_mut().ok_or("initial holder socket is absent")?;
        let database = register_database(socket, PeerId::try_from(HOLDER_PEER.to_owned())?).await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(database)
    }
    .await;
    match arranged {
        Ok(database) => Ok(RegisteredForwardingPeers {
            database,
            sender_socket: sender,
            holder_socket: holder.take().ok_or("registered holder socket absent")?,
        }),
        Err(error) => {
            let sender = wire::close(sender).await;
            let holder = match holder {
                Some(holder) => wire::close(holder).await,
                None => Ok(()),
            };
            finish_forwarding(Err(error), sender, holder)
        }
    }
}

async fn register_database(
    socket: &mut reqwest_websocket::WebSocket,
    peer: PeerId,
) -> ConformanceResult<DatabaseId> {
    let workspace = WorkspaceId::generate();
    let board = BoardId::generate();
    let database = DatabaseId::new(Uuid::now_v7())?;
    for operation in [
        FleetOperation::WorkspaceCreate {
            name: ResourceName::try_from(format!("forwarding workspace {}", workspace.as_str()))?,
            description: Description::try_from("normal forwarding composition".to_owned())?,
        },
        FleetOperation::BoardCreate {
            board: board.clone(),
            name: ResourceName::try_from("forwarding board".to_owned())?,
            description: Description::try_from("normal holder carrier".to_owned())?,
            board_kind: message_board::BoardKind::Area {},
        },
        creation::project(database, board)?,
    ] {
        creation::accepted(creation::metadata(socket, &workspace, operation).await?)?;
    }
    let registered =
        creation::register(socket, database, peer, RequestId::new(Uuid::now_v7())?).await?;
    creation::registered(&registered, peer)?;
    Ok(database)
}

pub(super) async fn run<'base, Reconnect, Reconnected>(
    base: &'base str,
    case: ForwardCase,
    mut reconnect: Reconnect,
) -> ConformanceResult<()>
where
    Reconnect: FnMut(
        &'base str,
        &'static str,
        &'static str,
        collaboration_central_core::RegistryTime,
    ) -> Reconnected,
    Reconnected: Future<Output = ConformanceResult<reqwest_websocket::WebSocket>>,
{
    let now = Some(collaboration_central_conformance::fixture_time()?);
    let RegisteredForwardingPeers {
        database,
        sender_socket: mut sender,
        holder_socket,
    } = registered_forwarding_peers(base, now).await?;
    let mut holder = Some(holder_socket);
    let proof = async {
        let socket = holder.as_mut().ok_or("initial holder socket is absent")?;
        match case {
            ForwardCase::Capacity => {
                super::forwarded_post_lifecycle_oracles::capacity(&mut sender, socket, database, |command| control(base, command)).await?;
                println!("FORWARD_CAPACITY_CASE sixteenth_reserved_before_dispatch_seventeenth_overloaded_terminal_frees_slot passed");
            }
            ForwardCase::Deadline => {
                super::forwarded_post_lifecycle_oracles::deadline(&mut sender, socket, database, |command| control(base, command)).await?;
                println!("FORWARD_DEADLINE_CASE original_deadline_unanswered_late_reply_cannot_answer_fresh_identity passed");
            }
            ForwardCase::ExactBoundary => {
                super::forwarded_post_deadline_boundaries::exact_boundary(&mut sender, socket, database, |command| control(base, command)).await?;
                println!("FORWARD_BOUNDARY_CASE pre_boundary_answer_exact_expiry_and_independent_fresh_correlation passed");
            }
            ForwardCase::TerminalRelease => {
                let sender_owned = register_database(&mut sender, PeerId::try_from(SENDER_PEER.to_owned())?).await?;
                super::forwarded_post_terminal_slots::terminal_release(&mut sender, socket, database, sender_owned,
                    |command| control(base, command)).await?;
                println!("FORWARD_TERMINAL_CASE sender_holds_not_held_answer_and_deadline_each_release_only_their_slot passed");
            }
            ForwardCase::RouteOverlap => {
                super::forwarded_post_state_cases::overlap(base,&mut sender,socket,database).await?;
                println!("FORWARD_OVERLAP_CASE completed_late_route_no_resurrection_or_fresh_reply_replacement passed");
            }
            ForwardCase::OwnState => {
                super::forwarded_post_state_cases::own_state(base,&mut sender,socket,database).await?;
                println!("FORWARD_STATE_CASE raw_owning_forward_content_and_state_absent_after_terminal passed");
            }
            ForwardCase::AlarmEvent => {
                super::forwarded_post_deadline_boundaries::alarm_event(base, &mut sender, socket, database, |command| control(base, command)).await?;
                println!("FORWARD_REAL_TIMER_CASE controlled_alarm_unanswered_full_capacity_and_fresh_correlation passed");
            }
            ForwardCase::OriginAndReply => {
                oracle::preserves_origin_and_opaque_reply(&mut sender, socket, database, ActorOrigin {
                    installation: ServiceId::try_from(SENDER_SERVICE.to_owned())?,
                    machine: SENDER_CREDENTIAL.to_owned().try_into()?,
                }).await?;
                println!("FORWARD_SESSION_CASE exact_verified_origin_opaque_request_reply_and_correlation passed");
            }
            ForwardCase::Disconnected => {
                wire::close(holder.take().ok_or("holder socket already closed")?).await?;
                oracle::disconnected_holder_has_registry_presence(&mut sender, database).await?;
                println!("FORWARD_SESSION_CASE disconnected_holder_reports_actual_registry_presence passed");
            }
            ForwardCase::Reconnected => {
                let outstanding = oracle::begin_reconnect(&mut sender, socket, database).await?;
                wire::close(holder.take().ok_or("holder socket already closed")?).await?;
                holder = Some(reconnect(base, HOLDER_SERVICE, HOLDER_CREDENTIAL,
                    now.ok_or("reconnect fixture has no pinned clock")?).await?);
                oracle::finish_reconnect(&mut sender, holder.as_mut().ok_or("replacement holder socket absent")?, outstanding).await?;
                println!("FORWARD_SESSION_CASE reconnected_verified_holder_answers_original_identity passed");
            }
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    let closed_sender = wire::close(sender).await;
    let closed_holder = match holder {
        Some(holder) => wire::close(holder).await,
        None => Ok(()),
    };
    finish_forwarding(proof, closed_sender, closed_holder)
}

fn finish_forwarding<Output>(
    proof: ConformanceResult<Output>,
    sender: ConformanceResult<()>,
    holder: ConformanceResult<()>,
) -> ConformanceResult<Output> {
    let mut failures = Vec::new();
    let output = match proof {
        Ok(output) => Some(output),
        Err(error) => {
            failures.push(format!("scenario: {error}"));
            None
        }
    };
    for (owner, result) in [("sender socket", sender), ("holder socket", holder)] {
        if let Err(error) = result {
            failures.push(format!("{owner}: {error}"));
        }
    }
    if failures.is_empty() {
        output.ok_or_else(|| "forwarding scenario omitted its output".into())
    } else {
        Err(failures.join("; ").into())
    }
}

pub(super) async fn control(
    base: &str,
    command: collaboration_central_conformance::RuntimeControl,
) -> ConformanceResult<collaboration_central_conformance::RuntimeFaultObservation> {
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let response = super::holder_writer_retry::owner_post(
        &http,
        base,
        "runtime",
        serde_json::to_value(collaboration_central_conformance::RuntimeControlRequest {
            address: collaboration_central_core::ObjectAddress::Connection(ServiceId::try_from(
                SENDER_SERVICE.to_owned(),
            )?),
            command,
        })?,
    )
    .await?;
    Ok(serde_json::from_value(response)?)
}
