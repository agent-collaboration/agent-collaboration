//! Stage the existing Fleet RPC seam, then restart and use its actual alarm.
use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, ControlResult,
    archive_runtime::{archive_request, snapshot},
    fleet_request_scenarios::envelope,
    holder_fixtures::*,
    notice_runtime::{claimed, retained},
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use message_board::Record;

#[derive(Clone, Copy, Debug)]
pub enum NoticeDeliveryLoss {
    Dispatch,
    Acknowledgement,
}

pub struct FleetOutboxRestartScenario {
    holder: HolderFixture,
    at: RegistryTime,
    retry: RegistryTime,
    before: (Vec<Record>, u64),
    loss: NoticeDeliveryLoss,
}

async fn step(
    client: &ConformanceClient,
    input: WireInput,
    now: RegistryTime,
) -> ConformanceResult<Vec<WireEffect>> {
    let ControlResult::Effects(effects) = client
        .control(ControlRequest::Step {
            address: ObjectAddress::Fleet,
            input: Box::new(input),
            now,
        })
        .await?
    else {
        return Err("Fleet step effects".into());
    };
    Ok(effects)
}
async fn alarm(client: &ConformanceClient, deadline: RegistryTime) -> ConformanceResult<()> {
    let ControlResult::Alarms(alarms) = client.control(ControlRequest::PendingAlarms).await? else {
        return Err("Fleet platform alarm".into());
    };
    check(
        alarms.contains(&(ObjectAddress::Fleet, deadline)),
        "exact actual Fleet platform deadline",
    )
}

impl FleetOutboxRestartScenario {
    pub async fn prepare(
        client: &ConformanceClient,
        loss: NoticeDeliveryLoss,
    ) -> ConformanceResult<Self> {
        let (holder, mut socket, identity, _, at) = claimed(client).await?;
        equal(retained(&mut socket, at).await?.len(), 1)?;
        equal(snapshot(&mut socket).await?.1, 3)?;
        let request = archive_request(&holder, &identity)?;
        let mutation_at = RegistryTime(at.0 + chrono::TimeDelta::seconds(30));
        let input = WireInput::Call {
            from: ObjectAddress::Connection(holder.installation.clone()),
            call_id: ObjectCallId::new(uuid::Uuid::now_v7().to_string())?,
            call: Box::new(WireCall::Fleet(FleetCall::Fleet {
                installation: holder.installation.clone(),
                request: envelope(&request)?,
            })),
        };
        let effects = step(client, input.clone(), mutation_at).await?;
        validate_call_returns(&input, &effects)?;
        check(
            effects.iter().all(|effect| {
                matches!(
                    effect,
                    WireEffect::Return { .. } | WireEffect::SetAlarm { .. }
                )
            }),
            "inbound mutation commits debt without a delivery dispatch",
        )?;
        let before = snapshot(&mut socket).await?;
        equal(before.1, 6)?;
        alarm(client, mutation_at).await?;
        // Future supplied step time keeps this event pending while the real
        // platform clock is pinned at `at`. Step commits the real plan but
        // holds all RPC effects for controlled loss.
        let offered = step(
            client,
            WireInput::Alarm {
                face: CallerKind::Fleet,
            },
            mutation_at,
        )
        .await?;
        let mut calls = Vec::new();
        for effect in offered {
            match effect {
                WireEffect::Dispatch {
                    call_id,
                    to: ObjectAddress::Connection(installation),
                    call: WireCall::Delivery(call @ DeliveryCall::DeliverNotice(_)),
                    carry: None,
                } => {
                    equal(installation.clone(), holder.installation.clone())?;
                    calls.push((call_id, installation, call));
                }
                WireEffect::SetAlarm { .. } => {}
                other => return Err(format!("unexpected Fleet alarm effect: {other:?}").into()),
            }
        }
        equal(calls.len(), 2)?;
        check(calls.iter().any(|(_,_,call)|matches!(call,DeliveryCall::DeliverNotice(NoticeKind::BoardArchiving {board}) if board.as_str()==identity.board.as_str())),"held board notice offered")?;
        check(calls.iter().any(|(_,_,call)|matches!(call,DeliveryCall::DeliverNotice(NoticeKind::FleetFeedAdvanced {through}) if through.clone().value()==6)),"registered reader hint offered")?;
        if matches!(loss, NoticeDeliveryLoss::Acknowledgement) {
            for (call_id, installation, call) in calls {
                let input = WireInput::Call {
                    from: ObjectAddress::Fleet,
                    call_id,
                    call: Box::new(WireCall::Delivery(call)),
                };
                let ControlResult::Effects(committed) = client
                    .control(ControlRequest::Step {
                        address: ObjectAddress::Connection(installation),
                        input: Box::new(input.clone()),
                        now: mutation_at,
                    })
                    .await?
                else {
                    return Err("actual stepped delivery transaction".into());
                };
                validate_call_returns(&input, &committed)?;
                check(
                    committed.iter().any(|effect| {
                        matches!(
                            effect,
                            WireEffect::Return {
                                result: WireResult::Delivery(DeliveryResult::NoticeDelivered),
                                ..
                            }
                        )
                    }),
                    "actual recipient commit produces its own ACK",
                )?;
            }
        }
        // Withhold results from Fleet. In ACK loss the actual recipient
        // commits; no synthetic acknowledgement exists.
        let retry = RegistryTime(mutation_at.0 + chrono::TimeDelta::seconds(5));
        alarm(client, retry).await?;
        equal(
            retained(&mut socket, at).await?.len(),
            if matches!(loss, NoticeDeliveryLoss::Acknowledgement) {
                3
            } else {
                1
            },
        )?;
        socket.close().await?;
        Ok(Self {
            holder,
            at,
            retry,
            before,
            loss,
        })
    }

    pub async fn verify(self, client: &ConformanceClient) -> ConformanceResult<()> {
        alarm(client, self.retry).await?; // before any clock/control can re-arm it
        let repaired = RegistryTime(self.retry.0 + chrono::TimeDelta::seconds(1));
        client.set_time(repaired).await?;
        let (mut socket, _) = self.holder.connect(client).await?;
        let notices = retained(&mut socket, repaired).await?;
        let copies = if matches!(self.loss, NoticeDeliveryLoss::Acknowledgement) {
            2
        } else {
            1
        };
        equal(notices.len(), 1 + copies * 2)?;
        equal(
            notices
                .iter()
                .filter(|notice| matches!(notice.kind, NoticeKind::BoardArchiving { .. }))
                .count(),
            copies,
        )?;
        equal(notices.iter().filter(|notice|matches!(&notice.kind,NoticeKind::FleetFeedAdvanced {through} if through.clone().value()==6)).count(),copies)?;
        equal(snapshot(&mut socket).await?, self.before.clone())?;
        alarm(
            client,
            RegistryTime(self.at.0 + chrono::TimeDelta::days(30)),
        )
        .await?;
        let later = RegistryTime(repaired.0 + chrono::TimeDelta::seconds(10));
        client.set_time(later).await?;
        equal(retained(&mut socket, later).await?, notices)?;
        equal(snapshot(&mut socket).await?, self.before)?;
        socket.close().await?;
        println!(
            "FLEET OUTBOX physical restart: {:?}, exact pre-clock retained retry alarm, real Fleet→delivery retries/ACKs, numbered offline hints, unchanged records/feed, retry debt clears and no later repeat: passed",
            self.loss
        );
        Ok(())
    }
}
