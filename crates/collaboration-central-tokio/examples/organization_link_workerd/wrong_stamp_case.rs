//! The routing-bug model on workerd: organization A's Fleet object holds B's
//! genuinely signed stamp. After the restart every A-named call that needs the
//! stamp is refused and nothing stored changes.
use super::link_client::Caller;
use super::organization_stamps as stamps;
use super::record_checks::require;
use collaboration_central_conformance::ConformanceResult;
use serde_json::{Value, json};

const WRONG_A: &str = "019f0000-0000-7000-8000-00000000b3b0";
const WRONG_B: &str = "019f0000-0000-7000-8000-00000000b3b1";

/// Before the restart: provision A, then store B's statement in A's object.
/// Returns A's rows as the observer reads them after the damage.
pub(crate) async fn seed(caller: &Caller) -> ConformanceResult<Value> {
    let provisioned = caller.provision(WRONG_A, &stamps::stamp(WRONG_A)?).await?;
    let tampered = caller
        .post(
            "/public/admin/v1/conformance/organization-statement-tamper",
            &json!({"organization":WRONG_A,"statement":stamps::stamp(WRONG_B)?}),
        )
        .await?;
    let rows = caller.records(WRONG_A).await?;
    require(
        provisioned == 200
            && tampered.0 == 200
            && rows.get("statement") == Some(&json!(stamps::stamp(WRONG_B)?)),
        "A's Fleet object holds B's genuinely signed stamp",
        &(provisioned, tampered, rows.clone()),
    )?;
    Ok(rows)
}

/// After the restart: the A-named stamp-guarded read and setup are refused,
/// B's statement sent to A is refused, and A's rows are exactly as seeded.
pub(crate) async fn check(caller: &Caller, before: &Value) -> ConformanceResult<()> {
    let digest = caller.digest(WRONG_A).await?;
    let setup = caller.provision(WRONG_A, &stamps::stamp(WRONG_A)?).await?;
    let foreign = caller.provision(WRONG_A, &stamps::stamp(WRONG_B)?).await?;
    let after = caller.records(WRONG_A).await?;
    require(
        digest.0 == 503 && setup == 409 && foreign == 403 && &after == before,
        "a Fleet holding another organization's stamp refuses every stamped call and changes nothing",
        &(digest, setup, foreign, after),
    )
}
