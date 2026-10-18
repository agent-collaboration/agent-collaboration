//! Fleet cursor/retention behavior through real sessions and a physical restart.
use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, ControlResult, HolderSession,
    fleet_request_scenarios::{fleet_records, write},
    holder_fixtures::*,
    registry_rpc_driver::future_time,
};
use collaboration_central_core::{AdminRequest, AdminResult, ObjectAddress, RegistryTime};
use collaboration_replication_protocol::{
    InstallationId, ReadCursor, ReadPage, ReadScope, SessionAnswer, SessionId, SessionRefusal,
    SessionRequest,
};
use fleet_admission::*;
use message_board::*;
use sha2::{Digest, Sha256};

const BOARDS: u64 = 12;

pub struct FleetFeedRestartScenario {
    holder: HolderFixture,
    workspace: WorkspaceId,
    at: RegistryTime,
    imported_head: u64,
}

pub struct FleetFeedAfterPruning {
    source: FleetFeedRestartScenario,
    head: u64,
    at: RegistryTime,
    records: (Vec<collaboration_replication_protocol::FleetPayload>, u64),
}

async fn read(
    socket: &mut HolderSession,
    after: u64,
    head: u64,
    now: RegistryTime,
) -> ConformanceResult<Vec<Record>> {
    let answer = socket
        .request(SessionRequest::Read {
            scope: ReadScope::FleetRecords,
            after: Some(ReadCursor::Fleet(after.into())),
        })
        .await?;
    let SessionAnswer::ReadPage(ReadPage::FleetRecords {
        records,
        position,
        as_of,
    }) = answer
    else {
        return Err(format!("expected incremental Fleet page: {answer:?}").into());
    };
    equal(position.value(), head)?;
    equal(as_of, now.0)?;
    let records = records
        .into_iter()
        .map(|record| serde_json::from_value::<Record>(record.into_value()))
        .collect::<Result<Vec<_>, _>>()?;
    equal(records.len() as u64, head - after)?;
    for (index, record) in records.iter().enumerate() {
        equal(
            record.envelope().position,
            RecordPosition::Fleet(FleetPosition::try_from(after + index as u64 + 1)?),
        )?;
    }
    Ok(records)
}

