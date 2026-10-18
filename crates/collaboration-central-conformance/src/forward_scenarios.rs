use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, HolderSession, holder_fixtures::*,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use std::collections::BTreeSet;
fn fixed_reply() -> BoardPayload {
    BoardPayload::new(
        serde_json::json!({"kind":"replied","fixture":"M4-holder-admission-stand-in","accepted":true}),
    )
}
fn post() -> ForwardedPost {
    ForwardedPost {
        kind: ForwardedPostKind::ThreadPost,
        body: BoardPayload::new(
            serde_json::json!({"requestId":"fixture-post-request","body":"forwarded post"}),
        ),
    }
}
async fn send_forward(
    socket: &mut HolderSession,
    project: &ProjectId,
) -> ConformanceResult<FrameId> {
    let frame_id = FrameId::new(uuid::Uuid::now_v7().to_string())?;
    socket
        .send(&MachineFrame::Request {
            frame_id: frame_id.clone(),
            body: SessionRequest::Forward {
                project: project.clone(),
                post: post(),
            },
        })
        .await?;
    Ok(frame_id)
}
async fn forward_frame(
    socket: &mut HolderSession,
) -> ConformanceResult<(ForwardId, InstallationId, String, ProjectId, ForwardedPost)> {
    loop {
        match socket.receive().await? {
            CentralFrame::Notice { .. } => continue,
            CentralFrame::Forwarded {
                forward_id,
                origin,
                origin_label,
                project,
                post,
            } => return Ok((forward_id, origin, origin_label, project, post)),
            frame => return Err(format!("expected Forwarded, got {frame:?}").into()),
        }
    }
}
async fn forward_answer(
    socket: &mut HolderSession,
    id: &FrameId,
) -> ConformanceResult<ForwardOutcome> {
    loop {
        match socket.receive().await? {
            CentralFrame::Notice { .. } => continue,
            CentralFrame::Answer {
                frame_id,
                body: SessionAnswer::Forwarded(outcome),
            } if &frame_id == id => return Ok(outcome),
            frame => {
                return Err(format!("expected correlated Forwarded answer, got {frame:?}").into());
            }
        }
    }
}
async fn reply(socket: &mut HolderSession, id: ForwardId) -> ConformanceResult<()> {
    socket
        .send(&MachineFrame::ForwardAnswer {
            forward_id: id,
            reply: fixed_reply(),
        })
        .await
}
pub async fn run_forward_roundtrip_and_race(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut f = crate::session_scenarios::SessionFixture::create(client).await?;
    let id = send_forward(&mut f.new_socket, &f.project).await?;
    let (forward_id, origin, label, project, forwarded) = forward_frame(&mut f.old_socket).await?;
    equal(origin, f.new.installation.clone())?;
    equal(label.as_str(), "new")?;
    equal(project, f.project.clone())?;
    equal(forwarded, post())?;
    reply(&mut f.old_socket, forward_id).await?;
    equal(
        forward_answer(&mut f.new_socket, &id).await?,
        ForwardOutcome::Answered {
            reply: fixed_reply(),
        },
    )?;
    println!(
        "FORWARD real clients: sender post -> holder Forwarded -> fixed holder ForwardAnswer -> correlated Answered; origin/label/project/payload exact"
    );
    // Fifteen delivered forwards remain unanswered, consuming fifteen slots.
    let mut requests = Vec::new();
    let mut deliveries = Vec::new();
    for _ in 0..15 {
        requests.push(send_forward(&mut f.new_socket, &f.project).await?);
        deliveries.push(forward_frame(&mut f.old_socket).await?.0);
    }
    client
        .control(ControlRequest::HoldNextDispatch {
            installation: f.new.installation.clone(),
        })
        .await?;
    let last = send_forward(&mut f.new_socket, &f.project).await?;
    client
        .control(ControlRequest::AwaitHeldDispatch {
            installation: f.new.installation.clone(),
        })
        .await?;
    let racing = send_forward(&mut f.new_socket, &f.project).await?;
    equal(
        forward_answer(&mut f.new_socket, &racing).await?,
        ForwardOutcome::Overloaded,
    )?;
    // Do not wait on the held lookup's answer before releasing it.
    client
        .control(ControlRequest::ResumeDispatch {
            installation: f.new.installation.clone(),
        })
        .await?;
    requests.push(last);
    deliveries.push(forward_frame(&mut f.old_socket).await?.0);
    for forward_id in deliveries {
        reply(&mut f.old_socket, forward_id).await?;
    }
    let mut expected = requests
        .into_iter()
        .map(|id| id.as_str().to_owned())
        .collect::<BTreeSet<_>>();
    while !expected.is_empty() {
        match f.new_socket.receive().await? {
            CentralFrame::Notice { .. } => {}
            CentralFrame::Answer {
                frame_id,
                body: SessionAnswer::Forwarded(ForwardOutcome::Answered { reply }),
            } => {
                equal(reply, fixed_reply())?;
                check(
                    expected.remove(frame_id.as_str()),
                    "one answer per reserved forward",
                )?;
            }
            frame => return Err(format!("race reply {frame:?}").into()),
        }
    }
    let fresh = send_forward(&mut f.new_socket, &f.project).await?;
    let fresh_delivery = forward_frame(&mut f.old_socket).await?.0;
    reply(&mut f.old_socket, fresh_delivery).await?;
    equal(
        forward_answer(&mut f.new_socket, &fresh).await?,
        ForwardOutcome::Answered {
            reply: fixed_reply(),
        },
    )?;
    println!(
        "FORWARD overloaded race: 15 awaiting + one committed Routing held at lookup, second concurrent request Overloaded; release -> exactly 16 answers; next forward admitted"
    );
    f.close().await
}
pub async fn run_forward_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    run_forward_roundtrip_and_race(client).await?;
    let mut f = crate::session_scenarios::SessionFixture::create(client).await?;
    for _ in 0..17 {
        equal(
            f.old_socket
                .request(SessionRequest::Forward {
                    project: f.project.clone(),
                    post: post(),
                })
                .await?,
            SessionAnswer::Forwarded(ForwardOutcome::SenderHolds),
        )?;
        equal(
            f.new_socket
                .request(SessionRequest::Forward {
                    project: ProjectId::new(uuid::Uuid::now_v7().to_string())?,
                    post: post(),
                })
                .await?,
            SessionAnswer::Forwarded(ForwardOutcome::NotHeld),
        )?;
    }
    let peer = HolderFixture::enroll(client, "unrelated").await?;
    let (mut peer_socket, _) = peer.connect(client).await?;
    let id = send_forward(&mut f.new_socket, &f.project).await?;
    let forward_id = forward_frame(&mut f.old_socket).await?.0;
    // The peer has no delivered-forward row; the fixed authentic reply still wins.
    peer_socket
        .send(&MachineFrame::ForwardAnswer {
            forward_id: forward_id.clone(),
            reply: BoardPayload::new(serde_json::json!({"forged":true})),
        })
        .await?;
    // A same-socket read is a barrier proving the unrelated answer was processed.
    let _ = peer_socket
        .request(SessionRequest::Read {
            scope: ReadScope::ProjectCopy {
                project: f.project.clone(),
            },
            after: None,
        })
        .await?;
    reply(&mut f.old_socket, forward_id).await?;
    equal(
        forward_answer(&mut f.new_socket, &id).await?,
        ForwardOutcome::Answered {
            reply: fixed_reply(),
        },
    )?;
    peer_socket.close().await?;
    // A takeover changes admission, not correlation authority for this in-flight post.
    let id = send_forward(&mut f.new_socket, &f.project).await?;
    let forward_id = forward_frame(&mut f.old_socket).await?.0;
    equal(
        client.admin(f.takeover(f.preview().await?)?).await?,
        AdminResult::Granted {
            new_epoch: epoch(2)?,
            start: position(1, 1)?,
        },
    )?;
    let not_writer = BoardPayload::new(serde_json::json!({"kind":"refused","reason":"notWriter"}));
    f.old_socket
        .send(&MachineFrame::ForwardAnswer {
            forward_id,
            reply: not_writer.clone(),
        })
        .await?;
    equal(
        forward_answer(&mut f.new_socket, &id).await?,
        ForwardOutcome::Answered { reply: not_writer },
    )?;
    f.close().await?;
    deadline_and_late_route(client).await?;
    late_delivery(client).await?;
    no_holder_connection(client).await?;
    println!(
        "FORWARD peer answer dropped, senderHolds/notHeld, holder change mid-forward, deadline, late route, sender runtime restart and holderUnreachable: passed; holder admission remains a fixed M4 stand-in"
    );
    Ok(())
}
async fn deadline_and_late_route(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut f = crate::session_scenarios::SessionFixture::create(client).await?;
    client
        .control(ControlRequest::HoldNextDispatch {
            installation: f.new.installation.clone(),
        })
        .await?;
    let expired = send_forward(&mut f.new_socket, &f.project).await?;
    client
        .control(ControlRequest::AwaitHeldDispatch {
            installation: f.new.installation.clone(),
        })
        .await?;
    client
        .evict(ObjectAddress::Connection(f.new.installation.clone()))
        .await?;
    client
        .set_time(RegistryTime(time()?.0 + chrono::TimeDelta::seconds(31)))
        .await?;
    equal(
        forward_answer(&mut f.new_socket, &expired).await?,
        ForwardOutcome::Unanswered,
    )?;
    client
        .control(ControlRequest::ResumeDispatch {
            installation: f.new.installation.clone(),
        })
        .await?;
    // The next real post is the next delivery: the late routing result did not deliver.
    let next = send_forward(&mut f.new_socket, &f.project).await?;
    let forward_id = forward_frame(&mut f.old_socket).await?.0;
    reply(&mut f.old_socket, forward_id).await?;
    equal(
        forward_answer(&mut f.new_socket, &next).await?,
        ForwardOutcome::Answered {
            reply: fixed_reply(),
        },
    )?;
    f.close().await
}
async fn no_holder_connection(client: &ConformanceClient) -> ConformanceResult<()> {
    let f = crate::session_scenarios::SessionFixture::create(client).await?;
    let project = f.project.clone();
    let holder = f.old;
    let sender = f.new;
    f.old_socket.close().await?;
    f.new_socket.close().await?;
    // Registry disconnection is a protocol barrier; the holder's close callback may
    // race HTTP, so explicitly drive the already-authorized disconnect fact.
    client
        .call(
            ObjectAddress::Connection(holder.installation.clone()),
            WireCall::Session(SessionCall::ConformanceClose {
                reason: CredentialRejection::Revoked,
            }),
        )
        .await?;
    let (mut socket, _) = sender.connect(client).await?;
    for _ in 0..17 {
        equal(
            socket
                .request(SessionRequest::Forward {
                    project: project.clone(),
                    post: post(),
                })
                .await?,
            SessionAnswer::Forwarded(ForwardOutcome::HolderUnreachable {
                last_seen: time()?.0,
            }),
        )?;
    }
    socket.close().await
}

