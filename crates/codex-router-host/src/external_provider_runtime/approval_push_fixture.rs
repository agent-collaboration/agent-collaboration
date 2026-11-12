//! Approval notice composition and native notice capture.

use collaboration_contracts::MachineIdentity;
use collaboration_protocol::{MachineId, PushKind, RouterLink, SessionRef, UuidIdentity};

use collaboration_runtime::ServiceInteractionBroker;
use std::{path::Path, sync::Arc};

type TestResult<TValue> = Result<TValue, Box<dyn std::error::Error + Send + Sync>>;

pub(super) struct ApprovalPushFixture {
    machine_identity: MachineIdentity,
}

impl ApprovalPushFixture {
    pub(super) fn compose(
        _directory: &Path,
        service_id: &UuidIdentity,
        broker: &Arc<ServiceInteractionBroker>,
    ) -> TestResult<Self> {
        let machine_identity = MachineIdentity::new(service_id.clone(), Some("approval-fixture"))?;
        broker.install_display_names(collaboration_runtime::SessionDisplayNameCache::default());
        broker.install_push_context(machine_identity.clone());
        Ok(Self { machine_identity })
    }

    /// Checks the one-line approval notice the native approver received. Notices are not
    /// stored, so the line itself is the whole record.
    pub(super) fn captured_approval(
        &self,
        delivered_line: &str,
        _approver: &SessionRef,
    ) -> TestResult<()> {
        let link_text = delivered_line
            .split_whitespace()
            .last()
            .ok_or("approval notice omitted its Router link")?;
        let link = RouterLink::parse(link_text)?;
        if link.machine_id() != &MachineId::from(self.machine_identity.service_id().clone()) {
            return Err("approval notice link belongs to another service".into());
        }
        let header = collaboration_protocol::parse_push_line_header(delivered_line)
            .ok_or("approval notice has no readable header")?;
        if header.kind != PushKind::Approval || delivered_line.lines().count() != 1 {
            return Err("native approver did not receive a one-line approval notice".into());
        }
        Ok(())
    }
}
