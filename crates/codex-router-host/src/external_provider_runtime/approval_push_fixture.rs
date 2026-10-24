//! Private store-backed approval composition and native notice capture.

use automation_storage::AutomationStore;
use collaboration_protocol::{
    MachineId, PushKind, PushLineInput, PushOrigin, PushRecord, RouterLink, SessionRef,
    UuidIdentity, render_push_line,
};
use collaboration_service::{MachineIdentity, ServiceIdentity, ServiceInteractionBroker};
use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

type TestResult<TValue> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;

pub(super) struct ApprovalPushFixture {
    store: Arc<interaction_storage::InteractionStore>,
    legacy_path: PathBuf,
    machine_identity: MachineIdentity,
}

impl ApprovalPushFixture {
    pub(super) async fn compose(
        directory: &Path,
        service_id: &UuidIdentity,
        broker: &Arc<ServiceInteractionBroker>,
    ) -> TestResult<Self> {
        let legacy_path = directory.join("approval-pushes.sqlite");
        let legacy_owner = AutomationStore::open(&legacy_path).await?;
        legacy_owner.close().await?;
        let store = broker.interaction_store();
        let machine_identity = MachineIdentity::new(service_id.clone(), Some("approval-fixture"))?;
        let service_id_text = String::from(service_id.clone());
        let identity = ServiceIdentity::new(&service_id_text, &service_id_text)?
            .with_machine_identity(machine_identity.clone())?
            .with_approval_broker(Arc::clone(broker));
        drop(identity);
        Ok(Self {
            store,
            legacy_path,
            machine_identity,
        })
    }

    pub(super) async fn captured_approval(
        &self,
        delivered_line: &str,
        approver: &SessionRef,
    ) -> TestResult<PushRecord> {
        let link_text = delivered_line
            .split_whitespace()
            .last()
            .ok_or("approval push omitted its Router link")?;
        let link = RouterLink::parse(link_text)?;
        if link.machine_id() != &MachineId::from(self.machine_identity.service_id().clone()) {
            return Err("approval push link belongs to another service".into());
        }
        let record = self
            .store
            .get_push_record(link.push_id())
            .await?
            .ok_or("delivered approval push was not stored")?;
        if record.kind != PushKind::Approval
            || record.origin != PushOrigin::Router(PushKind::Approval)
            || &record.target != approver
        {
            return Err("stored approval push has the wrong kind, origin or target".into());
        }
        let expected_line = render_push_line(&PushLineInput {
            link,
            machine_label: self.machine_identity.machine_label().clone(),
            origin: record.origin.clone(),
            header_facts: record.header_facts.clone(),
            body: record.body.clone(),
        })?;
        if delivered_line != expected_line || delivered_line.lines().count() != 1 {
            return Err("native approver did not receive the stored neutral push line".into());
        }
        // The completed A6 cutover removes the legacy table. Its owner closed
        // in compose; this independent observer refuses its reappearance.
        let mut legacy = SqliteConnection::connect_with(
            &SqliteConnectOptions::new()
                .filename(&self.legacy_path)
                .read_only(true),
        )
        .await?;
        let legacy_push_tables: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM sqlite_schema WHERE type='table' AND name='router_pushes'",
        )
        .fetch_one(&mut legacy)
        .await?;
        legacy.close().await?;
        if legacy_push_tables != 0 {
            let error = "legacy approval store recreated its retired push table";
            eprintln!("{error}");
            return Err(error.into());
        }
        Ok(record)
    }
}
