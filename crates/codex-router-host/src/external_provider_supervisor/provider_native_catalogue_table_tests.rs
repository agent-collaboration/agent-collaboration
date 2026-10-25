//! Decision-table rows through the actual private socket, Host and file reader.
//! Export/ACP processes are contract stand-ins, not installed-provider proof.
use super::*;

#[tokio::test]
async fn native_capabilities_socket_plugin_read_rejects_unqualified_scope_without_native_dispatch()
{
    for (provider, expected_reason) in [
        (
            ProviderKind::ClaudeCode,
            CatalogueUnsupported::ScopeUnavailable,
        ),
        (
            ProviderKind::Cursor,
            CatalogueUnsupported::NativeOperationMissing,
        ),
    ] {
        let mut fixture = CatalogueFixture::new(provider).await;
        let selector_path = fixture
            ._directory
            .path()
            .join("unread-selector/marketplace.json");
        let request = NativePluginRead {
            endpoint: fixture.board_endpoint.clone(),
            selector: NativePluginSelector {
                marketplace: NativeMarketplaceSelector::Filesystem {
                    manifest_path: CapabilityPath::new(selector_path.clone())
                        .expect("absolute selector"),
                },
                plugin_name: CapabilityName::new("not-installed".into()).expect("plugin name"),
            },
            actor: fixture.read_request(CatalogueQuery::Home).actor,
        };
        let before = fs::read_to_string(&fixture.trace).expect("initial ACP trace");

        let direct = fixture.supervisor.read_plugin(request.clone()).await;
        let mut foreign = request;
        foreign.endpoint.endpoint_id =
            message_board::EndpointId::try_from("foreign-provider".to_owned())
                .expect("foreign endpoint");
        let rejected_foreign = fixture.supervisor.read_plugin(foreign).await;
        let response = fixture
            .rpc_params(
                "plugin/read",
                json!({
                    "marketplacePath": selector_path, "pluginName": "not-installed"
                }),
            )
            .await;

        assert_eq!(
            direct,
            NativePluginReadResult::Unsupported {
                reason: expected_reason
            }
        );
        assert_eq!(
            rejected_foreign,
            NativePluginReadResult::Failed {
                reason: CatalogueReadFailure::InvalidScope,
            }
        );
        assert_eq!(response["error"]["code"], -32000);
        assert_eq!(
            response["error"]["message"],
            "native capability discovery is unsupported for this provider binding"
        );
        assert!(response.get("result").is_none(), "{response}");
        fixture.assert_no_native_export();
        assert_eq!(
            fs::read_to_string(&fixture.trace).expect("final ACP trace"),
            before
        );
        assert!(
            !selector_path.exists(),
            "selector must not create or qualify a setup"
        );
        fixture.finish().await;
    }
}

#[tokio::test]
async fn native_capabilities_socket_user_scope_cannot_qualify_reserved_synced_repository() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    let manifest = fixture.install_package_fixture(false);
    let package = manifest
        .parent()
        .expect("manifest directory")
        .parent()
        .expect("package");
    write(
        &package.join("skills/native/SKILL.md"),
        "Native package skill",
    );
    // A real ordinary package/market remains independently qualified.
    let ordinary = fixture.rpc("skills/list").await;
    assert_eq!(
        ordinary["result"]["data"][0]["skills"][0]["name"],
        "fixture-package:native"
    );
    write(
        &fixture._directory.path().join("marketplace.json"),
        r#"{"name":"synced","plugins":[{"name":"fixture-package"}]}"#,
    );
    write(&fixture.config.join("plugins/known_marketplaces.json"), &json!({"synced":{"installLocation":fixture._directory.path().join("marketplace.json"),"source":{"source":"file"}}}).to_string());
    write(&fixture.export, &json!([{"id":"fixture-package@synced","version":"1","scope":"user","enabled":true,"installPath":package}]).to_string());
    for method in ["skills/list", "plugin/list"] {
        let response = fixture.rpc(method).await;
        assert_eq!(
            response["error"]["message"],
            "native filesystem catalogue cannot be represented completely: incomplete native filesystem view: partial native export (1 limitations)",
            "{method}: {response}"
        );
        assert!(response.get("result").is_none());
    }
    fixture.finish().await;
}

#[tokio::test]
async fn native_capabilities_socket_synced_scope_does_not_gain_regular_package_availability() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    fixture.install_package_fixture(false);
    let mut exported: Vec<Value> =
        serde_json::from_str(&fs::read_to_string(&fixture.export).expect("native export"))
            .expect("rows");
    exported.first_mut().expect("package row")["scope"] = json!("synced");
    write(
        &fixture.export,
        &serde_json::to_string(&exported).expect("export JSON"),
    );
    for method in ["skills/list", "plugin/list"] {
        let response = fixture.rpc(method).await;
        assert_eq!(
            response["error"]["message"],
            "native filesystem catalogue cannot be represented completely: incomplete native filesystem view: partial native export (1 limitations)"
        );
        assert!(response.get("result").is_none());
    }
    fixture.finish().await;
}

