use super::{
    metadata::request,
    support::{Fixture, TestResult, bundle},
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{FleetOperation, FleetRequestResult, ProcessDefinitionInput};
use message_board::{Record, RequestDecision};
use sqlx_core::{query::query, row::Row};

#[tokio::test]
async fn a_stored_definition_source_must_match_its_immutable_source_hash() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.import(&bundle()?).await?;
        let definition = ProcessDefinitionInput {
            reference: message_board::ProcessDefinitionRef {
                name: message_board::ResourceName::try_from("source validation".to_owned())?,
                version: message_board::ResourceName::try_from("1".to_owned())?,
                source_hash: message_board::CanonicalHash::try_from(
                    "de4937fcf3dfe26848cdb53fd284de510f628efa9e5f16e1044de92576b94abc".to_owned(),
                )?,
            },
            source: "{\"initialStatus\":\"draft\",\"edges\":[]}".into(),
        };
        let first = request(FleetOperation::DefinitionRegister {
            definition: definition.clone(),
        })?;
        assert!(matches!(
            fixture.fleet_request(&first).await?,
            FleetRequestResult::Decided(RequestDecision::Accepted { .. })
        ));
        fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    let row = query::<sqlx_turso::Turso>("SELECT payload FROM process_definitions")
                        .fetch_one(scope.executor())
                        .await
                        .map_err(StoreError::backend)?;
                    let payload: String = row.try_get("payload").map_err(StoreError::backend)?;
                    let mut record: Record =
                        serde_json::from_str(&payload).map_err(StoreError::backend)?;
                    let Record::ProcessDefinition(value) = &mut record else {
                        return Err(StoreError::Aborted {
                            reason: "missing definition fixture".into(),
                        });
                    };
                    // Keep both the valid JSON source and its parsed graph coherent,
                    // while deliberately violating the independently stored source hash.
                    value.source = "{\"initialStatus\":\"done\",\"edges\":[]}".into();
                    value.initial_status = message_board::StatusName::try_from("done".to_owned())
                        .map_err(StoreError::backend)?;
                    query::<sqlx_turso::Turso>("UPDATE process_definitions SET payload=?")
                        .bind(serde_json::to_string(&record).map_err(StoreError::backend)?)
                        .execute(scope.executor())
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
        let repeated = request(FleetOperation::DefinitionRegister { definition })?;
        assert!(
            fixture.fleet_request(&repeated).await.is_err(),
            "corrupt source must not become AlreadySo"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}
