use super::support::*;
use central_storage::{
    AlarmUpdate, BatchOutcome, ConstraintKind, GuardedStatement, RowExpectation, Statement,
    StatementBatch,
};
use serde_json::json;
use workspace_model as m;

async fn generic(f: &Fixture, request: &str, name: &str) -> Result<m::WorkspaceReply> {
    Ok(f.host
        .request(
            &caller()?,
            m::WorkspaceRequest {
                request_id: Some(request.to_owned().try_into()?),
                workspace_id: None,
                operation: m::WorkspaceOperation::CreateWorkspace {
                    name: name.to_owned().try_into()?,
                },
            },
        )
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The same real directory index governs generic and signed admin create without fabricated origin rights."
)]
#[tokio::test]
async fn generic_create_defaults_description_and_shares_admin_name_uniqueness_and_replay() -> Result
{
    let f = Fixture::new().await?;
    f.ready().await?;
    f.seed_machine().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let operation = "019f0000-0000-7000-8000-00000000d791";
    assert_eq!(
        create(&f, &owner, operation, "Admin name", "admin description")
            .await?
            .status,
        200
    );
    let request = "019f0000-0000-7000-8000-00000000d792";
    let created = generic(&f, request, "Generic name").await?;
    let m::WorkspaceResult::Done(m::WorkspaceAnswer::WorkspaceCreated { workspace }) =
        &created.result
    else {
        return Err("generic create refused".into());
    };
    assert_ne!(
        workspace.workspace_id.as_str(),
        request,
        "generic workspace ID remains a fresh minted identity"
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT name,description,state FROM workspaces WHERE workspace_id=?",
            vec![text(&workspace.workspace_id)]
        )
        .await?,
        vec![vec![text("Generic name"), text(""), text("active")]]
    );
    let directory_before = f
        .rows(A, "SELECT * FROM workspaces ORDER BY workspace_id", vec![])
        .await?;
    let audit_before = f.snapshot().await?.1;
    let actors_before = f
        .rows(
            A,
            "SELECT * FROM actor_identities ORDER BY actor_identity_id",
            vec![],
        )
        .await?;
    let before = all_effects(&f).await?;
    assert_eq!(generic(&f, request, "Generic name").await?, created);
    assert!(
        all_effects(&f).await? == before,
        "generic replay rewrote rows"
    );
    assert_eq!(
        generic(&f, request, "Changed operation").await?.result,
        m::WorkspaceResult::Refused(m::RefusalReason::RequestIdReused)
    );
    assert!(
        all_effects(&f).await? == before,
        "changed request ID wrote rows"
    );
    for (request, name) in [
        ("019f0000-0000-7000-8000-00000000d793", "generic NAME"),
        ("019f0000-0000-7000-8000-00000000d794", " ADMIN NAME "),
    ] {
        let normalized: m::ResourceName = name.to_owned().try_into()?;
        let refused = generic(&f, request, name).await?;
        assert_eq!(
            refused.result,
            m::WorkspaceResult::Refused(m::RefusalReason::WorkspaceNameInUse { name: normalized })
        );
        let after = all_effects(&f).await?;
        assert_eq!(generic(&f, request, name).await?, refused);
        assert!(
            all_effects(&f).await? == after,
            "duplicate-name refusal was not retained exactly"
        );
    }
    let duplicate = create(
        &f,
        &owner,
        "019f0000-0000-7000-8000-00000000d795",
        "GENERIC NAME",
        "different description",
    )
    .await?;
    assert_eq!(duplicate.status, 200);
    assert_eq!(
        duplicate.value()?,
        json!({"result":"refused","reason":"nameInUse"})
    );
    assert_eq!(f.rows(A,"SELECT (SELECT COUNT(*) FROM workspaces),(SELECT COUNT(*) FROM organization_change_records WHERE kind='workspaceCreated'),(SELECT COUNT(*) FROM request_log)",vec![]).await?,vec![vec![number(2)?,number(1)?,number(5)?]]);
    assert!(
        f.rows(A, "SELECT * FROM workspaces ORDER BY workspace_id", vec![])
            .await?
            == directory_before,
        "name refusals changed the directory"
    );
    assert!(
        f.snapshot().await?.1 == audit_before,
        "name refusals added or rewrote audit records"
    );
    assert!(
        f.rows(
            A,
            "SELECT * FROM actor_identities ORDER BY actor_identity_id",
            vec![]
        )
        .await?
            == actors_before,
        "existing callers' name refusals changed actor mappings"
    );
    let lookup = f
        .person(
            signed::LOOKUP,
            &json!({"organizationId":A,"workspaceId":workspace.workspace_id}),
            &owner,
            &signed::lookup_intent(A, workspace.workspace_id.as_str()),
            &signed::fresh_request_id(signed::NOW_SECONDS)?,
        )
        .await?;
    assert_eq!(
        lookup.value()?,
        json!({"result":"found","workspace":{"workspaceId":workspace.workspace_id,"name":"Generic name","description":"","state":"active"}})
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Literal accepted and refused names exercise real Rust canonical validation and SQLite ASCII uniqueness."
)]
#[tokio::test]
async fn descriptions_ascii_collision_index_and_non_ascii_names_keep_their_exact_contract() -> Result
{
    let f = Fixture::new().await?;
    f.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let operation = "019f0000-0000-7000-8000-00000000d7a1";
    let description = "Research notes\nwith a second line";
    let created = create(&f, &owner, operation, "Research", description).await?;
    assert_eq!(created.status, 200);
    assert_eq!(
        created.value()?,
        json!({"result":"applied","workspace":{"workspaceId":operation,"name":"Research","description":description,"state":"active"}})
    );
    let before = f
        .rows(A, "SELECT * FROM workspaces ORDER BY workspace_id", vec![])
        .await?;
    let collision = create(
        &f,
        &owner,
        "019f0000-0000-7000-8000-00000000d7a2",
        "rEsEaRcH",
        "",
    )
    .await?;
    assert_eq!(collision.status, 200);
    assert_eq!(
        collision.value()?,
        json!({"result":"refused","reason":"nameInUse"})
    );
    assert_eq!(
        f.rows(A, "SELECT * FROM workspaces ORDER BY workspace_id", vec![])
            .await?,
        before
    );
    let unchanged = all_effects(&f).await?;
    let noncanonical = create(
        &f,
        &owner,
        "019f0000-0000-7000-8000-00000000d7a3",
        " Research ",
        "",
    )
    .await?;
    assert_eq!(noncanonical.status, 401);
    assert!(noncanonical.body.is_empty());
    assert!(
        all_effects(&f).await? == unchanged,
        "signed noncanonical input must not be rewritten or admitted"
    );
    // Independently prove the SQL index itself rejects padded ASCII collisions.
    let result=f.runtime(A).await?.statement_host().run_batch(StatementBatch {
        statements:vec![GuardedStatement {statement:Statement {
            sql:"INSERT INTO workspaces(workspace_id,name,description,state,created_at,updated_at) VALUES (?,' RESEARCH ','','active',?,?)",
            args:vec![text("019f0000-0000-7000-8000-00000000d7a4"),text("2030-03-17T17:46:40Z"),text("2030-03-17T17:46:40Z")],
        },expect:RowExpectation::Exactly(1)}],alarm:AlarmUpdate::Unchanged,
    }).await?;
    assert!(
        matches!(
            result,
            BatchOutcome::ConstraintViolated {
                index: 0,
                kind: ConstraintKind::Unique
            }
        ),
        "the owning unique index did not reject surrounding spaces and ASCII case"
    );
    assert!(all_effects(&f).await? == unchanged);
    for (operation, name) in [
        ("019f0000-0000-7000-8000-00000000d7a5", "Ärea"),
        ("019f0000-0000-7000-8000-00000000d7a6", "ärea"),
    ] {
        let answer = create(&f, &owner, operation, name, "").await?;
        assert_eq!(answer.status, 200);
        assert_eq!(
            answer.value()?.get("result"),
            Some(&json!("applied")),
            "SQLite ASCII-only lower must not invent Unicode folding"
        );
    }
    assert_eq!(
        f.rows(A, "SELECT COUNT(*) FROM workspaces", vec![]).await?,
        vec![vec![number(3)?]]
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A person audit origin must not change machines or grant access beyond the ordinary machine allowance."
)]
#[tokio::test]
async fn person_create_records_checked_origin_without_machine_rights() -> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    f.seed_machine().await?;
    let before = machine_rows(&f).await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let operation = "019f0000-0000-7000-8000-00000000d7b1";
    let created = create(&f, &owner, operation, "Person directory", "").await?;
    assert_eq!(created.status, 200);
    assert!(
        machine_rows(&f).await? == before,
        "person create changed machine enrollment or credential state"
    );
    assert_eq!(f.rows(A,"SELECT request_id,kind,origin,actor_person_id,permission_checked,target_id FROM organization_change_records WHERE kind='workspaceCreated'",vec![]).await?,vec![vec![text(operation),text("workspaceCreated"),text("person"),text(signed::OWNER),text("workspace:create"),text(operation)]]);
    let request = |id: &str| -> Result<m::WorkspaceRequest> {
        Ok(m::WorkspaceRequest {
            request_id: Some(id.to_owned().try_into()?),
            workspace_id: Some(operation.to_owned().try_into()?),
            operation: m::WorkspaceOperation::CreateArea {
                name: "Ordinary machine area".to_owned().try_into()?,
            },
        })
    };
    let workspace_store = f
        .root
        .path()
        .join("workspace-stores")
        .join(format!("{operation}.sqlite"));
    assert!(
        !workspace_store.exists(),
        "directory creation must not create a workspace store"
    );
    let denied = f
        .host
        .request(&caller()?, request("019f0000-0000-7000-8000-00000000d7b2")?)
        .await?;
    assert_eq!(
        denied.result,
        m::WorkspaceResult::Refused(m::RefusalReason::WorkspaceNotAllowed)
    );
    assert!(
        !workspace_store.exists(),
        "denied machine operation must not open the workspace store"
    );
    assert!(machine_rows(&f).await? == before);
    f.apply(
        A,
        "UPDATE machines SET allowed_workspaces=? WHERE machine_id=?",
        vec![
            text(serde_json::to_string(&vec![operation])?),
            text(MACHINE),
        ],
    )
    .await?;
    let allowed = f
        .host
        .request(&caller()?, request("019f0000-0000-7000-8000-00000000d7b3")?)
        .await?;
    assert!(
        matches!(
            allowed.result,
            m::WorkspaceResult::Done(m::WorkspaceAnswer::Created { .. })
        ),
        "the same ordinary operation needs its explicit machine allowance"
    );
    assert_eq!(
        f.rows(A, "SELECT COUNT(*) FROM machines", vec![]).await?,
        vec![vec![number(1)?]]
    );
    f.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A held committed winner and queued same-name writer exercise serialized directory collision decisions."
)]
#[tokio::test]
async fn concurrent_same_name_creates_keep_the_first_workspace_and_retain_the_second_refusal()
-> Result {
    let f = Fixture::new().await?;
    f.ready().await?;
    let owner = signed::Person::owner(A, signed::NOW_SECONDS)?;
    let first_id = "019f0000-0000-7000-8000-00000000d7d1";
    let second_id = "019f0000-0000-7000-8000-00000000d7d2";
    let runtime = f.runtime(A).await?;
    let mut held = runtime
        .pause_points()
        .hold_central(collaboration_central_core::PausePoint::AfterSignInSample);
    let first_headers = owner.headers(
        signed::CREATE,
        &intent(first_id, "Concurrent name", "winner"),
        &signed::fresh_request_id(signed::NOW_SECONDS)?,
        signed::NOW_SECONDS,
    )?;
    let first_body = serde_json::to_string(&body(first_id, "Concurrent name", "winner"))?;
    let client = f.client();
    let first =
        tokio::spawn(async move { post(&client, signed::CREATE, first_body, first_headers).await });
    held.until_held().await;
    let second_headers = owner.headers(
        signed::CREATE,
        &intent(second_id, "CONCURRENT NAME", "loser"),
        &signed::fresh_request_id(signed::NOW_SECONDS)?,
        signed::NOW_SECONDS,
    )?;
    let second_body = serde_json::to_string(&body(second_id, "CONCURRENT NAME", "loser"))?;
    let client = f.client();
    let mut second =
        tokio::spawn(
            async move { post(&client, signed::CREATE, second_body, second_headers).await },
        );
    let queued = tokio::select! {()=runtime.until_central_waiting(1)=>true,_=&mut second=>false};
    assert!(queued, "second create crossed the held org turn");
    held.release();
    let first = first.await??;
    let second = second.await??;
    assert_eq!(first.status, 200);
    assert_eq!(first.value()?.get("result"), Some(&json!("applied")));
    assert_eq!(second.status, 200);
    assert_eq!(
        second.value()?,
        json!({"result":"refused","reason":"nameInUse"})
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT workspace_id,name,description FROM workspaces",
            vec![]
        )
        .await?,
        vec![vec![
            text(first_id),
            text("Concurrent name"),
            text("winner")
        ]]
    );
    assert_eq!(f.rows(A,"SELECT (SELECT COUNT(*) FROM request_log),(SELECT COUNT(*) FROM organization_change_records WHERE kind='workspaceCreated'),(SELECT COUNT(*) FROM organization_used_requests)",vec![]).await?,vec![vec![number(2)?,number(1)?,number(2)?]]);
    assert_eq!(
        create(&f, &owner, second_id, "CONCURRENT NAME", "loser")
            .await?
            .body,
        second.body,
        "the queued name refusal is itself retained"
    );
    f.close().await
}
