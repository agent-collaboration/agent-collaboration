use super::*;

fn write(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("fixture directory");
    fs::write(path, contents).expect("native source");
}

fn section(
    items: CatalogueSection<NativeSkill>,
) -> (
    Vec<NativeSkill>,
    SourceEnumeration,
    Vec<CatalogueSourceError>,
) {
    let CatalogueSection::Supported {
        items,
        enumeration,
        errors,
    } = items
    else {
        panic!("supported view")
    };
    (items, enumeration, errors)
}

#[test]
fn native_capabilities_claude_setup_reads_actual_roots_and_local_overrides_without_body_execution()
{
    let fixture = tempfile::tempdir_in("/tmp").expect("fixture");
    let home = fixture.path().join("home");
    let config = home.join(".claude");
    let workspace = fixture.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    write(&config.join("skills/user-skill/SKILL.md"), "# Native user");
    write(
        &workspace.join(".claude/skills/project-skill/SKILL.md"),
        "# $ARGUMENTS !`never-execute`",
    );
    write(
        &workspace.join(".claude/commands/legacy.md"),
        "# Native legacy",
    );
    write(
        &config.join("settings.json"),
        r#"{"skillOverrides":{"user-skill":"off","project-skill":"on"}}"#,
    );
    write(
        &workspace.join(".claude/settings.json"),
        r#"{"skillOverrides":{"project-skill":"off"}}"#,
    );
    write(
        &workspace.join(".claude/settings.local.json"),
        r#"{"skillOverrides":{"project-skill":"on"}}"#,
    );
    let (items, enumeration, errors) = section(
        read_native_claude_skill_section(
            &home,
            &config,
            &workspace,
            &[],
            &CancellationToken::new(),
        )
        .expect("native setup"),
    );
    assert_eq!(enumeration, SourceEnumeration::Accounted);
    assert!(errors.is_empty());
    assert_eq!(items.len(), 3);
    let user = items
        .iter()
        .find(|skill| skill.name.as_str() == "user-skill")
        .expect("home skill");
    assert_eq!(user.availability, NativeAvailability::Disabled);
    let project = items
        .iter()
        .find(|skill| skill.name.as_str() == "project-skill")
        .expect("workspace skill");
    assert_eq!(project.availability, NativeAvailability::Available);
    assert_eq!(project.description, "$ARGUMENTS !`never-execute`");
    assert!(items.iter().any(|skill| skill.name.as_str() == "legacy"));
    assert!(!workspace.join("never-execute").exists());
}

#[test]
fn native_capabilities_claude_setup_retains_real_candidate_error_and_marks_unqualified_synced_source()
 {
    let fixture = tempfile::tempdir_in("/tmp").expect("fixture");
    let home = fixture.path().join("home");
    let config = home.join(".claude");
    let workspace = fixture.path().join("workspace");
    fs::create_dir_all(&home).expect("home");
    let healthy = workspace.join(".claude/skills/healthy/SKILL.md");
    let broken = workspace.join(".claude/skills/broken/SKILL.md");
    write(&healthy, "Healthy native source");
    write(&broken, "---\nname: [broken\n---\n");
    let (items, enumeration, errors) = section(
        read_native_claude_skill_section(
            &home,
            &config,
            &workspace,
            &[],
            &CancellationToken::new(),
        )
        .expect("accounted candidates"),
    );
    assert_eq!(items.len(), 1);
    assert_eq!(enumeration, SourceEnumeration::Accounted);
    assert_eq!(
        errors,
        vec![CatalogueSourceError {
            source: NativeErrorSource::SkillSource {
                actual_path: CapabilityPath::new(fs::canonicalize(&broken).expect("actual source"))
                    .expect("path")
            },
            reason: CatalogueReadFailure::InvalidNativeData,
        }]
    );
    write(
        &config.join("skills/synced/other/SKILL.md"),
        "Unqualified sync source",
    );
    let (_, enumeration, _) = section(
        read_native_claude_skill_section(
            &home,
            &config,
            &workspace,
            &[],
            &CancellationToken::new(),
        )
        .expect("explicit partial source"),
    );
    assert!(matches!(enumeration, SourceEnumeration::Incomplete { .. }));
}

#[test]
fn native_capabilities_claude_setup_plugin_components_cannot_disappear_into_ordinary_only_success()
{
    let fixture = tempfile::tempdir_in("/tmp").expect("fixture");
    let home = fixture.path().join("home");
    let workspace = fixture.path().join("workspace");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&workspace).expect("workspace");
    let mut plugin = native_plugin_fixture(fixture.path(), false);
    let config = home.join(".claude");
    let (items, enumeration, _) = section(
        read_native_claude_skill_section(
            &home,
            &config,
            &workspace,
            std::slice::from_ref(&plugin),
            &CancellationToken::new(),
        )
        .expect("merged source"),
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].name.as_str(), "package:native");
    assert_eq!(items[0].availability, NativeAvailability::Disabled);
    assert_eq!(enumeration, SourceEnumeration::Accounted);
    plugin.components.skills = NativeComponentSection::NotObserved;
    let (_, enumeration, _) = section(
        read_native_claude_skill_section(
            &home,
            &config,
            &workspace,
            &[plugin],
            &CancellationToken::new(),
        )
        .expect("explicit gap"),
    );
    assert!(matches!(enumeration, SourceEnumeration::Incomplete { .. }));
}

