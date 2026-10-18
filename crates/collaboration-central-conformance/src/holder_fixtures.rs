use crate::{ConformanceClient, ConformanceResult, HolderSession};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
pub(crate) fn check(condition: bool, message: &str) -> ConformanceResult<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}
pub(crate) fn equal<TValue: std::fmt::Debug + PartialEq>(
    actual: TValue,
    expected: TValue,
) -> ConformanceResult<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("actual {actual:?}; expected {expected:?}").into())
    }
}
pub(crate) fn epoch(value: u64) -> ConformanceResult<Epoch> {
    Ok(Epoch::new(value)?)
}
pub(crate) fn position(value: u64, commit: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(epoch(value)?, commit.into()))
}
pub(crate) fn project() -> ConformanceResult<ProjectId> {
    let project = ProjectId::new(uuid::Uuid::now_v7().to_string())?;
    println!("CENTRAL fresh fixture project={}", project.as_str());
    Ok(project)
}
pub(crate) fn time() -> ConformanceResult<RegistryTime> {
    Ok(RegistryTime(
        chrono::DateTime::parse_from_rfc3339("2026-10-12T12:00:00Z")?.with_timezone(&chrono::Utc),
    ))
}
pub(crate) fn schema_transaction() -> ReplicatedTransaction {
    ReplicatedTransaction::SchemaStep {
        commit_id: 1.into(),
        migration_version: 1,
        statements: vec![
            "CREATE TABLE records(id INTEGER PRIMARY KEY, label TEXT NOT NULL UNIQUE)".into(),
        ],
    }
}
pub(crate) fn row(id: i64, label: &str) -> RowChange {
    RowChange::Upsert {
        table: "records".into(),
        key: vec![SqlValue::Integer(id)],
        values: vec![SqlValue::Integer(id), SqlValue::Text(label.into())],
    }
}
pub(crate) fn rows(commit: u64, rows: Vec<RowChange>) -> ReplicatedTransaction {
    ReplicatedTransaction::Rows {
        commit_id: commit.into(),
        rows,
    }
}
pub(crate) fn piece(
    project: &ProjectId,
    from: ReplicationPosition,
    through: ReplicationPosition,
    transactions: Vec<ReplicatedTransaction>,
) -> ConformanceResult<Piece> {
    Ok(Piece::new(PieceContents {
        project: project.clone(),
        epoch: through.epoch,
        continues_from: from,
        through,
        transactions,
        part: None,
        summary: Some(ProjectSummary {
            status: "active".into(),
            latest_record_at: time()?.0,
        }),
    })?)
}
pub(crate) fn parts(project: &ProjectId, count: u32) -> ConformanceResult<Vec<Piece>> {
    let transaction = rows(
        4,
        (0..count)
            .map(|index| row(i64::from(index), &format!("part-{index}")))
            .collect(),
    );
    let digest = PieceDigest::of_transaction(&transaction)?;
    (0..count)
        .map(|index| {
            Ok(Piece::new(PieceContents {
                project: project.clone(),
                epoch: epoch(1)?,
                continues_from: position(1, 1)?,
                through: position(1, 4)?,
                transactions: vec![rows(
                    4,
                    vec![row(i64::from(index), &format!("part-{index}"))],
                )],
                part: Some(TransactionPart::new(
                    TransactionKey {
                        epoch: epoch(1)?,
                        commit_id: 4.into(),
                    },
                    index,
                    count,
                    digest.clone(),
                )?),
                summary: if index == count - 1 {
                    Some(ProjectSummary {
                        status: "active".into(),
                        latest_record_at: time()?.0,
                    })
                } else {
                    None
                },
            })?)
        })
        .collect()
}
pub(crate) struct HolderFixture {
    pub installation: InstallationId,
    pub token: String,
    pub session_id: SessionId,
}
impl HolderFixture {
    /// Enrolls a fresh installation locally. On staging, the label maps to one
    /// of the two installations the owner already enrolled through Access.
    pub async fn enroll(client: &ConformanceClient, label: &str) -> ConformanceResult<Self> {
        if let Some(staging) = client.staging_access() {
            let enrolled = staging
                .for_fixture(label)
                .ok_or("no temporary staging installation is configured")?;
            return Ok(Self {
                installation: enrolled.installation.clone(),
                token: enrolled.machine_credential.clone(),
                session_id: SessionId::new(uuid::Uuid::now_v7().to_string())?,
            });
        }
        let installation = InstallationId::new(uuid::Uuid::now_v7().to_string())?;
        let token = format!("conformance-{label}-{}", installation.as_str());
        use sha2::{Digest, Sha256};
        equal(
            client
                .admin(AdminRequest::EnrollMachine {
                    label: label.into(),
                    installation: installation.clone(),
                    access_client_id: crate::access_client_id(&token)?,
                    credential_hash: format!("{:x}", Sha256::digest(token.as_bytes())),
                    expires_at: time()?.0 + chrono::TimeDelta::days(30),
                })
                .await?,
            AdminResult::MachineEnrolled,
        )?;
        Ok(Self {
            installation,
            token,
            session_id: SessionId::new(uuid::Uuid::now_v7().to_string())?,
        })
    }
    pub async fn connect(
        &self,
        client: &ConformanceClient,
    ) -> ConformanceResult<(HolderSession, Vec<ProjectSessionState>)> {
        self.connect_supported(client, 10).await
    }
    pub async fn connect_supported(
        &self,
        client: &ConformanceClient,
        max_version: u64,
    ) -> ConformanceResult<(HolderSession, Vec<ProjectSessionState>)> {
        let mut session = client.connect(&self.token).await?;
        session
            .send(&MachineFrame::Hello {
                installation: self.installation.clone(),
                session_id: self.session_id.clone(),
                schema_support: SchemaSupport { max_version },
            })
            .await?;
        match session.receive().await? {
            CentralFrame::Welcome {
                session_id,
                projects,
            } if session_id == self.session_id => Ok((session, projects)),
            frame => Err(format!("expected Welcome, got {frame:?}").into()),
        }
    }
}