impl FleetFeedRestartScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        let at = future_time()?;
        client.set_time(at).await?;
        let installation = InstallationId::new(uuid::Uuid::now_v7().to_string())?;
        let token = format!("feed-conformance-{}", installation.as_str());
        equal(
            client
                .admin(AdminRequest::EnrollMachine {
                    label: "feed retention holder".into(),
                    installation: installation.clone(),
                    access_client_id: crate::access_client_id(&token)?,
                    credential_hash: format!("{:x}", Sha256::digest(token.as_bytes())),
                    expires_at: at.0 + chrono::TimeDelta::days(400),
                })
                .await?,
            AdminResult::MachineEnrolled,
        )?;
        let holder = HolderFixture {
            installation,
            token,
            session_id: SessionId::new(uuid::Uuid::now_v7().to_string())?,
        };
        let (mut socket, _) = holder.connect(client).await?;
        // This dedicated scenario requires fresh state. Register its first
        // feed reader after import so an owed hint cannot obscure the exact
        // retention alarm that the physical-restart assertion observes.
        let before = 0;
        let workspace = WorkspaceId::try_from(uuid::Uuid::now_v7().to_string())?;
        let old_time = at.0 - chrono::TimeDelta::days(3650);
        let mut records = vec![FleetImportRecord::Workspace {
            id: workspace.clone(),
            name: ResourceName::try_from(format!("feed {}", workspace.as_str()))?,
            description: Description::try_from("old source-time records".to_owned())?,
            repositories: vec![],
            created_at: old_time,
        }];
        for index in 0..BOARDS {
            records.push(FleetImportRecord::Board {
                id: BoardId::try_from(uuid::Uuid::now_v7().to_string())?,
                workspace: workspace.clone(),
                name: ResourceName::try_from(format!("feed board {index}"))?,
                description: Description::try_from("feed board".to_owned())?,
                state: ImportedBoardState::Active,
                created_at: old_time,
            });
        }
        equal(
            client
                .import_fleet(&FleetImportBundle {
                    provenance: ImportProvenance {
                        installation: ServiceId::try_from(holder.installation.as_str().to_owned())?,
                        machine: MachineLabel::try_from("import source".to_owned())?,
                    },
                    records,
                })
                .await?,
            FleetImportResult::Imported {
                counts: FleetImportCounts {
                    workspaces: 1,
                    boards: BOARDS,
                    directory_entries: 0,
                },
            },
        )?;
        let imported_head = before + BOARDS + 1;
        let imported = read(&mut socket, before, imported_head, at).await?;
        check(
            imported
                .iter()
                .all(|record| record.envelope().router_time == old_time),
            "import preserves original times",
        )?;
        let ControlResult::Alarms(alarms) = client.control(ControlRequest::PendingAlarms).await?
        else {
            return Err("pending Fleet alarm".into());
        };
        check(
            alarms.contains(&(
                ObjectAddress::Fleet,
                RegistryTime(at.0 + chrono::TimeDelta::days(30)),
            )),
            "actual persisted Fleet retention alarm",
        )?;
        socket.close().await?;
        Ok(Self {
            holder,
            workspace,
            at,
            imported_head,
        })
    }

    fn request(
        &self,
        workspace: WorkspaceId,
        operation: FleetOperation,
    ) -> ConformanceResult<FleetRequest> {
        Ok(FleetRequest {
            request_id: RequestId::try_from(uuid::Uuid::now_v7().to_string())?,
            workspace,
            actor: RecordedActor {
                actor: ActorKind::Declared(Identity::Human {
                    human_id: HumanId::try_from("feed owner".to_owned())?,
                }),
                installation: ServiceId::try_from(self.holder.installation.as_str().to_owned())?,
                machine: MachineLabel::try_from("feed machine".to_owned())?,
            },
            acting_for: None,
            resubmits: None,
            protocol_version: ProtocolVersion::CURRENT,
            operation,
        })
    }

    pub async fn until_pruned(
        self,
        client: &ConformanceClient,
    ) -> ConformanceResult<FleetFeedAfterPruning> {
        // Observe the platform alarm before SetTime can refresh/reconstruct it.
        let ControlResult::Alarms(alarms) = client.control(ControlRequest::PendingAlarms).await?
        else {
            return Err("restarted Fleet alarm".into());
        };
        check(
            alarms.contains(&(
                ObjectAddress::Fleet,
                RegistryTime(self.at.0 + chrono::TimeDelta::days(30)),
            )),
            "exact retention alarm survived physical restart before clock update",
        )?;
        let later = RegistryTime(self.at.0 + chrono::TimeDelta::days(29));
        client.set_time(later).await?;
        let (mut socket, _) = self.holder.connect(client).await?;
        let imported = read(
            &mut socket,
            self.imported_head - BOARDS - 1,
            self.imported_head,
            later,
        )
        .await?;
        check(
            imported.iter().all(|record| {
                record.envelope().router_time == self.at.0 - chrono::TimeDelta::days(3650)
            }),
            "ten-year-old records retained29 days after central append",
        )?;
        check(
            matches!(
                write(
                    &mut socket,
                    &self.request(self.workspace.clone(), FleetOperation::WorkspaceArchive {})?
                )
                .await?,
                RequestDecision::Accepted { .. }
            ),
            "archive accepted",
        )?;
        let archived_head = self.imported_head + BOARDS + 2;
        let archived = read(&mut socket, self.imported_head, archived_head, later).await?;
        equal(archived.iter().filter(|record| matches!(record, Record::Board(board) if board.state == BoardState::Archived)).count() as u64, BOARDS)?;
        check(archived.iter().any(|record| matches!(record, Record::Workspace(workspace) if workspace.state == WorkspaceState::Archived)), "workspace archived")?;
        check(
            matches!(archived.last(), Some(Record::Request(_))),
            "one independent ledger entry",
        )?;
        let expired = RegistryTime(self.at.0 + chrono::TimeDelta::days(31));
        client.set_time(expired).await?;
        equal(
            socket
                .request(SessionRequest::Read {
                    scope: ReadScope::FleetRecords,
                    after: Some(ReadCursor::Fleet((self.imported_head - 1).into())),
                })
                .await?,
            SessionAnswer::Refused(SessionRefusal::CursorExpired),
        )?;
        equal(
            read(&mut socket, self.imported_head, archived_head, expired).await?,
            archived,
        )?;
        let full_before = fleet_records(&mut socket).await?;
        let all_expired = RegistryTime(self.at.0 + chrono::TimeDelta::days(61));
        client.set_time(all_expired).await?;
        check(
            read(&mut socket, archived_head, archived_head, all_expired)
                .await?
                .is_empty(),
            "prune-all preserves old high-water cursor",
        )?;
        equal(fleet_records(&mut socket).await?, full_before.clone())?;
        socket.close().await?;
        Ok(FleetFeedAfterPruning {
            source: self,
            head: archived_head,
            at: all_expired,
            records: full_before,
        })
    }
}

impl FleetFeedAfterPruning {
    #[must_use]
    pub const fn retained_head(&self) -> u64 {
        self.head
    }

    pub async fn append(self, client: &ConformanceClient) -> ConformanceResult<()> {
        // Establish authentication before advancing a fresh server's clock
        // beyond the local fixture assertion's expiry.
        let (mut socket, _) = self.source.holder.connect(client).await?;
        client.set_time(self.at).await?;
        check(
            read(&mut socket, self.head, self.head, self.at)
                .await?
                .is_empty(),
            "old-head cursor remains valid before next append",
        )?;
        equal(fleet_records(&mut socket).await?, self.records)?;
        let workspace = WorkspaceId::try_from(uuid::Uuid::now_v7().to_string())?;
        check(
            matches!(
                write(
                    &mut socket,
                    &self.source.request(
                        workspace.clone(),
                        FleetOperation::WorkspaceCreate {
                            name: ResourceName::try_from(format!(
                                "post-prune {}",
                                workspace.as_str()
                            ))?,
                            description: Description::try_from(
                                "new after every feed row expired".to_owned()
                            )?,
                        }
                    )?
                )
                .await?,
                RequestDecision::Accepted { .. }
            ),
            "post-prune append accepted",
        )?;
        let appended = read(&mut socket, self.head, self.head + 2, self.at).await?;
        check(
            matches!(appended.first(), Some(Record::Workspace(record)) if record.workspace_id == workspace),
            "old-head reader receives new record above old high-water",
        )?;
        socket.close().await?;
        println!(
            "FLEET FEED physical restart: exact pre-clock platform alarm, K import/N+2 archive contiguous positions, old record time retained, central-time pruning, cursor boundary/full fallback, prune-all high-water and subsequent old-cursor catch-up: passed"
        );
        Ok(())
    }
}
