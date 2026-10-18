use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, copy_scenarios::CopyFixture,
    holder_fixtures::*, session_scenarios::SessionFixture,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// Prepared state is retained by the native test process while the entire
/// workerd process and every object instance is stopped and recreated.
pub struct TransactionRestartScenario {
    copies: Vec<(ProjectId, ReplicationPosition, Vec<Vec<SqlValue>>)>,
}
impl TransactionRestartScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        client.set_time(time()?).await?;
        let sql_failure = CopyFixture::create(client).await?;
        check(
            sql_failure
                .apply(piece(
                    &sql_failure.project,
                    position(1, 1)?,
                    position(1, 2)?,
                    vec![rows(2, vec![row(1, "same"), row(2, "same")])],
                )?)
                .await
                .is_err(),
            "SQL constraint must reject transaction after first write",
        )?;
        let rust_failure = CopyFixture::create(client).await?;
        client
            .control(ControlRequest::CopyFull {
                address: rust_failure.address(),
                after_writes: 2,
            })
            .await?;
        equal(
            rust_failure
                .apply(piece(
                    &rust_failure.project,
                    position(1, 1)?,
                    position(1, 2)?,
                    vec![rows(2, vec![row(1, "first"), row(2, "second")])],
                )?)
                .await?,
            SessionAnswer::Refused(SessionRefusal::CentralCopyFull),
        )?;

        let fenced = CopyFixture::create(client).await?;
        let transfer = TransferId::new(uuid::Uuid::now_v7().to_string())?;
        equal(
            client
                .call(
                    fenced.address(),
                    WireCall::Copy(CopyCall::Fence {
                        holder: fenced.holder.clone(),
                        transfer_id: transfer,
                        new_epoch: epoch(2)?,
                        expect: FenceExpectation {
                            epoch: epoch(1)?,
                            acked: position(1, 1)?,
                        },
                        target_schema_max: 10,
                    }),
                )
                .await?,
            WireResult::Copy(CopyResult::Fence(FenceResult::Fenced {
                old_epoch: epoch(1)?,
                new_epoch: epoch(2)?,
                start: position(1, 1)?,
            })),
        )?;
        equal(
            fenced
                .apply(piece(
                    &fenced.project,
                    position(1, 1)?,
                    position(1, 2)?,
                    vec![rows(2, vec![row(1, "stale")])],
                )?)
                .await?,
            SessionAnswer::Refused(SessionRefusal::StaleEpoch {
                current_epoch: epoch(2)?,
                epoch_ended_at: position(1, 1)?,
            }),
        )?;
        equal(fenced.state().await?.fence, epoch(2)?)?;

        let concurrent = CopyFixture::create(client).await?;
        let all_rows = (0..300).map(|id| row(id, &format!("row-{id}"))).collect();
        let complete_rows: Vec<_> = (0..300)
            .map(|id| vec![SqlValue::Integer(id), SqlValue::Text(format!("row-{id}"))])
            .collect();
        let change = piece(
            &concurrent.project,
            position(1, 1)?,
            position(1, 2)?,
            vec![rows(2, all_rows)],
        )?;
        let concurrent_address = concurrent.address();
        let apply_finished = AtomicBool::new(false);
        let (applied, observed) = tokio::join!(
            async {
                let answer = concurrent.apply(change).await;
                apply_finished.store(true, Ordering::SeqCst);
                answer
            },
            observe_old_or_complete_new(
                client,
                &concurrent_address,
                &apply_finished,
                &complete_rows
            ),
        );
        equal(
            applied?,
            SessionAnswer::Acked {
                through: position(1, 2)?,
            },
        )?;
        observed?;

        let mut scenario = Self {
            copies: vec![
                (sql_failure.project, position(1, 1)?, Vec::new()),
                (rust_failure.project, position(1, 1)?, Vec::new()),
                (fenced.project.clone(), position(1, 1)?, Vec::new()),
                (concurrent.project, position(1, 2)?, complete_rows),
            ],
        };
        scenario.verify(client).await?;
        // The matching fenced epoch commits the row and its position together.
        equal(
            fenced
                .apply(piece(
                    &fenced.project,
                    position(1, 1)?,
                    position(2, 1)?,
                    vec![rows(1, vec![row(1, "matching")])],
                )?)
                .await?,
            SessionAnswer::Acked {
                through: position(2, 1)?,
            },
        )?;
        let state = fenced.state().await?;
        equal(state.fence, epoch(2)?)?;
        let saved = scenario.copies.get_mut(2).ok_or("fenced fixture")?;
        saved.1 = position(2, 1)?;
        saved.2 = vec![vec![
            SqlValue::Integer(1),
            SqlValue::Text("matching".into()),
        ]];
        scenario.verify(client).await?;
        Ok(scenario)
    }
    pub async fn verify(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        for (project, position, expected) in &self.copies {
            let read =
                read_every_page_at_one_position(client, &ObjectAddress::Copy(project.clone()))
                    .await?;
            equal(read.position, *position)?;
            equal(&read.rows, expected)?;
        }
        Ok(())
    }
}

