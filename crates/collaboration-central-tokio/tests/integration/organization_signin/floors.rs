use super::support::*;
use serde_json::json;

#[expect(
    clippy::panic_in_result_fn,
    reason = "B2 floor acknowledgement and conflict assertions exercise committed private routes."
)]
#[tokio::test]
async fn floors_rise_acknowledge_older_pushes_refuse_equal_conflicts_and_never_resume_deleted()
-> Result {
    let f = Fixture::new().await?;
    assert_eq!(
        f.provision(A, &signed::stamp(A, signed::NOW_SECONDS)?)
            .await?
            .status,
        200
    );
    let person = person_floor(A, signed::OWNER, signed::OWNER_SUBJECT, 4, "active");
    assert_eq!(
        f.post(REVISIONS, &person).await?.status,
        412,
        "person floor before organization floor is refused"
    );
    for revision in [1, 3, 3, 2] {
        let reply = f
            .post(REVISIONS, &organization_floor(A, revision, "active"))
            .await?;
        assert_eq!(reply.status, 200);
        assert_eq!(
            reply.value()?,
            json!({"acknowledgedRevision":if revision==1{1}else{3}})
        );
    }
    assert_eq!(
        f.post(REVISIONS, &organization_floor(A, 3, "stopped"))
            .await?
            .status,
        409
    );
    let mut other_policy = organization_floor(A, 3, "active");
    signed::set_claim(
        &mut other_policy,
        "permissionPolicyDigest",
        json!("0000000000000000000000000000000000000000000000000000000000000000"),
    )?;
    assert_eq!(f.post(REVISIONS, &other_policy).await?.status, 409);
    for revision in [4, 4, 2] {
        let reply = f
            .post(
                REVISIONS,
                &person_floor(A, signed::OWNER, signed::OWNER_SUBJECT, revision, "active"),
            )
            .await?;
        assert_eq!(reply.status, 200);
        assert_eq!(reply.value()?, json!({"acknowledgedRevision":4}));
    }
    assert_eq!(
        f.post(
            REVISIONS,
            &person_floor(A, signed::OWNER, signed::OWNER_SUBJECT, 4, "inactive")
        )
        .await?
        .status,
        409
    );
    assert_eq!(
        f.post(
            REVISIONS,
            &person_floor(A, signed::OWNER, signed::MEMBER_SUBJECT, 5, "active")
        )
        .await?
        .status,
        409,
        "a subject change cannot replace the person's retained identity"
    );
    assert_eq!(
        f.post(REVISIONS, &organization_floor(A, 4, "deleted"))
            .await?
            .value()?,
        json!({"acknowledgedRevision":4})
    );
    assert_eq!(
        f.post(REVISIONS, &organization_floor(A, 5, "active"))
            .await?
            .value()?,
        json!({"acknowledgedRevision":4}),
        "deleted organizations cannot resume even under a higher push"
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT minimum_revision,state FROM organization_authorization_floors",
            vec![]
        )
        .await?,
        vec![vec![number(4)?, text("deleted")]]
    );
    assert_eq!(
        f.rows(
            A,
            "SELECT minimum_revision,state FROM person_organization_authorization_floors",
            vec![]
        )
        .await?,
        vec![vec![number(4)?, text("active")]]
    );
    assert_eq!(
        f.snapshot().await?.1.len(),
        1,
        "floor pushes do not invent organization changes"
    );
    f.close().await
}
