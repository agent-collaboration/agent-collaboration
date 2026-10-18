use crate::{
    ConformanceClient, ConformanceResult, ControlRequest, ControlResult, HolderSession,
    holder_fixtures::*,
};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
pub(crate) struct ImportedProjectFixture {
    pub project: ProjectId,
    pub pages: Vec<ProjectImportPage>,
    pub summary: ProjectSummary,
}
impl ImportedProjectFixture {
    pub fn new() -> ConformanceResult<Self> {
        let project = project()?;
        let sql =
            "CREATE TABLE records(id INTEGER PRIMARY KEY, label TEXT NOT NULL UNIQUE)".to_owned();
        let schema = SchemaIdentity::new(
            1,
            schema_fingerprint(&[SchemaEntry {
                kind: SchemaObjectKind::Table,
                name: "records".into(),
                sql: sql.clone(),
            }])?,
        )?;
        let header = ImportSnapshotHeader {
            project: project.clone(),
            head: position(1, 3)?,
            schema,
            schema_steps: vec![sql],
        };
        let summary = ProjectSummary {
            status: "imported".into(),
            latest_record_at: time()?.0,
        };
        let data = (1..=3)
            .map(|id| {
                vec![
                    SqlValue::Integer(id),
                    SqlValue::Text(format!("imported-{id}")),
                ]
            })
            .collect::<Vec<_>>();
        let mut digest = ImportDigestBuilder::new(&header)?;
        for row in &data {
            digest.append_row("records", row)?;
        }
        let digest = digest.finish(&summary)?;
        let now = time()?.0;
        let pages = data
            .chunks(2)
            .enumerate()
            .map(|(index, rows)| ProjectImportPage {
                digest: digest.clone(),
                index: index as u64,
                page: SnapshotPage {
                    project: project.clone(),
                    position: header.head,
                    schema: header.schema.clone(),
                    schema_steps: header.schema_steps.clone(),
                    table: "records".into(),
                    rows: rows.to_vec(),
                    next: if index == 0 {
                        Some(SnapshotCursor {
                            position: header.head,
                            table: "records".into(),
                            after_key: vec![SqlValue::Integer(2)],
                        })
                    } else {
                        None
                    },
                    as_of: now,
                },
                final_summary: if index == 1 {
                    Some(summary.clone())
                } else {
                    None
                },
            })
            .collect();
        Ok(Self {
            project,
            pages,
            summary,
        })
    }
    pub fn digest(&self) -> ConformanceResult<ImportDigest> {
        Ok(self
            .pages
            .first()
            .ok_or("fixture first page")?
            .digest
            .clone())
    }
    pub fn unheld(&self) -> ConformanceResult<UnheldProject> {
        Ok(UnheldProject {
            project: self.project.clone(),
            import_epoch: epoch(1)?,
            imported_head: position(1, 3)?,
            schema_version: 1,
        })
    }
    pub async fn upload(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        for page in &self.pages {
            let expected = if page.final_summary.is_some() {
                ProjectImportAnswer::Imported {
                    head: position(1, 3)?,
                }
            } else {
                ProjectImportAnswer::Staged {
                    digest: page.digest.clone(),
                    index: page.index,
                }
            };
            equal(client.import_project(&self.project, page).await?, expected)?;
        }
        Ok(())
    }
    pub async fn state(&self, client: &ConformanceClient) -> ConformanceResult<CopyStatus> {
        match client
            .call(
                ObjectAddress::Copy(self.project.clone()),
                WireCall::Copy(CopyCall::CopyStatus {
                    project: self.project.clone(),
                }),
            )
            .await?
        {
            WireResult::Copy(CopyResult::CopyStatus(Some(state))) => Ok(state),
            result => Err(format!("import state {result:?}").into()),
        }
    }
    pub async fn snapshot(&self, client: &ConformanceClient) -> ConformanceResult<SessionAnswer> {
        match client
            .call(
                ObjectAddress::Copy(self.project.clone()),
                WireCall::Copy(CopyCall::SnapshotPage { cursor: None }),
            )
            .await?
        {
            WireResult::Copy(CopyResult::SnapshotPage(answer)) => Ok(answer),
            _ => Err("snapshot result".into()),
        }
    }
}
pub(crate) fn attempt() -> ConformanceResult<RequestId> {
    Ok(RequestId::new(uuid::Uuid::now_v7().to_string())?)
}
pub(crate) fn claim_request(project: &ProjectId, attempt: RequestId) -> SessionRequest {
    SessionRequest::Claim {
        project: project.clone(),
        attempt,
    }
}
pub(crate) async fn send_request(
    socket: &mut HolderSession,
    body: SessionRequest,
) -> ConformanceResult<FrameId> {
    let frame_id = FrameId::new(uuid::Uuid::now_v7().to_string())?;
    socket
        .send(&MachineFrame::Request {
            frame_id: frame_id.clone(),
            body,
        })
        .await?;
    Ok(frame_id)
}
pub(crate) async fn receive_answer(
    socket: &mut HolderSession,
    id: &FrameId,
) -> ConformanceResult<SessionAnswer> {
    loop {
        match socket.receive().await? {
            CentralFrame::Notice { .. } => {}
            CentralFrame::Answer { frame_id, body } if frame_id == *id => return Ok(body),
            frame => return Err(format!("claim frame {frame:?}").into()),
        }
    }
}
pub(crate) async fn directory_updates(
    client: &ConformanceClient,
) -> ConformanceResult<Vec<DirectoryUpdate>> {
    match client.control(ControlRequest::DirectoryUpdates).await? {
        ControlResult::DirectoryUpdates(updates) => Ok(updates),
        _ => Err("directory producer observation".into()),
    }
}
