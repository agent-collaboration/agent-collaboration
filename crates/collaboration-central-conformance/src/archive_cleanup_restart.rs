use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, ControlResult,
    archive_runtime::{archive_request, finalized_records, snapshot},
    fleet_directory_fixture::import_identity,
    fleet_request_scenarios::write,
    holder_fixtures::*,
    import_fixtures::{ImportedProjectFixture, attempt},
    registry_rpc_driver::{begin, dispatch, future_time},
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use fleet_admission::DirectoryEntry;
use message_board::{BoardState, Record, RequestDecision};

#[derive(Clone, Copy, Debug)]
pub enum CleanupLoss {
    Dispatch,
    Acknowledgement,
}

pub struct ArchiveCleanupRestartScenario {
    identity: DirectoryEntry,
    holder: HolderFixture,
    at: RegistryTime,
    before: (Vec<Record>, u64),
    settled_before_restart: Option<(Vec<Record>, u64)>,
}

impl ArchiveCleanupRestartScenario {
    pub async fn prepare(client: &ConformanceClient, loss: CleanupLoss) -> ConformanceResult<Self> {
        let at = future_time()?;
        client.set_time(at).await?;
        let fixture = ImportedProjectFixture::new()?;
        let identity = import_identity(client, &fixture.project).await?;
        fixture.upload(client).await?;
        let holder = HolderFixture::enroll(client, "cleanup claimant").await?;
        let other = HolderFixture::enroll(client, "cleanup existing holder").await?;
        let (mut socket, _) = holder.connect(client).await?;
        let (other_socket, _) = other.connect(client).await?;
        let attempt = attempt()?;
        let reserve = dispatch(
            &begin(
                client,
                RegistryCall::Claim {
                    attempt: attempt.clone(),
                    project: fixture.project.clone(),
                    claimant: holder.installation.clone(),
                },
                at,
            )
            .await?,
        )?;
        let reserved = reserve.answer(client).await?;
        equal(
            reserved.clone(),
            WireResult::Fleet(FleetResult::ClaimReserved(ReserveClaimResult::Reserved)),
        )?;
        let fence = dispatch(&reserve.deliver(client, reserved, at).await?)?;
        equal(
            client
                .call(
                    ObjectAddress::Copy(fixture.project.clone()),
                    WireCall::Copy(CopyCall::Fence {
                        transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
                        new_epoch: epoch(4)?,
                        holder: other.installation.clone(),
                        expect: FenceExpectation {
                            epoch: epoch(1)?,
                            acked: position(1, 3)?,
                        },
                        target_schema_max: 10,
                    }),
                )
                .await?,
            WireResult::Copy(CopyResult::Fence(FenceResult::Fenced {
                old_epoch: epoch(1)?,
                new_epoch: epoch(4)?,
                start: position(1, 3)?,
            })),
        )?;
        let status = dispatch(&fence.complete(client, at).await?)?;
        let refused = status.complete(client, at).await?;
        check(refused.iter().any(|effect| matches!(effect, WireEffect::Return { result: WireResult::Registry(RegistryResult::Claim(SessionAnswer::Refused(SessionRefusal::AlreadyHeld { holder }))), .. } if *holder == other.installation)), "real stale fence produces terminal refusal")?;
        let release = dispatch(&refused)?;
        check(
            matches!(&release.call, WireCall::Fleet(FleetCall::ReleaseClaim { project, attempt: stored }) if project == &fixture.project && stored == &attempt),
            "release retains the exact refused attempt",
        )?;
        check(
            matches!(
                write(&mut socket, &archive_request(&holder, &identity)?).await?,
                RequestDecision::Accepted { .. }
            ),
            "workspace archive accepted",
        )?;
        let before = snapshot(&mut socket).await?;
        check(before.0.iter().any(|record| matches!(record, Record::Board(board) if board.board_id == identity.board && board.state == BoardState::Archiving)), "undelivered release keeps Active reservation blocking archive")?;
        let ControlResult::Alarms(alarms) = client.control(ControlRequest::PendingAlarms).await?
        else {
            return Err("pending cleanup alarm".into());
        };
        check(
            alarms.iter().any(|(address, deadline)| {
                *address == ObjectAddress::Registry
                    && *deadline == RegistryTime(at.0 + chrono::TimeDelta::seconds(5))
            }),
            "real platform has persisted the cleanup deadline",
        )?;
        let settled_before_restart = match loss {
            CleanupLoss::Dispatch => None,
            CleanupLoss::Acknowledgement => {
                equal(
                    release.answer(client).await?,
                    WireResult::Fleet(FleetResult::ClaimReleased),
                )?;
                let expected = (
                    finalized_records(&before.0, &identity, before.1 + 1, at)?,
                    before.1 + 2,
                );
                equal(snapshot(&mut socket).await?, expected.clone())?;
                // Deliberately do not deliver the real acknowledgement to Registry.
                Some(expected)
            }
        };
        socket.close().await?;
        other_socket.close().await?;
        println!(
            "ARCHIVE cleanup prepare: real import/reservation/fence refusal, exact owed release, Active reservation blocks finalization, persisted platform alarm, loss={loss:?}"
        );
        Ok(Self {
            identity,
            holder,
            at,
            before,
            settled_before_restart,
        })
    }

    pub async fn verify(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        let after = RegistryTime(self.at.0 + chrono::TimeDelta::seconds(6));
        // This wakes the restored platform alarm, which executes the actual
        // Registry -> Fleet release and acknowledgement through the shell.
        client.set_time(after).await?;
        let (mut socket, _) = self.holder.connect(client).await?;
        let expected = match &self.settled_before_restart {
            Some(expected) => expected.clone(),
            None => (
                finalized_records(&self.before.0, &self.identity, self.before.1 + 1, after)?,
                self.before.1 + 2,
            ),
        };
        equal(snapshot(&mut socket).await?, expected.clone())?;
        client
            .set_time(RegistryTime(self.at.0 + chrono::TimeDelta::seconds(12)))
            .await?;
        equal(snapshot(&mut socket).await?, expected)?;
        let ControlResult::Alarms(alarms) = client.control(ControlRequest::PendingAlarms).await?
        else {
            return Err("post-cleanup alarms".into());
        };
        check(
            alarms
                .iter()
                .all(|(address, _)| *address != ObjectAddress::Registry),
            "acknowledged cleanup stops retrying",
        )?;
        socket.close().await?;
        println!(
            "ARCHIVE physical restart: actual platform alarm repairs owed release/lost acknowledgement, finalizes board/workspace once with exact actors/requestNone/time/feed and unchanged ledger, later alarm no-op: passed"
        );
        Ok(())
    }
}
