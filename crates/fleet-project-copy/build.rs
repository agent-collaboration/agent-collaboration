#![forbid(unsafe_code)]

fn main() -> Result<(), fleet_sync_query_gen::GenerateError> {
    fleet_sync_query_gen::verify_manifest("checked-copy-queries.json")?;
    fleet_sync_query_gen::verify_manifest("checked-snapshot-queries.json")
}
