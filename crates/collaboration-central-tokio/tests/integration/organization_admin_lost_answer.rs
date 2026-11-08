//! A person's admin answer lost after central commits: the caller sees a lost
//! answer, the store already holds the change, and after a restart the same
//! operation finds it: one workspace, one record, the original workspace.
#![cfg(feature = "conformance")]
use crate::organization_admin_link::{
    CREATE, ORGANIZATION, OWNER, OWNER_SUBJECT, Person, answer, counts, create_body, create_intent,
    fresh_request_id, seed_floors, start,
};
use crate::organization_stamps::{self as stamps, NOW_SECONDS, ProofKey, SignInKeys, link_post};
use collaboration_central_conformance::{
    ConformanceResult, JwksFixtureServer, RuntimeControl, RuntimeControlRequest,
};
use collaboration_central_core::ObjectAddress;
use serde_json::{Value, json};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare the lost and recovered answers and the cold counts while fixture errors propagate."
)]
#[tokio::test]
async fn a_lost_admin_answer_is_recovered_by_the_same_operation_after_restart()
-> ConformanceResult<()> {
    // Wrong builds caught: a lost reply that rolled the change back, or one
    // that a retry turns into a second workspace or record, or a retry whose
    // answer names another workspace.
    let root = tempfile::tempdir()?;
    let access = JwksFixtureServer::start().await?;
    let keys = SignInKeys::start().await?;
    let (server, socket) = start(root.path(), &access, &keys).await?;
    let provisioned = link_post(
        &socket,
        "/internal/organization-provisioning",
        &json!({"organizationId":ORGANIZATION,"organizationStatement":stamps::stamp(ORGANIZATION)?}),
    )
    .await?;
    server.shutdown().await?;
    seed_floors(root.path()).await?;
    let (server, socket) = start(root.path(), &access, &keys).await?;
    let owner = Person {
        id: OWNER,
        subject: OWNER_SUBJECT,
        revision: 4,
        permissions: vec!["workspace:create"],
        second_factor_at: NOW_SECONDS - 60,
        key: ProofKey::generate()?,
    };
    let operation = "019f0000-0000-7000-8000-00000000d3a1";
    let intent = create_intent(operation, "research");
    let body = create_body(operation, "research");
    reqwest::Client::builder()
        .no_proxy()
        .build()?
        .post(format!(
            "{}/admin/v1/conformance/runtime",
            server.base_url()
        ))
        .header(
            "Cf-Access-Jwt-Assertion",
            collaboration_central_conformance::owner_assertion()?,
        )
        .json(&RuntimeControlRequest {
            address: ObjectAddress::Fleet {
                organization: ORGANIZATION.to_owned().try_into()?,
            },
            command: RuntimeControl::DropNextOrganizationAnswer,
        })
        .send()
        .await?
        .error_for_status()?;
    let lost = answer(
        &socket,
        CREATE,
        &owner.headers(CREATE, &intent, &fresh_request_id())?,
        &body,
    )
    .await?;
    server.shutdown().await?;
    // Observed before any retry: the change committed behind the lost answer.
    let after_loss = counts(root.path()).await?;
    let (server, socket) = start(root.path(), &access, &keys).await?;
    let recovered = answer(
        &socket,
        CREATE,
        &owner.headers(CREATE, &intent, &fresh_request_id())?,
        &body,
    )
    .await?;
    server.shutdown().await?;
    let after_retry = counts(root.path()).await?;

    assert_eq!(provisioned.0, 200);
    assert_eq!(lost, (503, Value::Null), "the caller sees a lost answer");
    assert_eq!(
        after_loss,
        (1, 2, 1),
        "one workspace, setup + creation records and one marker committed behind the loss"
    );
    assert_eq!(
        (
            recovered.0,
            recovered.1.pointer("/result").cloned(),
            recovered.1.pointer("/alreadyExisted").cloned(),
            recovered.1.pointer("/workspace/workspaceId").cloned(),
        ),
        (
            200,
            Some(json!("applied")),
            Some(json!(true)),
            Some(json!(operation))
        ),
        "the same operation finds its workspace after the restart"
    );
    assert_eq!(
        after_retry,
        (1, 2, 2),
        "still one workspace and one creation record; the retry adds only its own marker"
    );
    keys.shutdown().await?;
    access.shutdown().await?;
    Ok(())
}
