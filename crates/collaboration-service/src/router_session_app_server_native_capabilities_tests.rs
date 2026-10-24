//! Consumer discovery must distinguish an unavailable native reader from an empty view.
use super::test_support::ScriptedSessionBackend;
use super::*;
use crate::provider_capabilities::*;
use std::sync::Mutex;

#[tokio::test]
async fn native_capabilities_unavailable_reader_is_not_complete_empty_discovery() {
    // Arrange: no native catalogue port is bound to this connection.
    let backend = ScriptedSessionBackend::new().expect("scripted backend");
    let (_, models) = watch::channel(Vec::new());
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor().expect("actor"),
        backend.clone(),
        backend,
        models,
    ));

    // Act: this is the real asynchronous consumer dispatch, not the projection helper.
    let handled = handle_app_server_rpc(
        context,
        json!({
            "id": 1, "method": "skills/list", "params": {"cwds": ["/tmp"]}
        }),
    )
    .await
    .expect("RPC dispatch");

    // Assert: C1 forbids complete-empty success when discovery is unsupported.
    assert!(
        handled.response.get("error").is_some(),
        "{:?}",
        handled.response
    );
    assert!(handled.response.get("result").is_none());
}

pub(super) struct ScriptedCatalogue {
    pub(super) requests: Mutex<Vec<CatalogueRead>>,
    pub(super) result: CatalogueResult,
}

impl NativeCataloguePort for ScriptedCatalogue {
    fn read_plugin(&self, _request: NativePluginRead) -> NativePluginReadFuture<'_> {
        Box::pin(async {
            NativePluginReadResult::Unsupported {
                reason: CatalogueUnsupported::NotAdvertised,
            }
        })
    }

    fn read_catalogue(&self, request: CatalogueRead) -> CatalogueFuture<'_> {
        self.requests.lock().expect("requests").push(request);
        let result = self.result.clone();
        Box::pin(async move { result })
    }
}

pub(super) fn file_skill() -> NativeSkill {
    NativeSkill {
        id: NativeSkillId::new("native-workspace-selection".into()).expect("id"),
        name: CapabilityName::new("selection".into()).expect("name"),
        description: "Native workspace selection".into(),
        origin: NativeOrigin::Filesystem {
            actual_path: CapabilityPath::new(
                "/tmp/workspace/.claude/skills/selection/SKILL.md".into(),
            )
            .expect("path"),
        },
        native_scope: NativeScope::Workspace,
        availability: NativeAvailability::Available,
        display_version: None,
        plugin_id: None,
        lowering: NativeSkillLowering::Unqualified {
            reason: LoweringUnsupported::ScopeUnqualified,
        },
    }
}

