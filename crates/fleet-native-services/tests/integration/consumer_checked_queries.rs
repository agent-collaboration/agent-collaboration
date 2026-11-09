#[tokio::test]
async fn consumer_checked_queries_match_the_native_schema()
-> Result<(), fleet_sync_query_gen::GenerateError> {
    fleet_sync_query_gen::generate_manifest(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("consumer-checked-queries.json"),
        true,
    )
    .await
}
