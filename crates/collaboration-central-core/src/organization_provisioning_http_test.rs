use super::organization_provisioning_router;
use crate::{
    identity_contract::{
        ORGANIZATION_STATEMENT_DIGEST_ROUTE, OrganizationStatementDigestReply,
        PROVISION_ORGANIZATION_ROUTE, ProvisionOrganization, ProvisionOrganizationResult,
        ReadOrganizationStatementDigest,
    },
    organization_authority::{OrganizationProvisioningFailure, OrganizationProvisioningPort},
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tower_service::Service;

const ORGANIZATION: &str = "019a0000-0000-7000-8000-000000000001";
// Shape only: the Fleet object, not this HTTP policy, checks its signature.
const STATEMENT: &str = "aGVhZGVy.Y2xhaW1z.c2lnbmF0dXJl";

// This stand-in records transport calls only; it proves no SQL, stamp or link isolation.
struct RecordingProvisioner {
    requests: Mutex<Vec<ProvisionOrganization>>,
    digests: Mutex<Vec<ReadOrganizationStatementDigest>>,
    revisions: Mutex<Vec<crate::identity_contract::AuthorizationRevision>>,
    answer: Result<ProvisionOrganizationResult, OrganizationProvisioningFailure>,
}
impl OrganizationProvisioningPort for RecordingProvisioner {
    async fn provision_organization(
        &self,
        request: &ProvisionOrganization,
    ) -> Result<ProvisionOrganizationResult, OrganizationProvisioningFailure> {
        self.requests
            .lock()
            .map_err(|_| OrganizationProvisioningFailure::Unavailable)?
            .push(request.clone());
        self.answer.clone()
    }
    async fn deliver_authorization_revision(
        &self,
        revision: &crate::identity_contract::AuthorizationRevision,
    ) -> Result<
        crate::identity_contract::AuthorizationRevisionAcknowledgement,
        OrganizationProvisioningFailure,
    > {
        self.revisions
            .lock()
            .map_err(|_| OrganizationProvisioningFailure::Unavailable)?
            .push(revision.clone());
        self.answer.clone().map(|_| {
            crate::identity_contract::AuthorizationRevisionAcknowledgement {
                acknowledged_revision: 7.try_into().unwrap(),
            }
        })
    }
    async fn read_statement_digest(
        &self,
        request: &ReadOrganizationStatementDigest,
    ) -> Result<OrganizationStatementDigestReply, OrganizationProvisioningFailure> {
        self.digests
            .lock()
            .map_err(|_| OrganizationProvisioningFailure::Unavailable)?
            .push(request.clone());
        self.answer
            .clone()
            .map(|_| OrganizationStatementDigestReply {
                digest: "ab".repeat(32),
                expires_at: 1_810_368_000,
                signing_key_id: "stamp-a".into(),
            })
    }
}

fn recorder(
    answer: Result<ProvisionOrganizationResult, OrganizationProvisioningFailure>,
) -> Arc<RecordingProvisioner> {
    Arc::new(RecordingProvisioner {
        requests: Mutex::new(Vec::new()),
        digests: Mutex::new(Vec::new()),
        revisions: Mutex::new(Vec::new()),
        answer,
    })
}

fn request(body: Value) -> Request<Body> {
    Request::post(PROVISION_ORGANIZATION_ROUTE)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap()
}

// Oracle: B2 provisioning flow and the published F1c payload/result, not a database mock.
// No pre-fix semantic red has run for this new HTTP adapter.
#[tokio::test]
async fn a_valid_request_calls_the_provisioner_once_and_returns_the_closed_answer() {
    let provisioner = recorder(Ok(ProvisionOrganizationResult::Provisioned {}));
    let mut router = organization_provisioning_router(provisioner.clone());

    let response = router
        .call(request(
            json!({"organizationId":ORGANIZATION,"organizationStatement":STATEMENT}),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"result":"provisioned"})
    );
    let calls = provisioner.requests.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].organization_id.as_str(), ORGANIZATION);
}