pub(super) fn report_with_skills(skills: Vec<NativeSkill>) -> CatalogueObservation {
    CatalogueObservation {
        scope: CatalogueScope {
            binding: serde_json::from_value(json!({
                "endpoint":{"serviceId":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","endpointId":"claude-local"},
                "bindingId":"test-native-binding",
                "runtime":{"provider":"claudeCode","runtimeName":"scripted-native-source"},
                "transport":"stdioAcp",
                "generation":{"serviceEpoch":"0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89","generation":1},
                "capabilities":[{"name":"create","status":"supported","evidence":"advertised"}]
            })).expect("binding"),
            query: CatalogueQuery::Workspace { cwd: CapabilityPath::new("/tmp/workspace".into()).expect("cwd") },
            view: CatalogueView::FilesystemCatalogues,
        },
        observed_at: chrono::Utc::now(),
        skills: CatalogueSection::Supported { items: skills, enumeration: SourceEnumeration::Accounted, errors: Vec::new() },
        plugins: CatalogueSection::Unsupported { reason: CatalogueUnsupported::NativeOperationMissing },
    }
}

async fn discover(report: CatalogueObservation) -> (Value, Arc<ScriptedCatalogue>) {
    let backend = ScriptedSessionBackend::new().expect("backend");
    let (_, models) = watch::channel(Vec::new());
    let port = Arc::new(ScriptedCatalogue {
        requests: Mutex::new(Vec::new()),
        result: CatalogueResult::Ready {
            report: Box::new(report),
        },
    });
    let context = Arc::new(
        RouterSessionAppServerContext::new(
            backend.endpoint.clone(),
            ScriptedSessionBackend::actor().expect("actor"),
            backend.clone(),
            backend,
            models,
        )
        .with_native_catalogue(port.clone(), None),
    );
    let response = handle_app_server_rpc(
        context,
        json!({
            "id":2,"method":"skills/list","params":{"cwds":["/tmp/workspace"],"forceReload":true}
        }),
    )
    .await
    .expect("dispatch")
    .response;
    (response, port)
}

#[tokio::test]
async fn native_capabilities_discovery_uses_scoped_actor_bearing_read_and_real_file_origin() {
    // The native file facts are independent fixture values; the codec may not
    // replace this with Codex inventory or infer a path from the display name.
    let (response, port) = discover(report_with_skills(vec![file_skill()])).await;
    assert_eq!(
        response,
        json!({"id":2,"result":{"data":[{
            "cwd":"/tmp/workspace","skills":[{
                "name":"selection","description":"Native workspace selection",
                "path":"/tmp/workspace/.claude/skills/selection/SKILL.md",
                "scope":"repo","enabled":true,"pluginId":null
            }],"errors":[]
        }]}})
    );
    let requests = port.requests.lock().expect("requests");
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].actor,
        ScriptedSessionBackend::actor().expect("actor")
    );
    assert_eq!(
        requests[0].query,
        CatalogueQuery::Workspace {
            cwd: CapabilityPath::new("/tmp/workspace".into()).expect("cwd")
        }
    );
    assert_eq!(requests[0].view, CatalogueView::FilesystemCatalogues);
    assert!(requests[0].refresh_native);
}

#[tokio::test]
async fn native_capabilities_confirmed_bundled_skill_does_not_poison_filesystem_view() {
    let mut bundled = file_skill();
    bundled.origin = NativeOrigin::Bundled {
        native_key: CapabilityName::new("confirmed-native-builtin".into()).expect("native key"),
    };
    let (response, _) = discover(report_with_skills(vec![file_skill(), bundled.clone()])).await;
    assert_eq!(
        response
            .pointer("/result/data/0/skills")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    let (response, _) = discover(report_with_skills(vec![bundled])).await;
    assert_eq!(response.pointer("/result/data/0/skills"), Some(&json!([])));
}

#[tokio::test]
async fn native_capabilities_unknown_origin_is_an_error_including_all_unrepresentable_data() {
    let mut unknown = file_skill();
    unknown.origin = NativeOrigin::Other {
        native_label: CapabilityName::new("unresolved native source".into()).expect("label"),
    };
    for skills in [vec![file_skill(), unknown.clone()], vec![unknown]] {
        let (response, _) = discover(report_with_skills(skills)).await;
        assert!(response.get("result").is_none(), "{response}");
        assert!(
            response
                .pointer("/error/message")
                .and_then(Value::as_str)
                .expect("error")
                .contains("1 entries")
        );
        assert!(!response.to_string().contains("marketplacePath"));
    }
}

#[tokio::test]
async fn native_capabilities_incomplete_filesystem_read_does_not_become_empty_success() {
    let mut report = report_with_skills(Vec::new());
    report.skills = CatalogueSection::Supported {
        items: Vec::new(),
        enumeration: SourceEnumeration::Incomplete {
            limitations: CatalogueLimitations::new(CatalogueLimitation::MissingOrigin, Vec::new()),
        },
        errors: Vec::new(),
    };
    let (response, _) = discover(report).await;
    assert!(response.get("result").is_none());
    assert!(
        response
            .pointer("/error/message")
            .and_then(Value::as_str)
            .expect("error")
            .contains("incomplete native filesystem view")
    );
}

#[tokio::test]
async fn native_capabilities_complete_skill_section_survives_unqualified_plugin_section() {
    // Section coverage is independent: unsupported plugins do not erase a
    // complete filesystem skill observation (Program Design read shapes).
    let mut report = report_with_skills(vec![file_skill()]);
    report.plugins = CatalogueSection::Supported {
        items: Vec::new(),
        enumeration: SourceEnumeration::Incomplete {
            limitations: CatalogueLimitations::new(
                CatalogueLimitation::MissingEnablement,
                Vec::new(),
            ),
        },
        errors: Vec::new(),
    };
    let (response, _) = discover(report).await;
    assert_eq!(
        response
            .pointer("/result/data/0/skills")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1),
        "{response}"
    );
}

