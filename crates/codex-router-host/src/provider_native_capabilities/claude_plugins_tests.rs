use super::components::qualified_plugin_skill_components;
use super::*;

#[test]
fn native_capabilities_non_strict_manifest_keeps_component_conflicts_and_unsafe_roots_unqualified()
{
    let root = tempfile::tempdir_in("/tmp").expect("fixture");
    let package = read_claude_plugin_export("2.1.310", &serde_json::to_vec(&serde_json::json!([{
        "id":"native-package@native-market","version":"1","scope":"user","enabled":true,"installPath":root.path()
    }])).expect("export JSON")).expect("native export").remove(0);
    let manifest: NativePluginManifest =
        serde_json::from_value(serde_json::json!({"name":"native-package"})).expect("manifest");
    for field in [
        "commands",
        "agents",
        "skills",
        "hooks",
        "outputStyles",
        "themes",
        "experimental",
    ] {
        let mut entry = serde_json::json!({"name":"native-package","strict":false});
        entry.as_object_mut().expect("entry").insert(
            field.into(),
            if field == "experimental" {
                serde_json::json!({"themes":["unqualified"]})
            } else {
                serde_json::json!(["unqualified"])
            },
        );
        let marketplace: NativeMarketplaceManifest =
            serde_json::from_value(serde_json::json!({"name":"native-market","plugins":[entry]}))
                .expect("marketplace");
        assert_eq!(
            qualified_plugin_skill_components(
                root.path(),
                &manifest,
                &marketplace,
                &package,
                &CancellationToken::new()
            ),
            Err(CatalogueReadFailure::InvalidScope),
            "entry component {field}"
        );
    }
    let marketplace: NativeMarketplaceManifest = serde_json::from_value(serde_json::json!({"name":"native-market","plugins":[{"name":"native-package","strict":false,"lspServers":{}}]})).expect("LSP entry");
    for declared in ["../outside", "/native/outside"] {
        let manifest: NativePluginManifest =
            serde_json::from_value(serde_json::json!({"name":"native-package","skills":declared}))
                .expect("unqualified root");
        assert_eq!(
            qualified_plugin_skill_components(
                root.path(),
                &manifest,
                &marketplace,
                &package,
                &CancellationToken::new()
            ),
            Err(CatalogueReadFailure::InvalidScope),
            "unsafe root {declared}"
        );
    }
}

#[test]
fn native_capabilities_malformed_present_package_retains_healthy_inventory_and_actual_error() {
    let fixture = tempfile::tempdir_in("/tmp").expect("native files");
    let config = fixture.path().join("config");
    let marketplace = fixture.path().join("marketplace.json");
    let healthy = fixture.path().join("healthy");
    let broken = fixture.path().join("broken");
    fs::create_dir_all(config.join("plugins")).expect("registry directory");
    for package in [&healthy, &broken] {
        fs::create_dir_all(package.join(".claude-plugin")).expect("package directory");
    }
    fs::write(&marketplace, r#"{"name":"market","plugins":[{"name":"healthy"},{"name":"broken","strict":false,"description":"Must not hide a corrupt present file"}]}"#).expect("marketplace");
    fs::write(
        healthy.join(".claude-plugin/plugin.json"),
        r#"{"name":"healthy"}"#,
    )
    .expect("healthy manifest");
    let broken_manifest = broken.join(".claude-plugin/plugin.json");
    fs::write(&broken_manifest, "{ invalid native manifest").expect("broken manifest");
    fs::write(config.join("plugins/known_marketplaces.json"), serde_json::to_vec(&serde_json::json!({"market":{"installLocation":marketplace,"source":{"source":"file"}}})).expect("registry JSON")).expect("registry");
    let exported = serde_json::to_vec(&serde_json::json!([
        {"id":"healthy@market","version":"1","scope":"user","enabled":true,"installPath":healthy},
        {"id":"broken@market","version":"1","scope":"user","enabled":true,"installPath":broken}
    ]))
    .expect("native export");
    let packages = read_claude_plugin_export("2.1.310", &exported).expect("export");
    let section = read_installed_packages(packages, &config, &CancellationToken::new())
        .expect("accounted native candidates");
    let CatalogueSection::Supported {
        items,
        enumeration,
        errors,
    } = section.plugins
    else {
        panic!("supported native package source")
    };
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name.as_str(), "healthy");
    assert_eq!(enumeration, SourceEnumeration::Accounted);
    assert_eq!(
        errors,
        vec![CatalogueSourceError {
            source: NativeErrorSource::OtherFilesystem {
                actual_path: CapabilityPath::new(broken_manifest)
                    .expect("actual malformed package file")
            },
            reason: CatalogueReadFailure::InvalidNativeData,
        }]
    );
}

