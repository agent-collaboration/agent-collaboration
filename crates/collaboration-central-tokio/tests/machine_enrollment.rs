#![cfg(feature = "conformance")]

use collaboration_central_core::{
    AccessCaller, AccessClientId, AdmittedMachine, CoreHost, EnrollMachine, EnrollmentDecision,
    EnrollmentRefusal, ObjectAddress, ObjectRuntime, OwnerIdentity, PlatformIo, RegistryKind,
};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{
    AtomicStore, CallFailure, NotSentReason, StoreError, storage::TransactionScope,
};
use fleet_sync_lease::{LeasePolicy, RegistryConfig};
use fleet_sync_protocol::{AuthRefusal, PeerId, ReceiverId};
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore, TokioObject};
use message_board::{MachineLabel, ServiceId};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use sqlx_core::{executor::Executor, query::query, row::Row};
use std::{path::Path, sync::Arc, time::Duration};

#[path = "machine_enrollment/call_edge_mapping.rs"]
mod call_edge_mapping;
#[path = "machine_enrollment/mail_admission.rs"]
mod mail_admission;
#[path = "native_http/platform.rs"]
mod native_fixture;
#[path = "machine_enrollment/peer_binding.rs"]
mod peer_binding;
#[path = "machine_enrollment/presence_cases.rs"]
mod presence_cases;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
struct Io;
impl Clock for Io {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_792_281_600_000)
    }
}
impl IdSource for Io {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for Io {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl PlatformIo for Io {
    async fn object_call(&self, _: ObjectAddress, _: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Overloaded,
        })
    }
}
type Runtime = ObjectRuntime<RegistryKind, TokioStorageHost<CoreHost<Io>>, Io>;
struct Fixture {
    objects: ObjectStore<CoreHost<Io>>,
    object: Arc<TokioObject<CoreHost<Io>>>,
    runtime: Runtime,
}
impl Fixture {
    async fn open(root: &Path) -> TestResult<Self> {
        let objects = ObjectStore::open(root).await?;
        let receiver = ReceiverId::try_from("019f0000-0000-7000-8000-000000000801".to_owned())?;
        let object = objects
            .object(
                ObjectKey::new("registry")?,
                Default::default(),
                Arc::new(CoreHost::conformance_registry(Io, receiver)),
            )
            .await?;
        let runtime = ObjectRuntime::registry(
            Arc::new(TokioStorageHost::new(object.clone())),
            Io,
            RegistryConfig {
                lease: LeasePolicy {
                    lease_duration: Duration::from_secs(60),
                    renew_interval: Duration::from_secs(20),
                },
                authoritative_receiver: receiver,
                release_unsent_for: Duration::from_secs(10),
                move_needs_renewal_within: None,
            },
            collaboration_central_tokio::DeliveryConfig {
                relay_peer: fleet_sync_protocol::PeerId::try_from(
                    "019f0000-0000-7000-8000-000000005240".to_owned(),
                )?,
            },
        )?;
        runtime.initialize().await?;
        Ok(Self {
            objects,
            object,
            runtime,
        })
    }
    async fn authenticate(
        &self,
        request: &EnrollMachine,
    ) -> TestResult<Result<AdmittedMachine, AuthRefusal>> {
        Ok(self
            .runtime
            .authenticate_machine(
                request.access_client_id.clone(),
                request.credential_hash.clone(),
            )
            .await?)
    }
    async fn execute(&self, sql: &'static str) -> TestResult {
        self.object
            .transaction(move |scope| {
                Box::pin(async move {
                    scope
                        .executor()
                        .execute(sql)
                        .await
                        .map_err(StoreError::backend)?;
                    Ok(())
                })
            })
            .await?;
        Ok(())
    }
}
fn request(service: &str, client: &str, hash: &str) -> TestResult<EnrollMachine> {
    Ok(EnrollMachine {
        installation: ServiceId::try_from(service.to_owned())?,
        peer: PeerId::new(durable_steps::Uuid::now_v7())?,
        label: MachineLabel::try_from("original label".to_owned())?,
        access_client_id: AccessClientId::new(client)?,
        credential_hash: hash.into(),
        expires_at: "2033-05-18T03:33:20Z".parse()?,
    })
}
fn owner() -> TestResult<OwnerIdentity> {
    Ok(OwnerIdentity::from_access(AccessCaller::Owner {
        email: "owner@example.test".into(),
    })?)
}
fn peer(result: EnrollmentDecision, request: &EnrollMachine) -> TestResult<PeerId> {
    match result {
        EnrollmentDecision::MachineEnrolled {
            peer,
            expires_at,
            authoritative_receiver,
            relay_peer,
        } if expires_at == request.expires_at
            && peer == request.peer
            && relay_peer.to_string() == "019f0000-0000-7000-8000-000000005240"
            && authoritative_receiver.to_string() == "019f0000-0000-7000-8000-000000000801" =>
        {
            Ok(peer)
        }
        other => Err(format!("enrollment returned {other:?}").into()),
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn v4_service_keeps_one_v7_peer_across_updates_and_pending_readmission_after_reopen()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let owner = owner()?;
    let original = request(
        "550e8400-e29b-41d4-a716-446655440000",
        "first-client",
        "first-hash",
    )?;
    let first = peer(
        fixture
            .runtime
            .enroll_machine(&owner, original.clone())
            .await?,
        &original,
    )?;
    assert_eq!(first.as_uuid().get_version_num(), 7);
    assert_ne!(first.to_string(), original.installation.as_str());
    let admitted = fixture
        .authenticate(&original)
        .await?
        .map_err(|r| format!("{r:?}"))?;
    assert_eq!(admitted.peer(), first);
    assert_eq!(admitted.origin().installation, original.installation);

    let mut updated = original.clone();
    updated.label = MachineLabel::try_from("new label".to_owned())?;
    updated.access_client_id = AccessClientId::new("new-client")?;
    updated.credential_hash = "new-hash".into();
    let (left, right) = tokio::join!(
        fixture.runtime.enroll_machine(&owner, updated.clone()),
        fixture.runtime.enroll_machine(&owner, updated.clone())
    );
    assert_eq!(peer(left?, &updated)?, first);
    assert_eq!(peer(right?, &updated)?, first);
    assert_eq!(
        admitted.origin().machine.as_str(),
        "original label",
        "admitted provenance changed after relabeling"
    );
    assert_eq!(
        fixture
            .authenticate(&updated)
            .await?
            .map_err(|r| format!("{r:?}"))?
            .origin()
            .machine
            .as_str(),
        "new label"
    );
    assert_eq!(
        fixture.authenticate(&original).await?,
        Err(AuthRefusal::Unknown)
    );

    // Persisted revoked state is setup for re-enrollment here. Ending sessions
    // through the actual revoke route remains a separate integration obligation.
    fixture
        .execute("UPDATE machine_enrollments SET state='revoked'")
        .await?;
    assert_eq!(
        fixture.authenticate(&updated).await?,
        Err(AuthRefusal::Revoked)
    );
    fixture.objects.close().await?;
    drop(fixture);
    let fixture = Fixture::open(directory.path()).await?;
    assert_eq!(
        fixture
            .runtime
            .enroll_machine(&owner, updated.clone())
            .await?,
        EnrollmentDecision::EnrollmentInProgress,
    );
    // This fixture refuses every object call, including Open. Stable identity
    // persists while activation waits; real completed readmission is exercised
    // by revocation_carryover's two normal close/open cycles.
    let reserved_peer = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let stored: String =
                    query::<sqlx_turso::Turso>("SELECT peer_id FROM machine_enrollments")
                        .fetch_one(scope.executor())
                        .await
                        .map_err(StoreError::backend)?
                        .try_get("peer_id")
                        .map_err(StoreError::backend)?;
                PeerId::try_from(stored).map_err(StoreError::backend)
            })
        })
        .await?;
    assert_eq!(reserved_peer, first);
    assert_eq!(
        fixture.authenticate(&updated).await?,
        Err(AuthRefusal::Revoked),
    );
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn client_and_hash_conflicts_leave_every_prior_enrollment_unchanged() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let owner = owner()?;
    let first = request("550e8400-e29b-41d4-a716-446655440001", "client-a", "hash-a")?;
    let second = request("550e8400-e29b-41d4-a716-446655440002", "client-b", "hash-b")?;
    let peer_a = peer(
        fixture
            .runtime
            .enroll_machine(&owner, first.clone())
            .await?,
        &first,
    )?;
    let peer_b = peer(
        fixture
            .runtime
            .enroll_machine(&owner, second.clone())
            .await?,
        &second,
    )?;
    for (client, hash, refusal) in [
        ("client-b", "unused", EnrollmentRefusal::AccessClientInUse),
        ("unused", "hash-b", EnrollmentRefusal::CredentialHashInUse),
    ] {
        let mut conflicting = first.clone();
        conflicting.access_client_id = AccessClientId::new(client)?;
        conflicting.credential_hash = hash.into();
        conflicting.label = MachineLabel::try_from("must not replace".to_owned())?;
        assert_eq!(
            fixture.runtime.enroll_machine(&owner, conflicting).await?,
            EnrollmentDecision::Refused(refusal)
        );
        assert_eq!(
            fixture
                .authenticate(&first)
                .await?
                .map_err(|r| format!("{r:?}"))?
                .peer(),
            peer_a
        );
        assert_eq!(
            fixture
                .authenticate(&second)
                .await?
                .map_err(|r| format!("{r:?}"))?
                .peer(),
            peer_b
        );
    }
    let mut wrong_access = first.clone();
    wrong_access.access_client_id = second.access_client_id;
    assert_eq!(
        fixture.authenticate(&wrong_access).await?,
        Err(AuthRefusal::WrongKind)
    );
    let count: String = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "SELECT CAST(COUNT(*) AS TEXT) AS count FROM machine_enrollments",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .try_get("count")
                .map_err(StoreError::backend)
            })
        })
        .await?;
    assert_eq!(count, "2");
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn stored_domain_corruption_is_an_error_instead_of_an_authentication_refusal() -> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let request = request("550e8400-e29b-41d4-a716-446655440003", "client", "hash")?;
    fixture
        .runtime
        .enroll_machine(&owner()?, request.clone())
        .await?;
    fixture
        .execute("UPDATE machine_enrollments SET state='unknown-state'")
        .await?;
    let result = fixture
        .runtime
        .authenticate_machine(request.access_client_id, request.credential_hash)
        .await;
    assert!(
        matches!(result, Err(StoreError::Corrupt { field, .. }) if field == "machine_enrollments.state")
    );
    fixture.objects.close().await?;
    Ok(())
}