struct SnapshotRead {
    position: ReplicationPosition,
    rows: Vec<Vec<SqlValue>>,
    page_sizes: Vec<usize>,
}

/// Reads every snapshot page at one copy position. The copy answers `Moved`
/// when its position changes between pages; the read then starts again.
async fn read_every_page_at_one_position(
    client: &ConformanceClient,
    address: &ObjectAddress,
) -> ConformanceResult<SnapshotRead> {
    'restart: loop {
        let mut cursor = None;
        let mut first_position = None;
        let mut rows = Vec::new();
        let mut page_sizes = Vec::new();
        loop {
            let page = match client
                .call(
                    address.clone(),
                    WireCall::Copy(CopyCall::SnapshotPage { cursor }),
                )
                .await?
            {
                WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(
                    ReadPage::ProjectCopy(ProjectCopyRead::Page(page)),
                ))) => page,
                WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(
                    ReadPage::ProjectCopy(ProjectCopyRead::Moved { .. }),
                ))) => continue 'restart,
                result => return Err(format!("snapshot page {result:?}").into()),
            };
            equal(page.table.as_str(), "records")?;
            equal(page.position, *first_position.get_or_insert(page.position))?;
            page_sizes.push(page.rows.len());
            rows.extend(page.rows);
            cursor = page.next;
            if cursor.is_none() {
                return Ok(SnapshotRead {
                    position: page.position,
                    rows,
                    page_sizes,
                });
            }
        }
    }
}

/// Reads the whole snapshot repeatedly while the apply is in flight, until a
/// read shows the applied position. Every read must hold exactly the old
/// contents at the old position or every new row at the new position, so a
/// partial state cannot pass on a full first page.
async fn observe_old_or_complete_new(
    client: &ConformanceClient,
    address: &ObjectAddress,
    apply_finished: &AtomicBool,
    complete_rows: &[Vec<SqlValue>],
) -> ConformanceResult<()> {
    let (old, new) = (position(1, 1)?, position(1, 2)?);
    loop {
        let finished_before_read = apply_finished.load(Ordering::SeqCst);
        let read = read_every_page_at_one_position(client, address).await?;
        if read.position == old && read.rows.is_empty() {
            check(
                !finished_before_read,
                "a read after the acknowledgement still sees the old position",
            )?;
            continue;
        }
        if read.position == new && read.rows == complete_rows {
            return Ok(());
        }
        return Err(format!(
            "another request saw part of the transaction: position {:?}, {} rows in pages {:?}",
            read.position,
            read.rows.len(),
            read.page_sizes
        )
        .into());
    }
}

