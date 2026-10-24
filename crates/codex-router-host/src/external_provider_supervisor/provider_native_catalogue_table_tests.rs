//! Decision-table rows through the actual private socket, Host and file reader.
//! Export/ACP processes are contract stand-ins, not installed-provider proof.
use super::*;

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