#[test]
fn native_capabilities_plugin_detail_selector_cannot_infer_home_over_project_skill_override() {
    // Same installed user package/manifest is visible in both contexts. Its
    // skill availability is configured differently; selector identity contains
    // no cwd. The native files, not a candidate-provided disabled flag, supply
    // this counterexample to the Home inference.
    let fixture = tempfile::tempdir_in("/tmp").expect("fixture");
    let home = fixture.path().join("home");
    let config = home.join(".claude");
    let workspace = fixture.path().join("workspace");
    fs::create_dir_all(&workspace).expect("workspace");
    fs::create_dir_all(&home).expect("home");
    let plugin = native_plugin_fixture(fixture.path(), true);
    write(
        &config.join("settings.json"),
        r#"{"skillOverrides":{"package:native":"on"}}"#,
    );
    write(
        &workspace.join(".claude/settings.local.json"),
        r#"{"skillOverrides":{"package:native":"off"}}"#,
    );
    let (home_items, _, _) = section(
        read_native_claude_skill_section(
            &home,
            &config,
            &home,
            std::slice::from_ref(&plugin),
            &CancellationToken::new(),
        )
        .expect("Home setup"),
    );
    let (project_items, _, _) = section(
        read_native_claude_skill_section(
            &home,
            &config,
            &workspace,
            std::slice::from_ref(&plugin),
            &CancellationToken::new(),
        )
        .expect("project setup"),
    );
    assert_eq!(home_items.len(), 1);
    assert_eq!(home_items[0].name.as_str(), "package:native");
    assert_eq!(project_items.len(), 1);
    assert_eq!(home_items[0].availability, NativeAvailability::Available);
    assert_eq!(project_items[0].availability, NativeAvailability::Disabled);
    let launch = crate::ExternalProviderLaunch {
        executable: PathBuf::from("/native/adapter"),
        arguments: Vec::new(),
        environment: vec![("HOME".into(), home.to_str().expect("UTF8").into())],
        persistence_target: acp_client_runtime::ProviderPersistenceTarget::Unspecified,
    };
    let context = ProviderNativeReadContext::capture(
        collaboration_protocol::ProviderKind::ClaudeCode,
        &launch,
    );
    assert_eq!(
        context.plugin_read_workspace(),
        Err(CatalogueUnsupported::ScopeUnavailable)
    );
}

fn native_plugin_fixture(root: &Path, enabled: bool) -> NativePlugin {
    let path = root.join("plugin/skills/native/SKILL.md");
    write(&path, "Native plugin skill");
    write(
        &root.join("plugin/.claude-plugin/plugin.json"),
        r#"{"name":"package"}"#,
    );
    let mut native_skills =
        acp_client_runtime::native_capabilities::read_claude_plugin_skill_roots(
            "2.1.310",
            "package",
            &[root.join("plugin/skills")],
            ClaudeFileSkillScope::User,
            enabled,
            &CancellationToken::new(),
        )
        .expect("native plugin file reader");
    let mut skill = project_claude_file_skill(native_skills.remove(0)).expect("native skill");
    skill.plugin_id = Some(NativePluginId::new("package@market".into()).expect("native key"));
    NativePlugin {
        id: NativePluginId::new("package@market".into()).expect("package"),
        name: CapabilityName::new("package".into()).expect("name"),
        description: None,
        origin: NativeOrigin::Filesystem {
            actual_path: CapabilityPath::new(root.join("plugin")).expect("path"),
        },
        state: if enabled {
            NativePluginState::InstalledEnabled {
                loading: NativeLoadingState::NotObserved,
            }
        } else {
            NativePluginState::InstalledDisabled
        },
        display_version: None,
        marketplace: NativeMarketplaceObservation::Qualified {
            native_name: CapabilityName::new("market".into()).expect("market"),
            source: NativeMarketplaceSource::Filesystem {
                manifest_path: CapabilityPath::new(root.join("marketplace.json")).expect("path"),
            },
        },
        components: NativePluginComponents {
            skills: NativeComponentSection::Observed {
                items: vec![skill],
                coverage: CatalogueCoverage::Complete,
            },
            commands: NativeComponentSection::NotObserved,
            mcp: NativeComponentSection::NotObserved,
            hooks: NativeComponentSection::NotObserved,
            other: NativeComponentSection::NotObserved,
        },
        installation_policy: NativePolicyObservation::Undeclared,
        authentication_policy: NativePolicyObservation::Undeclared,
    }
}