pub struct TransferRestartScenario {
    project: ProjectId,
    old: HolderFixture,
    new: HolderFixture,
    takeover: AdminRequest,
}
impl TransferRestartScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        let mut fixture = SessionFixture::create(client).await?;
        let takeover = fixture.takeover(fixture.preview().await?)?;
        client.control(ControlRequest::InterruptAfterFence).await?;
        check(
            client.admin(takeover.clone()).await.is_err(),
            "fence interruption must leave unknown outcome",
        )?;
        let fenced = fixture.copy().await?;
        equal(fenced.fence, epoch(2)?)?;
        equal(fenced.start, position(1, 1)?)?;
        equal(fenced.holder, Some(fixture.new.installation.clone()))?;
        fixture.new_socket.close().await?;
        let (target, projects) = fixture.new.connect(client).await?;
        fixture.new_socket = target;
        check(
            !projects
                .iter()
                .any(|state| state.project == fixture.project),
            "C1 target Welcome must have no grant while the fence is committed",
        )?;
        let AdminRequest::Takeover {
            transfer_id,
            project,
            to_installation,
            reason,
            confirm_token,
        } = &takeover
        else {
            return Err("takeover fixture".into());
        };
        let competing = AdminRequest::Takeover {
            transfer_id: TransferId::new(uuid::Uuid::now_v7().to_string())?,
            project: project.clone(),
            to_installation: to_installation.clone(),
            reason: reason.clone(),
            confirm_token: confirm_token.clone(),
        };
        equal(
            client.admin(competing).await?,
            AdminResult::Refused(AdminRefusal::TransferInProgress {
                transfer_id: transfer_id.clone(),
            }),
        )?;
        equal(
            fixture
                .old_socket
                .request(SessionRequest::Piece(piece(
                    &fixture.project,
                    position(1, 1)?,
                    position(1, 2)?,
                    vec![rows(2, vec![row(9, "C1 stale")])],
                )?))
                .await?,
            SessionAnswer::Refused(SessionRefusal::StaleEpoch {
                current_epoch: epoch(2)?,
                epoch_ended_at: position(1, 1)?,
            }),
        )?;
        println!(
            "STAGING C1 prepared project={} transfer={}: committed fence2/start(1,1), target Welcome no lease, competing transfer names original, old holder stale; original not replayed",
            fixture.project.as_str(),
            transfer_id.as_str()
        );
        fixture.old_socket.close().await?;
        fixture.new_socket.close().await?;
        Ok(Self {
            project: fixture.project,
            old: fixture.old,
            new: fixture.new,
            takeover,
        })
    }
    pub async fn verify(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        client.set_time(time()?).await?;
        for _ in 0..2 {
            equal(
                client.admin(self.takeover.clone()).await?,
                AdminResult::Granted {
                    new_epoch: epoch(2)?,
                    start: position(1, 1)?,
                },
            )?;
        }
        let status = client
            .call(
                ObjectAddress::Copy(self.project.clone()),
                WireCall::Copy(CopyCall::CopyStatus {
                    project: self.project.clone(),
                }),
            )
            .await?;
        check(
            matches!(status,WireResult::Copy(CopyResult::CopyStatus(Some(state))) if state.fence==epoch(2)? && state.start==position(1,1)? && state.acked==position(1,1)?),
            "C2 identical transfer retained original fence/start after actual restart",
        )?;
        let (mut old, _) = self.old.connect(client).await?;
        equal(
            old.request(SessionRequest::Piece(piece(
                &self.project,
                position(1, 1)?,
                position(1, 2)?,
                vec![rows(2, vec![row(2, "stale tail")])],
            )?))
            .await?,
            SessionAnswer::Refused(SessionRefusal::StaleEpoch {
                current_epoch: epoch(2)?,
                epoch_ended_at: position(1, 1)?,
            }),
        )?;
        let (mut new, _) = self.new.connect(client).await?;
        equal(
            new.request(SessionRequest::Piece(piece(
                &self.project,
                position(1, 1)?,
                position(2, 1)?,
                vec![rows(1, vec![row(2, "new holder")])],
            )?))
            .await?,
            SessionAnswer::Acked {
                through: position(2, 1)?,
            },
        )?;
        old.close().await?;
        new.close().await?;
        Ok(())
    }
}
