//! Literal HTTP bodies are the oracle, independent of core's domain mapping.
use collaboration_central_admin_protocol::*;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
type TestResult = Result<(), Box<dyn std::error::Error>>;
const PROJECT: &str = "019f0000-0000-7000-8000-000000005881";
const INSTALLATION: &str = "550e8400-e29b-41d4-a716-446655440583";
const PEER: &str = "019f0000-0000-7000-8000-000000005583";
fn round_trip<T: Serialize + DeserializeOwned>(expected: Value) -> TestResult {
    let value: T = serde_json::from_value(expected.clone())?;
    let actual = serde_json::to_value(value)?;
    if actual != expected {
        return Err(format!("HTTP contract changed: expected {expected}, actual {actual}").into());
    }
    Ok(())
}
fn native_preview() -> Value {
    json!({"token":PROJECT,"fence":1,"acked":{"epoch":1,"sequence":7},"holder":PEER,"lastSeen":1900000000000_i64})
}
#[test]
fn preview_and_confirmation_keep_native_fields_and_distinct_presence() -> TestResult {
    round_trip::<OwnerPreviewSuccess>(
        json!({"preview":native_preview(),"holderInstallation":INSTALLATION,"lastSeen":1900000060000_i64}),
    )?;
    round_trip::<OwnerTakeover>(
        json!({"transferId":PROJECT,"toInstallation":INSTALLATION,"reason":"recover offline holder","preview":native_preview()}),
    )?;
    round_trip::<OwnerMove>(
        json!({"transferId":PROJECT,"toInstallation":INSTALLATION,"reason":"planned move"}),
    )?;
    round_trip::<OwnerRetirement>(
        json!({"requestId":PROJECT,"expectFence":1,"expectAcked":{"epoch":1,"sequence":7}}),
    )
}
#[test]
fn directory_reuses_canonical_identity_metadata_and_rfc3339() -> TestResult {
    round_trip::<CopyDirectoryPage>(json!({"projects":[{
        "project":PROJECT,"name":"Owner project","workspace":"019f0000-0000-7000-8000-000000005882",
        "board":"019f0000-0000-7000-8000-000000005883","boardArchive":"archived",
        "copySide":{"kind":"notYetReported"},
        "leaseSide":{"kind":"known","value":{"lease":{"kind":"retired","value":{"retired_at":"2030-03-17T17:46:40Z"}},"leaseVersion":{"epoch":1,"incarnation":1,"change":1},"asOf":"2030-03-17T17:46:40Z"}}
    }],"nextAfter":PROJECT,"asOf":"2030-03-17T17:47:40Z"}))
}
#[test]
fn holder_refusals_retain_board_names_native_schema_and_renewal() -> TestResult {
    for value in [
        json!({"kind":"registryEpochUnproven"}),
        json!({"kind":"schemaTooNew","value":{"store":{"version":4,"fingerprint":"abcd"},"supported":3}}),
        json!({"kind":"transferInProgress","value":{"transfer":PROJECT}}),
        json!({"kind":"holderUnreachable","value":{"lastRenewedAt":1900000000000_i64}}),
        json!({"kind":"holderUnreachable","value":{"lastSeen":null}}),
    ] {
        round_trip::<OwnerTransferAnswer>(json!({"kind":"refused","value":value}))?;
    }
    Ok(())
}
#[test]
fn health_stage_nullable_observations_and_retired_time_keep_encoding() -> TestResult {
    for stage in ["starting", "seeding", "following"] {
        round_trip::<CopyStage>(json!(stage))?;
        round_trip::<CopyHealthRead<Option<CopyStage>>>(json!({"kind":"known","value":stage}))?;
    }
    round_trip::<CopyHealthRead<Option<CopyStage>>>(json!({"kind":"known","value":null}))?;
    round_trip::<CopyHealthRead<Option<CopyStage>>>(
        json!({"kind":"unreadable","value":{"detail":"bad stored stage"}}),
    )?;
    round_trip::<CopyHealth>(json!({"kind":"retired","value":{"retiredAt":"2030-03-17T17:46:40Z"}}))
}
#[test]
fn migrate_repair_and_typed_http_failures_preserve_unknown_version() -> TestResult {
    for result in [
        json!({"kind":"migrated","corpus":"fleet-sync-receiver","from":null,"to":3}),
        json!({"kind":"current","corpus":"fleet-sync-receiver","version":3}),
        json!({"kind":"failed","corpus":"fleet-sync-receiver","version":null,"reason":"unproven history"}),
        json!({"kind":"retired"}),
    ] {
        round_trip::<CopyMigrationPage>(
            json!({"results":[{"project":PROJECT,"result":result}],"nextAfter":PROJECT}),
        )?;
    }
    round_trip::<CopyRepairAnswer>(json!({"kind":"repaired"}))?;
    for body in [
        json!({"kind":"objectUnavailable"}),
        json!({"kind":"retired"}),
        json!({"kind":"centralCopyFull"}),
        json!({"kind":"copyUnavailable","reason":{"kind":"migrationPending","value":{"version":3}}}),
    ] {
        round_trip::<OwnerOperationFailure>(body)?;
    }
    round_trip::<OwnerRetirementAnswer>(
        json!({"kind":"pending","value":{"requestId":PROJECT,"phase":"directory"}}),
    )
}
