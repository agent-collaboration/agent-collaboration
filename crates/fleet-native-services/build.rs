fn main() -> Result<(), fleet_sync_query_gen::GenerateError> {
    if std::env::var_os("CARGO_FEATURE_TEST_SUPPORT").is_some() {
        fleet_sync_query_gen::verify_manifest("consumer-checked-queries.json")?;
    }
    Ok(())
}
