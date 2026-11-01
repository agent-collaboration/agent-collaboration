//! Launcher assertions use supplied facts and IDs; provider permission settings never select isolation.
use super::*;
use collaboration_contracts::HarnessMcpConfigurationIdentity;
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};

struct FixedIds(AtomicU64);
impl IdSource for FixedIds {
    fn new_id(&self) -> uuid::Uuid {
        uuid::Uuid::from_u128(
            0x019f0000000070008000000000000100 + u128::from(self.0.fetch_add(1, Ordering::SeqCst)),
        )
    }
}

#[tokio::test]
async fn host_launch_facts_keep_the_confirmed_environment_and_assert_host_with_fresh_injected_ids()
{
    let directory = tempfile::tempdir().unwrap();
    let endpoint = serde_json::from_value(
        json!({"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"}),
    )
    .unwrap();
    let actor: collaboration_protocol::SessionRef =
        serde_json::from_value(json!({"endpoint":endpoint,"sessionId":"permission-route-owner"}))
            .unwrap();
    let mut declared = HarnessDeclaredLaunchInput {
        project_id: Some(
            "019f0000-0000-7000-8000-000000000004"
                .to_owned()
                .try_into()
                .unwrap(),
        ),
        endpoint,
        working_directory: directory
            .path()
            .to_str()
            .unwrap()
            .to_owned()
            .try_into()
            .unwrap(),
        settings: HarnessDeclaredSettings::Native {
            model: Some("fixture-model".to_owned().try_into().unwrap()),
            effort: None,
            permissions: collaboration_contracts::HarnessNativePermissions::Explicit {
                access: collaboration_protocol::RouterAccess::WriteRestricted,
                created_by: actor.clone().into(),
                approver: actor.clone().into(),
            },
            additional_directories: Vec::new(),
            fork: None,
            root_message_id: None,
        },
        user_mcp: HarnessMcpConfigurationIdentity::empty(),
    };
    let operation = "019f0000-0000-7000-8000-000000000020"
        .to_owned()
        .try_into()
        .unwrap();
    let ids = FixedIds(AtomicU64::new(0));

    let physical = declared_host_launch(&operation, &declared, &MachineEnvironment::Physical, &ids)
        .await
        .unwrap();
    declared.settings = HarnessDeclaredSettings::Provider {
        model: Some("fixture-model".to_owned().try_into().unwrap()),
        effort: None,
        access: collaboration_protocol::RouterAccess::WorkspaceWrite,
        mode: Some("provider-mode".to_owned().try_into().unwrap()),
        created_by: actor.clone().into(),
        approver: actor.into(),
    };
    let cloud = declared_host_launch(&operation, &declared, &MachineEnvironment::CloudVm, &ids)
        .await
        .unwrap();

    assert_eq!(physical.isolation.kind, IsolationKind::Host);
    assert_eq!(cloud.isolation.kind, IsolationKind::Host);
    assert_eq!(physical.machine_environment, MachineEnvironment::Physical);
    assert_eq!(cloud.machine_environment, MachineEnvironment::CloudVm);
    assert_ne!(physical.isolation.instance_id, cloud.isolation.instance_id);
    assert_eq!(
        physical.launch_context.working_directory,
        declared.working_directory
    );
    assert_eq!(physical.launch_context.model, cloud.launch_context.model);
    assert!(physical.launch_context.repository.is_none());
    assert!(physical.launch_context.commit.is_none());
}
