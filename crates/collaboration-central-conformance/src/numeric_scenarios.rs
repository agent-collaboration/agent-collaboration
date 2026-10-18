use crate::{ConformanceClient, ConformanceResult, holder_fixtures::*};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
/// The same scalar/key oracle must pass through both engines. In particular,
/// an INTEGER is not transported as a rounded JavaScript Number, and REAL 1.0
/// must not acquire an INTEGER tag while constructing a snapshot.
pub async fn run_numeric_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let scenario = NumericRestartScenario::prepare(client).await?;
    client.evict(scenario.address.clone()).await?;
    scenario.verify(client).await?;
    println!(
        "COPY exact i64 minimum/maximum/adjacent >2^53 keys and values, REAL 1.0, snapshot and receipt replay: passed"
    );
    Ok(())
}

pub struct NumericRestartScenario {
    address: ObjectAddress,
    holder: InstallationId,
    change: Piece,
    expected: Vec<Vec<SqlValue>>,
}
impl NumericRestartScenario {
    pub async fn prepare(client: &ConformanceClient) -> ConformanceResult<Self> {
        let project = project()?;
        let holder = InstallationId::new(uuid::Uuid::now_v7().to_string())?;
        let address = ObjectAddress::Copy(project.clone());
        client
            .call(
                address.clone(),
                WireCall::Copy(CopyCall::Initialize {
                    project: project.clone(),
                    epoch: epoch(1)?,
                    holder: holder.clone(),
                }),
            )
            .await?;
        let schema = ReplicatedTransaction::SchemaStep {
            commit_id: 1.into(),
            migration_version: 1,
            statements: vec![
                "CREATE TABLE scalar_records(record_key INTEGER PRIMARY KEY, record_value)".into(),
            ],
        };
        equal(
            client
                .call(
                    address.clone(),
                    WireCall::Copy(CopyCall::ApplyPiece {
                        piece: piece(&project, position(1, 0)?, position(1, 1)?, vec![schema])?,
                        sender: holder.clone(),
                    }),
                )
                .await?,
            WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Acked {
                through: position(1, 1)?,
            })),
        )?;
        let expected = vec![
            vec![SqlValue::Integer(i64::MIN), SqlValue::Integer(i64::MIN)],
            vec![SqlValue::Integer(2), SqlValue::Real(1.0)],
            vec![
                SqlValue::Integer(9_007_199_254_740_992),
                SqlValue::Integer(9_007_199_254_740_992),
            ],
            vec![
                SqlValue::Integer(9_007_199_254_740_993),
                SqlValue::Integer(9_007_199_254_740_993),
            ],
            vec![SqlValue::Integer(i64::MAX), SqlValue::Integer(i64::MAX)],
        ];
        let changes = expected
            .iter()
            .map(|values| {
                Ok(RowChange::Upsert {
                    table: "scalar_records".into(),
                    key: vec![values.first().ok_or("key fixture")?.clone()],
                    values: values.clone(),
                })
            })
            .collect::<ConformanceResult<Vec<_>>>()?;
        let change = piece(
            &project,
            position(1, 1)?,
            position(1, 2)?,
            vec![rows(2, changes)],
        )?;
        equal(
            client
                .call(
                    address.clone(),
                    WireCall::Copy(CopyCall::ApplyPiece {
                        piece: change.clone(),
                        sender: holder.clone(),
                    }),
                )
                .await?,
            WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Acked {
                through: position(1, 2)?,
            })),
        )?;
        Ok(Self {
            address,
            holder,
            change,
            expected,
        })
    }
    pub async fn verify(&self, client: &ConformanceClient) -> ConformanceResult<()> {
        match client
            .call(
                self.address.clone(),
                WireCall::Copy(CopyCall::SnapshotPage { cursor: None }),
            )
            .await?
        {
            WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(
                ReadPage::ProjectCopy(ProjectCopyRead::Page(page)),
            ))) => {
                equal(page.rows, self.expected.clone())?;
                equal(page.table.as_str(), "scalar_records")?;
                equal(page.position, position(1, 2)?)?;
            }
            _ => return Err("numeric snapshot Return".into()),
        }
        equal(
            client
                .call(
                    self.address.clone(),
                    WireCall::Copy(CopyCall::ApplyPiece {
                        piece: self.change.clone(),
                        sender: self.holder.clone(),
                    }),
                )
                .await?,
            WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Acked {
                through: position(1, 2)?,
            })),
        )?;
        Ok(())
    }
}
