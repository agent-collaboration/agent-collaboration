//! How the journey judges a case and reads back change records.
use super::ADMIN_OWNER;
use collaboration_central_conformance::ConformanceResult;
use serde_json::{Value, json};

pub(crate) fn require(
    condition: bool,
    case: &str,
    observed: &impl std::fmt::Debug,
) -> ConformanceResult<()> {
    if condition {
        println!("LINK CASE passed: {case}");
        Ok(())
    } else {
        Err(format!("LINK CASE failed: {case}: observed {observed:?}").into())
    }
}

pub(crate) fn summary(records: &Value) -> Vec<(String, String, String)> {
    records
        .get("records")
        .and_then(Value::as_array)
        .map(|records| {
            records
                .iter()
                .map(|record| {
                    let field = |name: &str| {
                        record
                            .get(name)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    (field("requestId"), field("kind"), field("targetId"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// How many workspace-created records name `workspace`.
pub(crate) fn created_records(records: &Value, workspace: &str) -> usize {
    summary(records)
        .iter()
        .filter(|(_, kind, target)| kind == "workspaceCreated" && target == workspace)
        .count()
}

/// Every record read back names exactly the origin its kind allows: setup and
/// renewal records the sign-in service alone (no person, no permission), a
/// created workspace the owner and the permission checked; each ID a UUIDv7.
pub(crate) fn origins_closed(records: &Value) -> bool {
    let Some(records) = records.get("records").and_then(Value::as_array) else {
        return false;
    };
    !records.is_empty()
        && records.iter().all(|record| {
            let id = record.get("id").and_then(Value::as_str).unwrap_or_default();
            let uuid_v7 = id.len() == 36
                && id.get(14..15) == Some("7")
                && id.chars().all(|character| {
                    character == '-'
                        || character.is_ascii_hexdigit() && !character.is_ascii_uppercase()
                });
            let expected = match record.get("kind").and_then(Value::as_str) {
                Some("organizationProvisioned" | "stampRenewed") => {
                    json!({"kind":"signInProvisioning"})
                }
                Some("workspaceCreated") => json!({"kind":"person","personId":ADMIN_OWNER,
                    "permissionChecked":"workspace:create"}),
                _ => return false,
            };
            uuid_v7 && record.get("origin") == Some(&expected)
        })
}
