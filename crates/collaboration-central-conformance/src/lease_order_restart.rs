use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, ControlResult,
    directory_restart::read_entry,
    fleet_directory_fixture::import_identity,
    holder_fixtures::*,
    import_fixtures::{ImportedProjectFixture, attempt, claim_request},
    registry_rpc_driver::{begin, dispatch, future_time},
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use fleet_admission::DirectoryEntry;

pub struct LeaseOrderRestartScenario {
    fixture: ImportedProjectFixture,
    identity: DirectoryEntry,
    holder: HolderFixture,
    initial: DirectoryUpdate,
    at: RegistryTime,
}

impl LeaseOrderRestartScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        let at = future_time()?;
        client.set_time(at).await?;
        let fixture = ImportedProjectFixture::new()?;
        let identity = import_identity(client, &fixture.project).await?;
        fixture.upload(client).await?;
        let holder = HolderFixture::enroll(client, "lease-order holder").await?;
        let (mut socket, _) = holder.connect(client).await?;
        let attempt = attempt()?;
        equal(
            socket
                .request(claim_request(&fixture.project, attempt.clone()))
                .await?,
            SessionAnswer::Claimed {
                epoch: epoch(2)?,
                expires_at: at.0 + chrono::TimeDelta::hours(4),
                start: position(1, 3)?,
            },
        )?;
        // This reads the actual Fleet reservation disposition after the real
        // session -> registry -> Fleet/copy grant path, rather than a collector ACK.
        equal(
            client
                .call(
                    ObjectAddress::Fleet,
                    WireCall::Fleet(FleetCall::ReserveClaim {
                        project: fixture.project.clone(),
                        attempt: attempt.clone(),
                    }),
                )
                .await?,
            WireResult::Fleet(FleetResult::ClaimReserved(
                ReserveClaimResult::AlreadySettled {
                    outcome: ClaimReservationOutcome::Granted,
                },
            )),
        )?;
        let ControlResult::DirectoryUpdates(updates) =
            client.control(ControlRequest::DirectoryUpdates).await?
        else {
            return Err("actual directory offers".into());
        };
        let initial = updates.into_iter().find(|update| matches!(update, DirectoryUpdate::ClaimGranted { project, attempt: stored, .. } if project == &fixture.project && stored == &attempt)).ok_or("real ClaimGranted publication")?;
        socket.close().await?;
        println!(
            "LEASE prepare: real session claim installed Held and exact original reservation is Settled(Granted)"
        );
        Ok(Self {
            fixture,
            identity,
            holder,
            initial,
            at,
        })
    }

    pub async fn verify(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        let now = RegistryTime(self.at.0 + chrono::TimeDelta::seconds(1));
        client.set_time(now).await?;
        let (mut socket, _) = self.holder.connect(client).await?;
        let before = read_entry(&mut socket, &self.identity).await?;
        let effects = begin(
            client,
            RegistryCall::Renew {
                installation: self.holder.installation.clone(),
                attempt: attempt()?,
                project: self.fixture.project.clone(),
                epoch: epoch(2)?,
            },
            now,
        )
        .await?;
        check(effects.iter().any(|effect| matches!(effect, WireEffect::Return { result: WireResult::Registry(RegistryResult::Renew(SessionAnswer::Renewed { replayed: false, expires_at, .. })), .. } if *expires_at == now.0 + chrono::TimeDelta::hours(4))), "renewal decision survives the startup publication gate")?;
        let allocation = dispatch(&effects)?;
        let WireCall::Fleet(FleetCall::AdvanceLeaseIncarnation { at_least }) = allocation.call
        else {
            return Err("new physical Registry must send only Advance before publication".into());
        };
        let DirectoryUpdate::ClaimGranted {
            lease_version: old_version,
            ..
        } = &self.initial
        else {
            return Err("prepared grant".into());
        };
        equal(at_least, old_version.incarnation().value())?;
        equal(read_entry(&mut socket, &self.identity).await?, before)?;
        let answer = allocation.answer(client).await?;
        let WireResult::Fleet(FleetResult::LeaseIncarnationAdvanced(incarnation)) = answer else {
            return Err("real Fleet allocator result".into());
        };
        check(
            incarnation > old_version.incarnation(),
            "new run outranks the old one",
        )?;
        let publication = dispatch(&allocation.deliver(client, answer, now).await?)?;
        let WireCall::Fleet(FleetCall::DirectoryUpdate(DirectoryUpdate::Lease {
            lease_version,
            as_of,
            ..
        })) = &publication.call
        else {
            return Err("current lease publication after allocation".into());
        };
        equal(lease_version.incarnation(), incarnation)?;
        equal(lease_version.change(), 2)?;
        equal(*as_of, now)?;
        let applied = publication.answer(client).await?;
        equal(
            applied.clone(),
            WireResult::Fleet(FleetResult::LeaseUpdated(LeaseUpdateResult::Applied)),
        )?;
        publication.deliver(client, applied, now).await?;
        let current = read_entry(&mut socket, &self.identity).await?;
        check(
            matches!(&current.lease_side, fleet_admission::DirectorySide::Known(metadata) if metadata.as_of == now.0 && matches!(&metadata.lease, DirectoryLease::Held { expires_at, .. } if *expires_at == now.0 + chrono::TimeDelta::hours(4))),
            "public real Fleet projection carries the current renewal",
        )?;

        let WireResult::Fleet(FleetResult::LeaseIncarnationAdvanced(fence)) = client
            .call(
                ObjectAddress::Fleet,
                WireCall::Fleet(FleetCall::AdvanceLeaseIncarnation {
                    at_least: incarnation.value(),
                }),
            )
            .await?
        else {
            return Err("advance test witness".into());
        };
        // A fresh exact reservation makes Fenced ClaimGranted's applies-nothing
        // rule observable independently of the already-settled original claim.
        let pending = attempt()?;
        let reserve = WireCall::Fleet(FleetCall::ReserveClaim {
            project: self.fixture.project.clone(),
            attempt: pending.clone(),
        });
        equal(
            client.call(ObjectAddress::Fleet, reserve.clone()).await?,
            WireResult::Fleet(FleetResult::ClaimReserved(ReserveClaimResult::Reserved)),
        )?;
        let mut delayed = self.initial.clone();
        if let DirectoryUpdate::ClaimGranted { attempt, .. } = &mut delayed {
            *attempt = pending.clone();
        }
        equal(
            client
                .call(
                    ObjectAddress::Fleet,
                    WireCall::Fleet(FleetCall::DirectoryUpdate(delayed)),
                )
                .await?,
            WireResult::Fleet(FleetResult::LeaseUpdated(LeaseUpdateResult::Fenced {
                fence,
            })),
        )?;
        equal(
            client.call(ObjectAddress::Fleet, reserve).await?,
            WireResult::Fleet(FleetResult::ClaimReserved(ReserveClaimResult::Reserved)),
        )?;
        equal(read_entry(&mut socket, &self.identity).await?, current)?;
        equal(
            client
                .call(
                    ObjectAddress::Fleet,
                    WireCall::Fleet(FleetCall::ReleaseClaim {
                        project: self.fixture.project.clone(),
                        attempt: pending,
                    }),
                )
                .await?,
            WireResult::Fleet(FleetResult::ClaimReleased),
        )?;
        socket.close().await?;
        println!(
            "LEASE physical restart: authoritative renewal, Advance-only startup gate, real allocation then current projection; delayed below-fence grant changes no lease/active reservation; original real session grant settled exact attempt: passed"
        );
        Ok(())
    }
}