#[path = "machine_enrollment/readmission_cases.rs"]
mod readmission_cases;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn expired_input_cannot_replace_a_valid_enrollment_and_stored_expiry_is_enforced()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let owner = owner()?;
    let valid = request("550e8400-e29b-41d4-a716-446655440004", "client", "hash")?;
    let expected = peer(
        fixture
            .runtime
            .enroll_machine(&owner, valid.clone())
            .await?,
        &valid,
    )?;
    let mut expired = valid.clone();
    expired.expires_at = "2020-01-01T00:00:00Z".parse()?;
    assert_eq!(
        fixture.runtime.enroll_machine(&owner, expired).await?,
        EnrollmentDecision::Refused(EnrollmentRefusal::InvalidInput)
    );
    assert_eq!(
        fixture
            .authenticate(&valid)
            .await?
            .map_err(|r| format!("{r:?}"))?
            .peer(),
        expected
    );
    fixture
        .execute("UPDATE machine_enrollments SET expires_at='2020-01-01T00:00:00Z'")
        .await?;
    assert_eq!(
        fixture.authenticate(&valid).await?,
        Err(AuthRefusal::Expired)
    );
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn verified_edge_authority_reaches_the_shared_registry_and_forged_sources_are_refused()
-> TestResult {
    use collaboration_central_conformance::{
        JwksFixtureServer, fixture_policy, machine_assertion, owner_assertion,
    };
    use collaboration_central_core::{
        AccessKeyCache, CentralApplication, Dispatch, ObjectCallId, RegistryCall, RegistryResult,
    };
    let directory = tempfile::tempdir()?;
    let fixture = Fixture::open(directory.path()).await?;
    let keys = JwksFixtureServer::start().await?;
    let (platform, _platform_directory) = native_fixture::native_platform(&keys.url).await?;
    let app = CentralApplication::new(
        platform.clone(),
        fixture_policy(),
        Arc::new(AccessKeyCache::default()),
    )?;
    let request = request("550e8400-e29b-41d4-a716-446655440005", "unused", "hash")?;
    let mut request = request;
    request.access_client_id = collaboration_central_conformance::access_client_id("edge-token")?;
    let dispatch = |call| -> TestResult<Dispatch<RegistryKind>> {
        Ok(Dispatch {
            call_id: ObjectCallId::new(durable_steps::Uuid::now_v7().to_string())?,
            to: (),
            call,
        })
    };
    let result = async {
        let call = app
            .prepare_call(
                Some(&owner_assertion()?),
                "/admin/v1/machines/enroll",
                dispatch(RegistryCall::EnrollMachine(request.clone()))?,
            )
            .await?;
        let bytes = fixture
            .runtime
            .on_call(call.bytes().to_vec())
            .await
            .map_err(|r| format!("{r:?}"))?;
        let result: RegistryResult = serde_json::from_slice(&bytes)?;
        let RegistryResult::Enrollment(result) = result else {
            return Err("wrong registry enrollment reply".into());
        };
        let expected_peer = peer(result, &request)?;
        let auth = app
            .prepare_call(
                Some(&machine_assertion("edge-token")?),
                "/v1/session",
                dispatch(RegistryCall::AuthenticateEnrollment {
                    credential_hash: "hash".into(),
                })?,
            )
            .await?;
        let bytes = fixture
            .runtime
            .on_call(auth.bytes().to_vec())
            .await
            .map_err(|r| format!("{r:?}"))?;
        let result: RegistryResult = serde_json::from_slice(&bytes)?;
        let RegistryResult::EnrollmentAuthenticated(Ok(admitted)) = result else {
            return Err("wrong registry authentication reply".into());
        };
        assert_eq!(admitted.peer(), expected_peer);
        assert_eq!(admitted.origin().installation, request.installation);

        // The receiver rechecks the serialized boundary. Its expected source
        // rules are the MayCall DAG and the authority from the verified edge.
        let original: serde_json::Value = serde_json::from_slice(call.bytes())?;
        for (field, value) in [
            ("sourceFace", serde_json::json!("copy")),
            ("source", serde_json::json!({"kind":"fleet"})),
            ("destinationFace", serde_json::json!("fleet")),
            ("authority", serde_json::json!({"kind":"object"})),
        ] {
            let mut forged = original.clone();
            *forged.get_mut(field).ok_or("call omitted required field")? = value;
            assert!(
                matches!(
                    fixture.runtime.on_call(serde_json::to_vec(&forged)?).await,
                    Err(CallFailure::NotSent { .. })
                ),
                "forged {field} was accepted"
            );
        }
        let bad_call = app
            .prepare_call(
                Some(&machine_assertion("edge-token")?),
                "/v1/session",
                dispatch(RegistryCall::EnrollMachine(request.clone()))?,
            )
            .await?;
        assert!(
            matches!(
                fixture.runtime.on_call(bad_call.bytes().to_vec()).await,
                Err(CallFailure::NotSent { .. })
            ),
            "machine authority enrolled a machine"
        );
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    platform.shutdown().await?;
    keys.shutdown().await?;
    fixture.objects.close().await?;
    result
}
