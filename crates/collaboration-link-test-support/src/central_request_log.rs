//! Simulated host "central": accepts the link WebSocket, sends Welcome, answers
//! pings, and applies each write once by request id, like central's 24 h
//! request log: a replay of an applied id gets the first answer back.
use central_link_protocol::{
    DataFrame, FrameDirection, LinkFrame, LinkRequest, ReplyPayload, decode_text, encode_text,
};
use futures_util::{SinkExt, StreamExt};
use router_clock::RuntimeClock;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr},
    rc::Rc,
};
use tokio::task::JoinSet;
use tokio_tungstenite::{WebSocketStream, accept_async, tungstenite::Message};
use turmoil::net::{TcpListener, TcpStream};
use workspace_ids::MachineId;
use workspace_model::{ChangePosition, ItemRef, WorkspaceAnswer, WorkspaceReply, WorkspaceResult};

use super::simulated_clock::SimulatedClock;

pub const CENTRAL_HOST: &str = "central";
pub const LINK_PORT: u16 = 7400;

/// What central applied: each request id's created post and how many times
/// the write itself was applied (a replay does not apply it again).
#[derive(Default)]
pub struct CentralLog {
    pub applied: BTreeMap<String, AppliedWrite>,
    pub replays: usize,
    /// Each batch central received: when (simulated) and which writes.
    pub timeline: Vec<(std::time::Duration, Vec<String>)>,
}

pub struct AppliedWrite {
    pub post: String,
    pub times_applied: usize,
}

pub type SharedCentralLog = Rc<RefCell<CentralLog>>;

/// Host software for "central".
pub async fn serve_central(machine: MachineId, log: SharedCentralLog) -> turmoil::Result {
    let listener = TcpListener::bind((IpAddr::from(Ipv4Addr::UNSPECIFIED), LINK_PORT)).await?;
    let mut links = JoinSet::new();
    loop {
        tokio::select! {
            biased;
            Some(finished) = links.join_next(), if !links.is_empty() => finished?,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                links.spawn_local(serve_one_link(machine.clone(), Rc::clone(&log), stream));
            }
        }
    }
}

async fn serve_one_link(machine: MachineId, log: SharedCentralLog, stream: TcpStream) {
    let clock = SimulatedClock::for_host(CENTRAL_HOST);
    let Ok(mut socket) = accept_async(stream).await else {
        return;
    };
    let welcome = LinkFrame::Data(DataFrame::Welcome {
        machine_id: machine,
        central_time: clock.utc_now(),
    });
    if send(&mut socket, &welcome).await.is_err() {
        return;
    }
    while let Some(Ok(message)) = socket.next().await {
        let Message::Text(text) = message else {
            continue;
        };
        let answers = match decode_text(text.as_str(), FrameDirection::ToCentral) {
            Ok(LinkFrame::Ping) => vec![LinkFrame::Pong],
            Ok(LinkFrame::Data(DataFrame::Request { request })) => {
                vec![reply(&request, apply(&log, &request))]
            }
            Ok(LinkFrame::Data(DataFrame::Batch { requests })) => {
                let members = requests
                    .requests()
                    .iter()
                    .filter_map(|request| request.payload().write_request_id())
                    .map(|id| id.as_str().to_owned())
                    .collect();
                log.borrow_mut()
                    .timeline
                    .push((turmoil::elapsed(), members));
                requests
            }
            .requests()
            .iter()
            .map(|request| reply(request, apply(&log, request)))
            .collect(),
            Ok(_) | Err(_) => return,
        };
        for answer in answers {
            if send(&mut socket, &answer).await.is_err() {
                return;
            }
        }
    }
}

/// Applies a write once; a replay answers with the post created first.
fn apply(log: &SharedCentralLog, request: &LinkRequest) -> ReplyPayload {
    let Some(request_id) = request.payload().write_request_id() else {
        return ReplyPayload::Workspace(WorkspaceReply {
            result: WorkspaceResult::Conflict,
            actors: Vec::new(),
        });
    };
    let mut log = log.borrow_mut();
    let next_post = log.applied.len() + 1;
    let replayed = log.applied.contains_key(request_id.as_str());
    let entry = log
        .applied
        .entry(request_id.as_str().to_owned())
        .or_insert_with(|| AppliedWrite {
            post: format!("019a0000-0000-7000-8000-{next_post:012x}"),
            times_applied: 1,
        });
    let post = entry.post.clone();
    if replayed {
        log.replays += 1;
    }
    created(&post, u64::try_from(next_post).unwrap_or(0))
}

fn created(post: &str, position: u64) -> ReplyPayload {
    ReplyPayload::Workspace(WorkspaceReply {
        #[expect(clippy::unwrap_used, reason = "the stub mints valid UUIDv7 post ids")]
        result: WorkspaceResult::Done(WorkspaceAnswer::Created {
            item: ItemRef::Post(post.parse().unwrap()),
            position: ChangePosition::try_from(position).unwrap_or_default(),
        }),
        actors: Vec::new(),
    })
}

fn reply(request: &LinkRequest, result: ReplyPayload) -> LinkFrame {
    LinkFrame::Data(DataFrame::Reply {
        correlation_id: request.correlation_id().clone(),
        result: Box::new(result),
    })
}

async fn send(
    socket: &mut WebSocketStream<TcpStream>,
    frame: &LinkFrame,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = encode_text(frame, FrameDirection::ToRouter)?;
    socket.send(Message::text(text)).await?;
    Ok(())
}
