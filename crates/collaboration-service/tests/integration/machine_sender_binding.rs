//! Independent machine/directory truth resolves sealed senders in the custody scope.
use collaboration_contracts::RouterDeliveryTypes;
use collaboration_mail_protocol::MailSender;
use collaboration_service::MachineSenderBinding;
use durable_steps::{AtomicStore, CallFailure, StoreError, storage::TransactionScope};
use fleet_mail_delivery::{SenderBinding, SenderBindingFailure};
use fleet_protocol::{DatabaseId, PeerId, Submitter};
use fleet_tokio_objects::{CallEnvelope, CallRouter, TokioHost};
use interaction_storage::{InteractionAtomicStore, InteractionStore};
use message_board::{ProjectId, ServiceId};
use runtime_primitives::SystemClock;
use std::{error::Error, sync::Arc};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;
struct NoCalls;
impl CallRouter for NoCalls {
    #[expect(
        clippy::panic,
        reason = "Any external dispatch violates the scoped lookup contract."
    )]
    async fn dispatch(&self, _: CallEnvelope) -> Result<serde_json::Value, CallFailure> {
        panic!("binding lookup must not dispatch under the custody scope")
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup errors propagate."
)]
async fn self_binding_uses_the_independent_peer_instead_of_service_uuid_bits() -> TestResult {
    // Arrange distinct valid identities, with no directory row.
    let directory = tempfile::tempdir()?;
    let owner =
        Arc::new(InteractionStore::open(directory.path().join("interaction.sqlite")).await?);
    let service: ServiceId = uuid::Uuid::now_v7().to_string().try_into()?;
    let peer = PeerId::new(uuid::Uuid::now_v7())?;
    assert_ne!(service.as_str(), peer.as_uuid().to_string());
    let binding = Arc::new(MachineSenderBinding::new(service.clone(), peer));
    let host = Arc::new(TokioHost::new(Arc::new(NoCalls), SystemClock));
    let store = InteractionAtomicStore::new(owner.clone(), host);
    // Act through a real custody transaction; the provider cannot see a claimed owner.
    let mapped = store.transaction(move |scope| Box::pin(async move {
        let sender = MailSender::HumanUser { installation: service };
        Ok(<MachineSenderBinding as SenderBinding<_, RouterDeliveryTypes>>::resolve_submitter(binding.as_ref(), scope, &sender).await)
    })).await?;
    // Assert the host peer, never a cast of the service identifier.
    assert_eq!(mapped, Ok(Submitter::Machine(peer)));
    drop(store);
    Arc::try_unwrap(owner)
        .map_err(|_| "interaction owner retained")?
        .close()
        .await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup errors propagate."
)]
async fn directory_binding_reads_the_supplied_uncommitted_custody_scope() -> TestResult {
    // Arrange a directory pair visible only in this transaction.
    let directory = tempfile::tempdir()?;
    let owner =
        Arc::new(InteractionStore::open(directory.path().join("interaction.sqlite")).await?);
    let local: ServiceId = uuid::Uuid::now_v7().to_string().try_into()?;
    let remote: ServiceId = uuid::Uuid::now_v7().to_string().try_into()?;
    let peer = PeerId::new(uuid::Uuid::now_v7())?;
    let binding = Arc::new(MachineSenderBinding::new(
        local,
        PeerId::new(uuid::Uuid::now_v7())?,
    ));
    let host = Arc::new(TokioHost::new(Arc::new(NoCalls), SystemClock));
    let store = InteractionAtomicStore::new(owner.clone(), host);
    let mapped = store.transaction(move |scope| Box::pin(async move {
        sqlx_turso::sqlx::query("INSERT INTO installation_peer_directory(peer_id,service_id) VALUES(?,?)")
            .bind(peer.as_uuid().to_string()).bind(remote.as_str()).execute(scope.executor()).await.map_err(StoreError::backend)?;
        let sender = MailSender::HumanUser { installation: remote };
        Ok(<MachineSenderBinding as SenderBinding<_, RouterDeliveryTypes>>::resolve_submitter(binding.as_ref(), scope, &sender).await)
    })).await?;
    // Assert an independent directory lookup; no network or second transaction ran.
    assert_eq!(mapped, Ok(Submitter::Machine(peer)));
    drop(store);
    Arc::try_unwrap(owner)
        .map_err(|_| "interaction owner retained")?
        .close()
        .await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; setup errors propagate."
)]
async fn unknown_stays_retryable_and_project_identity_survives_physical_holder_changes()
-> TestResult {
    let directory = tempfile::tempdir()?;
    let owner =
        Arc::new(InteractionStore::open(directory.path().join("interaction.sqlite")).await?);
    let local: ServiceId = uuid::Uuid::now_v7().to_string().try_into()?;
    let unknown: ServiceId = uuid::Uuid::now_v7().to_string().try_into()?;
    let project = ProjectId::generate();
    let expected = Submitter::Project(DatabaseId::try_from(project.as_str().to_owned())?);
    let binding = Arc::new(MachineSenderBinding::new(
        local,
        PeerId::new(uuid::Uuid::now_v7())?,
    ));
    let host = Arc::new(TokioHost::new(Arc::new(NoCalls), SystemClock));
    let store = InteractionAtomicStore::new(owner.clone(), host);
    let (missing, project) = store.transaction(move |scope| Box::pin(async move {
        let missing = <MachineSenderBinding as SenderBinding<_, RouterDeliveryTypes>>::resolve_submitter(binding.as_ref(), scope, &MailSender::HumanUser { installation: unknown }).await;
        let project = <MachineSenderBinding as SenderBinding<_, RouterDeliveryTypes>>::resolve_submitter(binding.as_ref(), scope, &MailSender::Project { project }).await;
        Ok((missing, project))
    })).await?;
    assert_eq!(missing, Err(SenderBindingFailure::Unknown));
    assert_eq!(project, Ok(expected));
    drop(store);
    Arc::try_unwrap(owner)
        .map_err(|_| "interaction owner retained")?
        .close()
        .await?;
    Ok(())
}
