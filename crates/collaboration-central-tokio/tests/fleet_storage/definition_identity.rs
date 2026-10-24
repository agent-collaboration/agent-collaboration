use super::{
    metadata::request,
    support::{Fixture, TestResult, bundle},
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_admission::{FleetOperation, FleetRequestResult, ProcessDefinitionInput};
use message_board::{ChangeOutcome, RequestDecision};
use sqlx_core::{query::query, row::Row};

#[tokio::test]
async fn separate_steps_of_one_operation_register_distinct_definition_records() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.import(&bundle()?).await?;
        let operation = durable_steps::Uuid::now_v7();
        for (name, step) in [
            ("first definition", "first_registration"),
            ("second definition", "second_registration"),
        ] {
            let definition = ProcessDefinitionInput {
                reference: message_board::ProcessDefinitionRef {
                    name: message_board::ResourceName::try_from(name.to_owned())?,
                    version: message_board::ResourceName::try_from("1".to_owned())?,
                    source_hash: message_board::CanonicalHash::try_from(
                        "de4937fcf3dfe26848cdb53fd284de510f628efa9e5f16e1044de92576b94abc"
                            .to_owned(),
                    )?,
                },
                source: "{\"initialStatus\":\"draft\",\"edges\":[]}".into(),
            };
            let request = request(FleetOperation::DefinitionRegister { definition })?;
            assert!(matches!(
                fixture.fleet_request_at(&request, operation, step).await?,
                FleetRequestResult::Decided(RequestDecision::Accepted {
                    change: ChangeOutcome::Changed,
                    ..
                })
            ));
        }
        let count = fixture
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    query::<sqlx_turso::Turso>("SELECT count(*) AS count FROM process_definitions")
                        .fetch_one(scope.executor())
                        .await
                        .map_err(StoreError::backend)?
                        .try_get::<i64, _>("count")
                        .map_err(StoreError::backend)
                })
            })
            .await?;
        assert_eq!(count, 2);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}