#[tokio::test]
async fn another_operation_or_malformed_body_never_calls_the_provisioner() {
    let provisioner = recorder(Ok(ProvisionOrganizationResult::Provisioned {}));
    let mut router = organization_provisioning_router(provisioner.clone());

    for body in [
        json!({"operation":"admitSignIn","organizationId":ORGANIZATION}),
        json!({"organizationId":ORGANIZATION,"organizationStatement":STATEMENT,"owner":"invented"}),
        // The retired organizationId-only body: a stamp is now required.
        json!({"organizationId":ORGANIZATION}),
        json!({"organizationId":ORGANIZATION,"organizationStatement":"not-a-jws"}),
        json!({"organizationId":"00000000-0000-4000-8000-000000000001"}),
        json!({}),
        Value::Null,
    ] {
        let response = router.call(request(body)).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap(),
            json!({"kind":"wrong-operation"})
        );
    }
    assert!(provisioner.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn a_failed_attempt_is_never_reported_provisioned_or_retried() {
    for (failure, expected) in [
        (
            OrganizationProvisioningFailure::OrganizationMismatch,
            StatusCode::FORBIDDEN,
        ),
        (
            OrganizationProvisioningFailure::InvalidStatement,
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            OrganizationProvisioningFailure::StaleStatement,
            StatusCode::CONFLICT,
        ),
        (
            OrganizationProvisioningFailure::Unavailable,
            StatusCode::SERVICE_UNAVAILABLE,
        ),
        (
            OrganizationProvisioningFailure::OutcomeUnknown,
            StatusCode::SERVICE_UNAVAILABLE,
        ),
    ] {
        let provisioner = recorder(Err(failure));
        let mut router = organization_provisioning_router(provisioner.clone());

        let response = router
            .call(request(
                json!({"organizationId":ORGANIZATION,"organizationStatement":STATEMENT}),
            ))
            .await
            .unwrap();

        assert_eq!(response.status(), expected);
        assert!(
            to_bytes(response.into_body(), 1024)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(provisioner.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn the_method_and_path_select_the_operation_before_a_body_is_used() {
    let provisioner = recorder(Ok(ProvisionOrganizationResult::Provisioned {}));
    let mut router = organization_provisioning_router(provisioner.clone());

    for (request, expected) in [
        (
            Request::get(PROVISION_ORGANIZATION_ROUTE)
                .body(Body::empty())
                .unwrap(),
            StatusCode::METHOD_NOT_ALLOWED,
        ),
        (
            Request::post("/public/organization-provisioning")
                .body(Body::empty())
                .unwrap(),
            StatusCode::NOT_FOUND,
        ),
    ] {
        assert_eq!(router.call(request).await.unwrap().status(), expected);
    }
    assert!(provisioner.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_digest_read_returns_only_the_digest_for_the_named_organization() {
    // Wrong build caught: the statement itself crossing the link, or the read
    // reaching the provisioning capability.
    let provisioner = recorder(Ok(ProvisionOrganizationResult::Provisioned {}));
    let mut router = organization_provisioning_router(provisioner.clone());

    let response = router
        .call(
            Request::post(ORGANIZATION_STATEMENT_DIGEST_ROUTE)
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_vec(&json!({"organizationId":ORGANIZATION})).unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&body).unwrap(),
        json!({"digest":"ab".repeat(32),"expiresAt":1_810_368_000,"signingKeyId":"stamp-a"})
    );
    assert_eq!(provisioner.digests.lock().unwrap().len(), 1);
    assert!(provisioner.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn the_revision_push_answers_the_retained_floor_and_maps_its_refusals() {
    // Wrong build caught: a person-before-organization push answered as
    // success, or a refusal leaking a body.
    for (answer, expected, body) in [
        (
            Ok(ProvisionOrganizationResult::Provisioned {}),
            StatusCode::OK,
            Some(json!({"acknowledgedRevision":7})),
        ),
        (
            Err(OrganizationProvisioningFailure::OrganizationFloorMissing),
            StatusCode::PRECONDITION_FAILED,
            None,
        ),
        (
            Err(OrganizationProvisioningFailure::StaleStatement),
            StatusCode::CONFLICT,
            None,
        ),
        (
            Err(OrganizationProvisioningFailure::OrganizationMismatch),
            StatusCode::FORBIDDEN,
            None,
        ),
    ] {
        let provisioner = recorder(answer);
        let mut router = organization_provisioning_router(provisioner.clone());
        let response = router
            .call(
                Request::post(crate::identity_contract::AUTHORIZATION_REVISIONS_ROUTE)
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&json!({"kind":"person","organizationId":ORGANIZATION,
                            "personId":"019a0000-0000-7000-8000-000000000002",
                            "subjectId":"019a0000-0000-7000-8000-000000000003",
                            "revision":3,"state":"active"}))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
        match body {
            Some(body) => assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), body),
            None => assert!(bytes.is_empty()),
        }
        assert_eq!(provisioner.revisions.lock().unwrap().len(), 1);
    }
}
