//! Real typed project rows representing notification work pending at the staging wipe.
use crate::{
    cutover_host_fixture::CutoverHostFixture,
    native_snapshot_fixture::{NativeFixture, fixture_database},
};
use message_board::{
    ActorOrigin, Identity, IntentRecipient, IntentState, NotifyIntent, NotifyIntentId,
    NotifyIntentProps, Record, RecordedActor, RequestId, ResolvedRecipient, SessionEndpointRef,
    SessionRef,
};
use message_board_storage::ProjectStore;
use std::sync::Arc;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub(super) async fn add_pending_intent(
    host: &CutoverHostFixture,
    source: &mut NativeFixture,
) -> TestResult<NotifyIntent> {
    let database = Arc::new(
        fixture_database(&source.path, source.manifest.database, host.peer)
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?,
    );
    let store =
        ProjectStore::from_database(source.selected.project.clone(), database.clone()).await?;
    let origin = ActorOrigin {
        installation: String::from(host.service.clone()).try_into()?,
        machine: "cutover source fixture".to_owned().try_into()?,
    };
    let mut write = store.begin_admission(origin).await?;
    let Some(Record::Project(state)) = write
        .record(&source.selected.project.clone().into())
        .await?
    else {
        return Err("source project state absent".into());
    };
    let intent_id = NotifyIntentId::generate();
    let recipient = Identity::Session {
        session: SessionRef {
            endpoint: SessionEndpointRef {
                service_id: String::from(host.service.clone()).try_into()?,
                endpoint_id: "codex-local".to_owned().try_into()?,
            },
            session_id: uuid::Uuid::now_v7().to_string().try_into()?,
        },
    };
    let intent = NotifyIntent::new(NotifyIntentProps {
        envelope: message_board::RecordEnvelope {
            id: intent_id.clone().into(),
            request: Some(RequestId::generate()),
            ..state.envelope
        },
        intent_id: intent_id.clone(),
        cause: source.selected.project.clone().into(),
        recipient: IntentRecipient::Resolved(ResolvedRecipient {
            actor: RecordedActor::declared(recipient),
            via_role: None,
        }),
        state: IntentState::Emitted {},
    })?;
    source.cursor = write.append_record(Record::NotifyIntent(intent)).await?;
    write.commit().await?;
    let mut read = store.snapshot().await?;
    let saved = read.record(&intent_id.into()).await?;
    read.finish().await?;
    let Some(Record::NotifyIntent(saved)) = saved else {
        return Err("persisted pending intent missing".into());
    };
    let mut snapshot = database.snapshot().await?;
    source.manifest = snapshot.manifest().await?;
    snapshot.close().await?;
    drop(store);
    database.close().await;
    drop(database);
    Ok(saved)
}