#[tokio::test]
async fn native_capabilities_socket_lsp_only_present_manifest_keeps_independent_skill_coverage() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    let manifest = fixture.install_package_fixture(false);
    let package = manifest
        .parent()
        .expect("manifest directory")
        .parent()
        .expect("package");
    write(
        &manifest,
        r#"{"name":"fixture-package","strict":false,"lspServers":{"native-lsp":{"command":"unused"}}}"#,
    );
    write(
        &fixture._directory.path().join("marketplace.json"),
        r#"{"name":"fixture-market","plugins":[{"name":"fixture-package","strict":false,"lspServers":{"native-lsp":{"command":"unused"}}}]}"#,
    );
    write(
        &fixture.workspace.join(".claude/skills/healthy/SKILL.md"),
        "Native healthy ordinary skill",
    );
    let read = fixture
        .supervisor
        .read_catalogue(fixture.read_request(CatalogueQuery::Workspace {
            cwd: CapabilityPath::new(fixture.workspace.clone()).expect("cwd"),
        }))
        .await;
    let CatalogueResult::Ready { report } = read else {
        panic!("native sections")
    };
    let CatalogueSection::Supported { items: plugins, .. } = report.plugins else {
        panic!("package section")
    };
    let plugin = plugins.first().expect("installed package");
    assert!(
        matches!(&plugin.components.skills, NativeComponentSection::Observed { items, coverage: CatalogueCoverage::Complete } if items.is_empty())
    );
    assert!(
        matches!(plugin.components.other, NativeComponentSection::NotObserved),
        "LSP semantics remain independently unqualified"
    );
    let empty_skills = fixture.rpc("skills/list").await;
    assert_eq!(
        empty_skills["result"]["data"][0]["skills"]
            .as_array()
            .expect("ordinary skill survives")
            .len(),
        1
    );
    assert_eq!(empty_skills["result"]["data"][0]["errors"], json!([]));
    write(
        &package.join("skills/installed/SKILL.md"),
        "Native installed skill",
    );
    let native_skills = fixture.rpc("skills/list").await;
    let rows = native_skills["result"]["data"][0]["skills"]
        .as_array()
        .expect("native default root remains usable");
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .any(|row| row["name"] == "fixture-package:installed")
    );
    assert_eq!(native_skills["result"]["data"][0]["errors"], json!([]));
    fixture.finish().await;
}

#[tokio::test]
async fn native_capabilities_socket_empty_and_clean_native_sections() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    let empty = fixture.rpc("plugin/list").await;
    assert_eq!(empty["result"]["marketplaces"], json!([]));
    assert_eq!(empty["result"]["marketplaceLoadErrors"], json!([]));
    fixture.install_package_fixture(false);
    write(
        &fixture.workspace.join(".claude/skills/healthy/SKILL.md"),
        "Native healthy skill",
    );
    let skills = fixture.rpc("skills/list").await;
    assert_eq!(skills["result"]["data"][0]["skills"][0]["name"], "healthy");
    assert_eq!(skills["result"]["data"][0]["errors"], json!([]));
    let plugins = fixture.rpc("plugin/list").await;
    assert_eq!(
        plugins["result"]["marketplaces"][0]["plugins"][0]["name"],
        "fixture-package"
    );
    assert_eq!(plugins["result"]["marketplaceLoadErrors"], json!([]));
    fixture.finish().await;
}

#[tokio::test]
async fn native_capabilities_socket_located_marketplace_error_retains_healthy_package() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    fixture.install_package_fixture(false);
    let broken_marketplace = fixture._directory.path().join("broken-marketplace.json");
    write(&broken_marketplace, "{ malformed native marketplace");
    let broken_package = fixture._directory.path().join("broken-package");
    fs::create_dir_all(&broken_package).expect("candidate package");
    write(&fixture.config.join("plugins/known_marketplaces.json"), &json!({
        "fixture-market":{"installLocation":fixture._directory.path().join("marketplace.json"),"source":{"source":"file"}},
        "broken-market":{"installLocation":broken_marketplace,"source":{"source":"file"}}
    }).to_string());
    let mut exported: Vec<Value> =
        serde_json::from_str(&fs::read_to_string(&fixture.export).expect("export fixture"))
            .expect("export rows");
    exported.push(json!({"id":"broken@broken-market","version":"1","scope":"user","enabled":true,"installPath":broken_package}));
    write(
        &fixture.export,
        &serde_json::to_string(&exported).expect("native export"),
    );
    let plugins = fixture.rpc("plugin/list").await;
    assert_eq!(
        plugins["result"]["marketplaces"][0]["plugins"][0]["name"],
        "fixture-package"
    );
    assert_eq!(
        plugins["result"]["marketplaceLoadErrors"][0]["marketplacePath"],
        json!(broken_marketplace)
    );
    assert_eq!(
        plugins["result"]["marketplaceLoadErrors"]
            .as_array()
            .expect("located errors")
            .len(),
        1
    );
    fixture.finish().await;
}