#[tokio::test]
async fn native_capabilities_wrong_context_observation_is_rejected() {
    let mut report = report_with_skills(vec![file_skill()]);
    report.scope.query = CatalogueQuery::Home;
    let (response, _) = discover(report).await;
    assert!(response.get("result").is_none());
    assert_eq!(
        response.pointer("/error/message"),
        Some(&json!("native catalogue scope unavailable"))
    );
}

#[tokio::test]
async fn native_capabilities_disabled_is_observed_and_unknown_enablement_is_not_defaulted() {
    let mut skill = file_skill();
    skill.availability = NativeAvailability::Disabled;
    let (response, _) = discover(report_with_skills(vec![skill.clone()])).await;
    assert_eq!(
        response.pointer("/result/data/0/skills/0/enabled"),
        Some(&json!(false))
    );
    skill.availability = NativeAvailability::Unknown;
    let (response, _) = discover(report_with_skills(vec![skill])).await;
    assert!(response.get("result").is_none());
    assert!(response.get("error").is_some());
}

#[tokio::test]
async fn native_capabilities_management_is_rejected_without_catalogue_or_command_dispatch() {
    let backend = ScriptedSessionBackend::new().expect("backend");
    let (_, models) = watch::channel(Vec::new());
    let port = Arc::new(ScriptedCatalogue {
        requests: Mutex::new(Vec::new()),
        result: CatalogueResult::Ready {
            report: Box::new(report_with_skills(vec![file_skill()])),
        },
    });
    let context = Arc::new(
        RouterSessionAppServerContext::new(
            backend.endpoint.clone(),
            ScriptedSessionBackend::actor().expect("actor"),
            backend.clone(),
            backend.clone(),
            models,
        )
        .with_native_catalogue(port.clone(), None),
    );
    for method in [
        "skills/config/write",
        "skills/extraRoots/set",
        "plugin/install",
        "plugin/uninstall",
        "plugin/enable",
        "plugin/disable",
        "plugin/reconcile",
        "marketplace/add",
        "marketplace/remove",
        "marketplace/upgrade",
    ] {
        let response = handle_app_server_rpc(
            context.clone(),
            json!({
                "id":3,"method":method,"params":{}
            }),
        )
        .await
        .expect("dispatch")
        .response;
        assert!(
            response
                .pointer("/error/message")
                .and_then(Value::as_str)
                .expect("error")
                .contains("management is unsupported"),
            "{method}: {response}"
        );
    }
    assert!(port.requests.lock().expect("requests").is_empty());
    assert!(backend.create_commands.lock().expect("create").is_empty());
    assert!(backend.prompt_commands.lock().expect("prompt").is_empty());
    assert!(
        backend
            .setting_commands
            .lock()
            .expect("settings")
            .is_empty()
    );
}

#[tokio::test]
async fn native_capabilities_missing_default_cwd_is_not_a_home_query() {
    let backend = ScriptedSessionBackend::new().expect("backend");
    let (_, models) = watch::channel(Vec::new());
    let port = Arc::new(ScriptedCatalogue {
        requests: Mutex::new(Vec::new()),
        result: CatalogueResult::Ready {
            report: Box::new(report_with_skills(Vec::new())),
        },
    });
    let context = Arc::new(
        RouterSessionAppServerContext::new(
            backend.endpoint.clone(),
            ScriptedSessionBackend::actor().expect("actor"),
            backend.clone(),
            backend,
            models,
        )
        .with_native_catalogue(port.clone(), None),
    );
    let response = handle_app_server_rpc(
        context,
        json!({
            "id":4,"method":"skills/list","params":{"cwds":[]}
        }),
    )
    .await
    .expect("dispatch")
    .response;
    assert_eq!(
        response.pointer("/error/message"),
        Some(&json!("native catalogue scope unavailable"))
    );
    assert!(port.requests.lock().expect("requests").is_empty());
}
