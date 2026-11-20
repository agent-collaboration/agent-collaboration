//! Private store-backed approval composition and native notice capture.

use collaboration_contracts::MachineIdentity;
use collaboration_protocol::{
    MachineId, PushKind, PushLineInput, PushOrigin, PushRecord, RouterLink, SessionRef,
    UuidIdentity, render_push_line,
};

use collaboration_runtime::ServiceInteractionBroker;
use std::sync::Arc;

type TestResult<TValue> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;

pub(super) struct ApprovalPushFixture {
    store: Arc<interaction_storage::InteractionStore>,
    machine_identity: MachineIdentity,
}

impl ApprovalPushFixture {
    pub(super) fn compose(
        service_id: &UuidIdentity,
        broker: &Arc<ServiceInteractionBroker>,
    ) -> TestResult<Self> {
        let store = broker.interaction_store();
        let machine_identity = MachineIdentity::new(service_id.clone(), Some("approval-fixture"))?;
        broker.install_display_names(collaboration_runtime::SessionDisplayNameCache::default());
        broker.install_push_context(store.clone(), machine_identity.clone());
        Ok(Self {
            store,
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
        Ok(record)
    }
}