#[tokio::test]
async fn native_capabilities_socket_package_path_is_not_a_marketplace_error_beside_healthy_row() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    fixture.install_package_fixture(false);
    let broken_package = fixture._directory.path().join("broken-package");
    write(
        &broken_package.join(".claude-plugin/plugin.json"),
        "{ malformed present package",
    );
    write(
        &fixture._directory.path().join("marketplace.json"),
        r#"{"name":"fixture-market","plugins":[{"name":"fixture-package"},{"name":"broken"}]}"#,
    );
    let mut exported: Vec<Value> =
        serde_json::from_str(&fs::read_to_string(&fixture.export).expect("export fixture"))
            .expect("rows");
    exported.push(json!({"id":"broken@fixture-market","version":"1","scope":"user","enabled":true,"installPath":broken_package}));
    write(
        &fixture.export,
        &serde_json::to_string(&exported).expect("export"),
    );
    let response = fixture.rpc("plugin/list").await;
    assert_eq!(
        response["error"]["message"],
        "native filesystem catalogue cannot be represented completely: native source-error role or location is unrepresentable (1 entries)"
    );
    assert!(response.get("result").is_none());
    fixture.finish().await;
}

#[tokio::test]
async fn native_capabilities_socket_all_skill_candidates_unrepresentable_is_not_empty_success() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    write(
        &fixture.workspace.join(".claude/skills/broken/SKILL.md"),
        "---\nname: [broken\n---\n",
    );
    let response = fixture.rpc("skills/list").await;
    assert_eq!(
        response["error"]["message"],
        "native filesystem catalogue cannot be represented completely: all native filesystem candidates are unrepresentable (1 entries)"
    );
    assert!(response.get("result").is_none());
    fixture.finish().await;
}

#[tokio::test]
async fn native_capabilities_socket_unqualified_export_scope_is_incomplete_with_healthy_rows() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    fixture.install_package_fixture(false);
    let mut exported: Vec<Value> =
        serde_json::from_str(&fs::read_to_string(&fixture.export).expect("export fixture"))
            .expect("rows");
    exported.push(json!({"id":"unqualified@fixture-market","version":"1","scope":"project","projectPath":fixture.workspace,"enabled":true,"installPath":fixture._directory.path()}));
    write(
        &fixture.export,
        &serde_json::to_string(&exported).expect("export"),
    );
    let response = fixture.rpc("plugin/list").await;
    assert_eq!(
        response["error"]["message"],
        "native filesystem catalogue cannot be represented completely: incomplete native filesystem view: partial native export (1 limitations)"
    );
    assert!(response.get("result").is_none());
    fixture.finish().await;
}

#[tokio::test]
async fn native_capabilities_socket_cursor_plugin_unsupported_survives_skill_read_failure() {
    let mut fixture = CatalogueFixture::new(ProviderKind::Cursor).await;
    let response = fixture.rpc("plugin/list").await;
    assert_eq!(
        response["error"]["message"],
        "native capability discovery is unsupported for this provider binding"
    );
    assert!(response.get("result").is_none());
    fixture.finish().await;
}

#[tokio::test]
async fn native_capabilities_socket_plugin_skill_error_keeps_actual_source_and_package_rows() {
    let mut fixture = CatalogueFixture::new(ProviderKind::ClaudeCode).await;
    let manifest = fixture.install_package_fixture(false);
    let package = manifest
        .parent()
        .expect("manifest directory")
        .parent()
        .expect("package directory");
    let healthy = fixture.workspace.join(".claude/skills/healthy/SKILL.md");
    write(&healthy, "Healthy native source");
    let broken = package.join("skills/broken/SKILL.md");
    write(&broken, "---\nname: [broken\n---\n");
    let response = fixture.rpc("skills/list").await;
    assert_eq!(
        response["result"]["data"][0]["skills"][0]["name"],
        "healthy"
    );
    assert_eq!(
        response["result"]["data"][0]["errors"][0]["path"],
        json!(broken)
    );
    assert_eq!(
        response["result"]["data"][0]["errors"]
            .as_array()
            .expect("located errors")
            .len(),
        1
    );
    let plugins = fixture.rpc("plugin/list").await;
    assert_eq!(
        plugins["result"]["marketplaces"][0]["plugins"][0]["name"],
        "fixture-package"
    );
    fixture.finish().await;
}
