fn main() -> Result<(), fleet_sync_query_gen::GenerateError> {
    fleet_sync_query_gen::verify_manifest("bootstrap-queries.json")?;
    fleet_sync_query_gen::verify_manifest("migration-record-queries.json")?;
    fleet_sync_query_gen::verify_manifest("enrollment-queries.json")
}