async fn late_delivery(client: &ConformanceClient) -> ConformanceResult<()> {
    let mut f = crate::session_scenarios::SessionFixture::create(client).await?;
    client
        .control(ControlRequest::HoldNextForwardDelivery {
            installation: f.new.installation.clone(),
        })
        .await?;
    let expired = send_forward(&mut f.new_socket, &f.project).await?;
    client
        .control(ControlRequest::AwaitHeldForwardDelivery {
            installation: f.new.installation.clone(),
        })
        .await?;
    client
        .set_time(RegistryTime(time()?.0 + chrono::TimeDelta::seconds(31)))
        .await?;
    equal(
        forward_answer(&mut f.new_socket, &expired).await?,
        ForwardOutcome::Unanswered,
    )?;
    client
        .control(ControlRequest::ResumeForwardDelivery {
            installation: f.new.installation.clone(),
        })
        .await?;
    let next = send_forward(&mut f.new_socket, &f.project).await?;
    let forward_id = forward_frame(&mut f.old_socket).await?.0;
    reply(&mut f.old_socket, forward_id).await?;
    equal(
        forward_answer(&mut f.new_socket, &next).await?,
        ForwardOutcome::Answered {
            reply: fixed_reply(),
        },
    )?;
    f.close().await
}

pub struct ForwardRestartScenario {
    sender: InstallationId,
    frame_id: FrameId,
    _sender_socket: HolderSession,
    _holder_socket: HolderSession,
}
impl ForwardRestartScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        let mut f = crate::session_scenarios::SessionFixture::create(client).await?;
        client
            .control(ControlRequest::HoldNextDispatch {
                installation: f.new.installation.clone(),
            })
            .await?;
        let frame_id = send_forward(&mut f.new_socket, &f.project).await?;
        client
            .control(ControlRequest::AwaitHeldDispatch {
                installation: f.new.installation.clone(),
            })
            .await?;
        // Retain both sockets until the owned workerd process is stopped.
        // The active carry is then lost; only its reservation is durable.
        Ok(Self {
            sender: f.new.installation.clone(),
            frame_id,
            _sender_socket: f.new_socket,
            _holder_socket: f.old_socket,
        })
    }
    pub async fn verify(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        let result = client
            .control(ControlRequest::Step {
                address: ObjectAddress::Connection(self.sender.clone()),
                input: Box::new(WireInput::Alarm {
                    face: CallerKind::Session,
                }),
                now: RegistryTime(time()?.0 + chrono::TimeDelta::seconds(31)),
            })
            .await?;
        let crate::ControlResult::Effects(effects) = result else {
            return Err("forward restart effects".into());
        };
        check(effects.iter().any(|effect|matches!(effect,WireEffect::Send { frame:CentralFrame::Answer { frame_id,body:SessionAnswer::Forwarded(ForwardOutcome::Unanswered) },.. } if frame_id==&self.frame_id)),"reopened sender produces Unanswered without its lost call/post carry")?;
        println!(
            "FORWARD restart: fresh workerd executes the persisted sender deadline -> Unanswered for the original frame; payload/carry not recovered or queued"
        );
        Ok(())
    }
}
