//! Plugin codec observations are separate from native reader integration proof.
use super::native_capabilities_tests::report_with_skills;
use super::test_support::ScriptedSessionBackend;
use super::*;
use crate::provider_capabilities::*;
use std::sync::Mutex;

struct PluginCatalogue {
    report: CatalogueObservation,
    plugin: NativePlugin,
    selected: Mutex<Vec<NativePluginRead>>,
    catalogue_reads: Mutex<usize>,
}
impl NativeCataloguePort for PluginCatalogue {
    fn read_catalogue(&self, request: CatalogueRead) -> CatalogueFuture<'_> {
        *self.catalogue_reads.lock().expect("read count") += 1;
        let mut report = self.report.clone();
        report.scope.query = request.query;
        Box::pin(async move {
            CatalogueResult::Ready {
                report: Box::new(report),
            }
        })
    }
    fn read_plugin(&self, request: NativePluginRead) -> NativePluginReadFuture<'_> {
        self.selected.lock().expect("selected").push(request);
        let plugin = self.plugin.clone();
        let scope = self.report.scope.clone();
        Box::pin(async move {
            NativePluginReadResult::Ready {
                scope: Box::new(scope),
                plugin: Box::new(plugin),
            }
        })
    }
}
fn empty<T>() -> NativeComponentSection<T> {
    NativeComponentSection::Observed {
        items: Vec::new(),
        coverage: CatalogueCoverage::Complete,
    }
}
pub(super) fn package() -> NativePlugin {
    NativePlugin {
        id: NativePluginId::new("opaque-native-package-key".into()).expect("id"),
        name: CapabilityName::new("native-package".into()).expect("name"),
        description: Some("Native package description".into()),
        origin: NativeOrigin::Filesystem {
            actual_path: CapabilityPath::new("/tmp/native-package".into()).expect("path"),
        },
        state: NativePluginState::InstalledDisabled,
        display_version: Some(CapabilityName::new("2.0".into()).expect("version")),
        marketplace: NativeMarketplaceObservation::Qualified {
            native_name: CapabilityName::new("native-marketplace".into()).expect("marketplace"),
            source: NativeMarketplaceSource::Filesystem {
                manifest_path: CapabilityPath::new(
                    "/tmp/marketplace/.claude-plugin/marketplace.json".into(),
                )
                .expect("manifest"),
            },
        },
        components: NativePluginComponents {
            skills: empty(),
            commands: empty(),
            mcp: empty(),
            hooks: empty(),
            other: empty(),
        },
        installation_policy: NativePolicyObservation::Undeclared,
        authentication_policy: NativePolicyObservation::Undeclared,
    }
}
async fn request(
    plugin: NativePlugin,
    method: &str,
    params: Value,
) -> (Value, Arc<PluginCatalogue>) {
    let backend = ScriptedSessionBackend::new().expect("backend");
    let (_, models) = watch::channel(Vec::new());
    let mut report = report_with_skills(Vec::new());
    report.plugins = CatalogueSection::Supported {
        items: vec![plugin.clone()],
        enumeration: SourceEnumeration::Accounted,
        errors: Vec::new(),
    };
    let port = Arc::new(PluginCatalogue {
        report,
        plugin,
        selected: Mutex::new(Vec::new()),
        catalogue_reads: Mutex::new(0),
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
    let response = handle_app_server_rpc(context, json!({"id":21,"method":method,"params":params}))
        .await
        .expect("dispatch")
        .response;
    (response, port)
}
fn selector() -> Value {
    json!({"marketplacePath":"/tmp/marketplace/.claude-plugin/marketplace.json","pluginName":"native-package"})
}

#[tokio::test]
async fn native_capabilities_plugin_list_uses_qualified_manifest_not_opaque_id_or_package_path() {
    let (response, _) = request(package(), "plugin/list", json!({})).await;
    let marketplace = &response["result"]["marketplaces"][0];
    assert_eq!(marketplace["name"], "native-marketplace");
    assert_eq!(
        marketplace["path"],
        "/tmp/marketplace/.claude-plugin/marketplace.json"
    );
    let summary = &marketplace["plugins"][0];
    assert_eq!(summary["id"], "opaque-native-package-key");
    assert_eq!(summary["enabled"], false);
    assert_eq!(summary["installed"], true);
    assert!(summary["remotePluginId"].is_null());
    assert!(summary["shareContext"].is_null());
    assert!(
        summary["interface"]["shortDescription"]
            .as_str()
            .expect("description")
            .contains("Codex display defaults")
    );
}

#[tokio::test]
async fn native_capabilities_plugin_detail_requires_each_complete_component_section() {
    let (response, port) = request(package(), "plugin/read", selector()).await;
    assert_eq!(response["result"]["plugin"]["hooks"], json!([]));
    assert_eq!(port.selected.lock().expect("selected").len(), 1);
    for unknown_kind in 0..5 {
        let mut plugin = package();
        match unknown_kind {
            0 => plugin.components.skills = NativeComponentSection::NotObserved,
            1 => plugin.components.commands = NativeComponentSection::NotObserved,
            2 => plugin.components.mcp = NativeComponentSection::NotObserved,
            3 => plugin.components.hooks = NativeComponentSection::NotObserved,
            _ => plugin.components.other = NativeComponentSection::NotObserved,
        }
        let (response, _) = request(plugin, "plugin/read", selector()).await;
        let category = match unknown_kind {
            0 => "skill component coverage",
            1 => "command component coverage",
            2 => "MCP component coverage",
            3 => "hook component coverage",
            _ => "other component coverage",
        };
        assert_eq!(response["error"]["code"], -32000);
        assert_eq!(
            response["error"]["message"],
            format!(
                "native filesystem catalogue cannot be represented completely: {category} (count unobserved)"
            )
        );
        assert!(response.get("result").is_none());
    }
}

#[tokio::test]
async fn native_capabilities_plugin_unknown_marketplace_policy_and_hook_cannot_be_defaulted() {
    let mut plugin = package();
    plugin.marketplace = NativeMarketplaceObservation::Unobserved;
    let (response, _) = request(plugin, "plugin/list", json!({})).await;
    assert_eq!(
        response["error"]["message"],
        "native filesystem catalogue cannot be represented completely: native marketplace identity or manifest unobserved (1 entries)"
    );
    assert!(response.get("result").is_none());
    let mut plugin = package();
    plugin.authentication_policy = NativePolicyObservation::Unobserved;
    let (response, _) = request(plugin, "plugin/read", selector()).await;
    assert_eq!(
        response["error"]["message"],
        "native filesystem catalogue cannot be represented completely: native installation or authentication policy (1 entries)"
    );
    assert!(response.get("result").is_none());
    let mut plugin = package();
    plugin.components.hooks = NativeComponentSection::Observed {
        items: vec![NativeHook {
            native_key: None,
            event: NativeHookEvent::NotObserved,
        }],
        coverage: CatalogueCoverage::Complete,
    };
    let (response, _) = request(plugin, "plugin/read", selector()).await;
    assert_eq!(
        response["error"]["message"],
        "native filesystem catalogue cannot be represented completely: native hook identity or event (1 entries)"
    );
    assert!(response.get("result").is_none());
}

#[tokio::test]
async fn native_capabilities_observed_hook_spelling_does_not_qualify_consumer_identity_or_event() {
    let mut plugin = package();
    plugin.components.hooks = NativeComponentSection::Observed {
        items: vec![NativeHook {
            native_key: Some(
                CapabilityName::new("native-fixture-hook-key".into()).expect("native fixture key"),
            ),
            event: NativeHookEvent::Observed {
                native_name: CapabilityName::new("PreToolUse".into()).expect("native event"),
            },
        }],
        coverage: CatalogueCoverage::Complete,
    };
    let (response, _) = request(plugin, "plugin/read", selector()).await;
    assert_eq!(response["error"]["code"], -32000);
    assert_eq!(
        response["error"]["message"],
        "native filesystem catalogue cannot be represented completely: native hook identity or event (1 entries)"
    );
    assert!(response.get("result").is_none());
}

#[tokio::test]
async fn native_capabilities_unqualified_marketplace_filter_sends_no_native_read() {
    for filter in [
        json!([]),
        json!(["vertical"]),
        json!(["local", "shared-with-me"]),
    ] {
        let (response, port) =
            request(package(), "plugin/list", json!({"marketplaceKinds":filter})).await;
        assert!(response.get("error").is_some(), "{response}");
        assert_eq!(*port.catalogue_reads.lock().expect("read count"), 0);
    }
}