#[test]
fn native_capabilities_manifestless_marketplace_package_does_not_erase_native_packages() {
    let fixture = tempfile::tempdir_in("/tmp").expect("native files");
    let config = fixture.path().join("config");
    let marketplace = fixture
        .path()
        .join("marketplace/.claude-plugin/marketplace.json");
    let ordinary = fixture.path().join("ordinary");
    let lsp_only = fixture.path().join("lsp-only");
    for directory in [
        marketplace.parent().expect("parent"),
        ordinary.join(".claude-plugin").as_path(),
        lsp_only.as_path(),
        config.join("plugins").as_path(),
    ] {
        fs::create_dir_all(directory).expect("native directory");
    }
    fs::write(&marketplace, r#"{"name":"native-market","plugins":[{"name":"ordinary"},{"name":"rust-analyzer-lsp","strict":false,"description":"Native Rust language server","lspServers":{"rust-analyzer":{"command":"rust-analyzer"}}}]}"#).expect("actual marketplace manifest");
    fs::write(
        ordinary.join(".claude-plugin/plugin.json"),
        r#"{"name":"ordinary","description":"Native ordinary package"}"#,
    )
    .expect("package manifest");
    fs::write(config.join("plugins/known_marketplaces.json"), serde_json::to_vec(&serde_json::json!({"native-market":{"installLocation":marketplace,"source":{"source":"file"}}})).expect("registry JSON")).expect("native registry");
    let exported = serde_json::to_vec(&serde_json::json!([
            {"id":"ordinary@native-market","version":"1","scope":"user","enabled":true,"installPath":ordinary},
            {"id":"rust-analyzer-lsp@native-market","version":"1","scope":"user","enabled":true,"installPath":lsp_only}
        ])).expect("native export");
    let packages =
        read_claude_plugin_export("2.1.310", &exported).expect("qualified native export");
    let section = read_installed_packages(packages, &config, &CancellationToken::new())
        .expect("native permits a marketplace-only package without plugin.json");
    let CatalogueSection::Supported {
        items: plugins,
        enumeration,
        errors,
    } = section.plugins
    else {
        panic!("supported native package source")
    };
    assert_eq!(enumeration, SourceEnumeration::Accounted);
    assert!(errors.is_empty());
    assert_eq!(plugins.len(), 2);
    let lsp = plugins
        .iter()
        .find(|plugin| plugin.name.as_str() == "rust-analyzer-lsp")
        .expect("marketplace-only package");
    assert_eq!(
        lsp.description.as_deref(),
        Some("Native Rust language server")
    );
    assert!(
        matches!(&lsp.components.skills, NativeComponentSection::Observed { items, coverage: CatalogueCoverage::Complete } if items.is_empty())
    );
    assert!(matches!(
        &lsp.components.other,
        NativeComponentSection::NotObserved
    ));
    assert!(
        matches!(&lsp.marketplace, NativeMarketplaceObservation::Qualified { source: NativeMarketplaceSource::Filesystem { manifest_path }, .. } if manifest_path.as_path() == marketplace)
    );
}

#[test]
fn native_capabilities_marketplace_source_uses_native_manifest_resolution_order() {
    let root = tempfile::tempdir_in("/tmp").expect("fixture");
    let nested = root.path().join(".claude-plugin/marketplace.json");
    fs::create_dir_all(nested.parent().expect("parent")).expect("manifest folder");
    fs::write(&nested, r#"{"name":"native-marketplace"}"#).expect("manifest");
    assert_eq!(actual_marketplace_manifest(root.path()), Ok(nested.clone()));
    assert_eq!(actual_marketplace_manifest(&nested), Ok(nested));
    assert_eq!(
        actual_marketplace_manifest(Path::new("relative")),
        Err(CatalogueReadFailure::InvalidScope)
    );
}

#[test]
fn native_capabilities_named_catalog_cannot_be_reclassified_from_an_install_location() {
    let registration = r#"{"installLocation":"/native/cache","source":{"source":"claudeai"}}"#;
    let registered =
        serde_json::from_str::<NativeMarketplaceRegistration>(registration).expect("named source");
    assert_eq!(
        registered.manifest_path(),
        Err(CatalogueReadFailure::InvalidScope)
    );
    let registration = r#"{"installLocation":"/native/market","source":{"source":"git"}}"#;
    assert!(serde_json::from_str::<NativeMarketplaceRegistration>(registration).is_ok());
}
#[test]
fn native_capabilities_plugin_component_skill_uses_actual_manifest_roots_and_native_identity() {
    let root = tempfile::tempdir_in("/tmp").expect("fixture");
    fs::create_dir_all(root.path().join("skills/alpha")).expect("skill folder");
    fs::write(
        root.path().join("skills/alpha/SKILL.md"),
        "---\nname: Display title\nuser-invocable: false\n---\n# Native alpha",
    )
    .expect("native skill");
    let manifest: NativePluginManifest = serde_json::from_value(serde_json::json!({
        "name":"native-package","skills":["./skills"]
    }))
    .expect("plugin manifest");
    let marketplace: NativeMarketplaceManifest = serde_json::from_value(serde_json::json!({
        "name":"native-marketplace","plugins":[{"name":"native-package"}]
    }))
    .expect("marketplace manifest");
    let export = serde_json::to_vec(&serde_json::json!([{
            "id":"native-package@native-marketplace","version":"1.0","scope":"user", "enabled":true,"installPath":root.path()
        }])).expect("native export");
    let packages = read_claude_plugin_export("2.1.310", &export).expect("export");
    let (skills, _) = qualified_plugin_skill_components(
        root.path(),
        &manifest,
        &marketplace,
        &packages[0],
        &CancellationToken::new(),
    )
    .expect("qualified skill sources");
    let CatalogueSection::Supported { items: skills, .. } = skills else {
        panic!("supported skill sources")
    };
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name.as_str(), "native-package:alpha");
    assert_eq!(skills[0].availability, NativeAvailability::Disabled);
    assert_eq!(
        skills[0].plugin_id.as_ref().expect("plugin id").as_str(),
        "native-package@native-marketplace"
    );
    assert!(matches!(
        skills[0].lowering,
        NativeSkillLowering::Unqualified { .. }
    ));
    let (components, _) = components::read_plugin_components(
        root.path(),
        &manifest,
        &marketplace,
        &packages[0],
        &CancellationToken::new(),
    );
    assert!(
        matches!(components.commands, NativeComponentSection::Observed { ref items, coverage: CatalogueCoverage::Complete } if items.is_empty())
    );
    assert!(
        matches!(components.hooks, NativeComponentSection::Observed { ref items, coverage: CatalogueCoverage::Complete } if items.is_empty())
    );
    assert!(
        matches!(components.mcp, NativeComponentSection::Observed { ref items, coverage: CatalogueCoverage::Complete } if items.is_empty())
    );
    assert!(
        matches!(components.other, NativeComponentSection::Observed { ref items, coverage: CatalogueCoverage::Complete } if items.is_empty())
    );
    fs::create_dir(root.path().join("hooks")).expect("hook folder");
    fs::write(root.path().join("hooks/hooks.json"), "{}").expect("present native source");
    let (components, _) = components::read_plugin_components(
        root.path(),
        &manifest,
        &marketplace,
        &packages[0],
        &CancellationToken::new(),
    );
    assert!(matches!(
        components.hooks,
        NativeComponentSection::NotObserved
    ));
    let mut entry_override = marketplace;
    entry_override.plugins[0].skills = Some(NativeSkillDirectories::One(
        "./unqualified-entry-skills".into(),
    ));
    assert_eq!(
        qualified_plugin_skill_components(
            root.path(),
            &manifest,
            &entry_override,
            &packages[0],
            &CancellationToken::new()
        ),
        Err(CatalogueReadFailure::InvalidScope)
    );
}
