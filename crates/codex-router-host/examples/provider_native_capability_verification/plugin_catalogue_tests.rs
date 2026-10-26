//! J1 plugin oracles: the installed plugin list, marketplace manifests, `plugin/read`
//! details, management rejection and the installed plugin skill sample, against
//! known-good and wrong evidence. Apparatus sensitivity only, never product proof.
use crate::catalogue_oracle::{
    check_installed_plugin_skill_sample, check_management_rejected, check_plugin_catalogue,
    check_plugin_detail,
};
use crate::native_file_facts::{native_plugin_facts, plugin_namespace, plugin_skill_files};
use crate::oracle_test_support::{
    NativeSetup, SKILL, error_reply, expect_error, result_reply, skill_row, skills_reply,
    write_skill,
};
use crate::verification_models::{
    CapabilityStatus, NativeMarketplace, NativePluginFacts, NativePluginRow, RpcReply,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};
use tempfile::TempDir;

#[test]
fn installed_plugin_skill_must_appear_by_its_own_path() {
    let setup = NativeSetup::new();
    let install = setup.home.join(".claude/plugins/cache/market/alpha/1.0.0");
    let skill_file = write_skill(&install.join("skills"), "review", "");
    let row = plugin_row("alpha@market", true, install, None);
    let present = skills_reply(
        &setup.workspace,
        vec![
            skill_row(SKILL, &setup.skill_file, "repo", true),
            skill_row("alpha:review", &skill_file, "user", true),
        ],
        vec![],
    );
    assert_eq!(
        check_installed_plugin_skill_sample(
            &present,
            &setup.workspace,
            &row,
            std::slice::from_ref(&skill_file)
        )
        .unwrap(),
        skill_file.display().to_string()
    );
    let other_plugin_skill = write_skill(
        &setup
            .home
            .join(".claude/plugins/cache/market/beta/1.0.0/skills"),
        "review",
        "",
    );
    let wrong = [
        skills_reply(
            &setup.workspace,
            vec![skill_row(SKILL, &setup.skill_file, "repo", true)],
            vec![],
        ),
        skills_reply(
            &setup.workspace,
            vec![skill_row("beta:review", &other_plugin_skill, "user", true)],
            vec![],
        ),
        error_reply(),
    ];
    for reply in wrong {
        assert!(
            check_installed_plugin_skill_sample(
                &reply,
                &setup.workspace,
                &row,
                std::slice::from_ref(&skill_file)
            )
            .is_err()
        );
    }
}

fn plugin_row(
    id: &str,
    enabled: bool,
    install_path: PathBuf,
    read_from_folder: Option<PathBuf>,
) -> NativePluginRow {
    NativePluginRow {
        id: id.to_owned(),
        enabled,
        install_path,
        read_from_folder,
        version: None,
        scope: None,
    }
}

#[test]
fn active_package_folder_is_read_from_folder_when_the_provider_overrides_it() {
    // Claude's export: `readFromFolder` overrides `installPath` as the loaded package.
    let directory = TempDir::new().unwrap();
    let stale = directory.path().join("cache/alpha/1.0.0");
    let active = directory.path().join("checkout/alpha");
    write_skill(&stale.join("skills"), "stale", "");
    let review = write_skill(&active.join("skills"), "review", "");
    fs::create_dir_all(active.join(".claude-plugin")).unwrap();
    fs::write(
        active.join(".claude-plugin/plugin.json"),
        r#"{"name": "alpha-dev"}"#,
    )
    .unwrap();
    let row = plugin_row("alpha@market", true, stale, Some(active));
    assert_eq!(plugin_skill_files(&row), vec![review]);
    assert_eq!(
        native_plugin_facts(&row)
            .unwrap()
            .skill_descriptions
            .keys()
            .collect::<Vec<_>>(),
        ["review"]
    );
    // The namespace is the active manifest's own name, not a guess from the key.
    assert_eq!(plugin_namespace(&row).as_deref(), Some("alpha-dev"));
}

#[test]
fn special_native_keys_are_not_marketplace_packages() {
    let install = PathBuf::from("/nonexistent/plugin");
    for special in [
        "alpha@npm",
        "session[abc]@market",
        "a@b@c",
        "alpha@",
        "@market",
        "alpha",
    ] {
        let row = plugin_row(special, true, install.clone(), None);
        assert_eq!(row.regular_key(), None, "{special}");
        assert_eq!(plugin_namespace(&row), None, "{special}");
    }
    assert_eq!(
        plugin_row("alpha@market", true, install, None).regular_key(),
        Some(("alpha", "market"))
    );
}

/// A native install directory, its marketplace manifest and an unrelated one.
struct PluginSetup {
    _directory: TempDir,
    manifest: PathBuf,
    other_manifest: PathBuf,
    package_file: PathBuf,
    native: Vec<NativePluginRow>,
    marketplaces: Vec<NativeMarketplace>,
}

impl PluginSetup {
    fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let root = fs::canonicalize(directory.path()).unwrap();
        let install = root.join("cache/market/alpha/1.0.0");
        let manifest = root.join("marketplaces/market/.claude-plugin/marketplace.json");
        let other_manifest = root.join("marketplaces/other/.claude-plugin/marketplace.json");
        for file in [&manifest, &other_manifest] {
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, "{}").unwrap();
        }
        let package_file = install.join(".claude-plugin/plugin.json");
        fs::create_dir_all(package_file.parent().unwrap()).unwrap();
        fs::write(&package_file, "{}").unwrap();
        Self {
            native: vec![
                plugin_row("alpha@market", true, install, None),
                plugin_row(
                    "beta@market",
                    false,
                    root.join("cache/market/beta/1.0.0"),
                    None,
                ),
            ],
            // The provider's own marketplace export: each name with its actual manifest.
            marketplaces: vec![
                NativeMarketplace {
                    name: "market".to_owned(),
                    manifest_path: Some(manifest.clone()),
                },
                NativeMarketplace {
                    name: "other".to_owned(),
                    manifest_path: Some(other_manifest.clone()),
                },
            ],
            _directory: directory,
            manifest,
            other_manifest,
            package_file,
        }
    }

    fn reply(
        &self,
        plugins: Vec<Value>,
        name: &str,
        errors: Vec<Value>,
        path: Option<&PathBuf>,
    ) -> RpcReply {
        result_reply(json!({
            "marketplaces": [{"name": name, "path": path.unwrap_or(&self.manifest), "plugins": plugins}],
            "marketplaceLoadErrors": errors,
        }))
    }
}

fn summary(plugin_id: &str, enabled: bool) -> Value {
    json!({
        "id": plugin_id,
        "name": plugin_id.split('@').next().unwrap(),
        "installed": true,
        "enabled": enabled,
        "installPolicy": "AVAILABLE",
        "authPolicy": "ON_INSTALL",
        "remotePluginId": null,
        "shareContext": null,
    })
}

fn both() -> Vec<Value> {
    vec![summary("alpha@market", true), summary("beta@market", false)]
}

#[test]
fn exact_native_installation_is_supported() {
    let setup = PluginSetup::new();
    let result = check_plugin_catalogue(
        &setup.reply(both(), "market", vec![], None),
        &setup.native,
        &setup.marketplaces,
    )
    .unwrap();
    assert_eq!(result.installed_ids, ["alpha@market", "beta@market"]);
    assert_eq!(result.defaulted_policy_ids, ["alpha@market", "beta@market"]);
}

#[test]
fn wrong_lists_are_rejected() {
    let setup = PluginSetup::new();
    let mut remote = summary("alpha@market", true);
    remote["remotePluginId"] = json!("remote-1");
    let mut foreign = both();
    foreign.push(summary("codex@openai", true));
    let wrong = [
        setup.reply(vec![summary("alpha@market", true)], "market", vec![], None),
        setup.reply(foreign, "market", vec![], None),
        setup.reply(
            vec![
                summary("alpha@market", false),
                summary("beta@market", false),
            ],
            "market",
            vec![],
            None,
        ),
        setup.reply(
            vec![remote, summary("beta@market", false)],
            "market",
            vec![],
            None,
        ),
        setup.reply(both(), "guessed", vec![], None),
    ];
    for reply in wrong {
        assert!(check_plugin_catalogue(&reply, &setup.native, &setup.marketplaces).is_err());
    }
}

#[test]
fn special_native_keys_are_reported_unqualified_and_never_listed() {
    let setup = PluginSetup::new();
    let mut native = setup.native.clone();
    native.push(plugin_row(
        "helper@npm",
        true,
        PathBuf::from("/nonexistent/npm"),
        None,
    ));
    let result = check_plugin_catalogue(
        &setup.reply(both(), "market", vec![], None),
        &native,
        &setup.marketplaces,
    )
    .unwrap();
    assert_eq!(result.unqualified_native_ids, ["helper@npm"]);
    let mut listed = both();
    listed.push(summary("helper@npm", true));
    expect_error(
        check_plugin_catalogue(
            &setup.reply(listed, "market", vec![], None),
            &native,
            &setup.marketplaces,
        ),
        "special native key",
    );
}

#[test]
fn unrelated_real_error_path_excuses_no_omission() {
    let setup = PluginSetup::new();
    let reply = setup.reply(
        vec![summary("alpha@market", true)],
        "market",
        vec![json!({"marketplacePath": setup.other_manifest, "message": "other"})],
        None,
    );
    expect_error(
        check_plugin_catalogue(&reply, &setup.native, &setup.marketplaces),
        "omits installed plugin beta",
    );
}

#[test]
fn only_the_actual_marketplace_manifest_qualifies() {
    let setup = PluginSetup::new();
    let package_error = vec![json!({"marketplacePath": setup.package_file, "message": "beta"})];
    let wrong = [
        // A real package file is the wrong source role for a marketplace error.
        setup.reply(
            vec![summary("alpha@market", true)],
            "market",
            package_error.clone(),
            None,
        ),
        // A real file that is not this marketplace's manifest.
        setup.reply(both(), "market", vec![], Some(&setup.package_file)),
        setup.reply(both(), "market", vec![], Some(&setup.other_manifest)),
    ];
    for reply in wrong {
        assert!(check_plugin_catalogue(&reply, &setup.native, &setup.marketplaces).is_err());
    }
    // Nothing omitted: the wrong source role alone must fail the list.
    expect_error(
        check_plugin_catalogue(
            &setup.reply(both(), "market", package_error, None),
            &setup.native,
            &setup.marketplaces,
        ),
        "names no actual manifest",
    );
    let unexported = [NativeMarketplace {
        name: "market".to_owned(),
        manifest_path: None,
    }];
    expect_error(
        check_plugin_catalogue(
            &setup.reply(both(), "market", vec![], None),
            &setup.native,
            &unexported,
        ),
        "no manifest in the provider export",
    );
}

#[test]
fn own_marketplace_error_explains_its_omission() {
    let setup = PluginSetup::new();
    let reply = setup.reply(
        vec![summary("alpha@market", true)],
        "market",
        vec![json!({"marketplacePath": setup.manifest, "message": "beta"})],
        None,
    );
    assert_eq!(
        check_plugin_catalogue(&reply, &setup.native, &setup.marketplaces)
            .unwrap()
            .status,
        CapabilityStatus::Supported
    );
}

#[test]
fn management_must_be_rejected_without_native_change() {
    let setup = PluginSetup::new();
    check_management_rejected(&error_reply(), &setup.native, &setup.native).unwrap();
    assert!(
        check_management_rejected(&result_reply(json!({})), &setup.native, &setup.native).is_err()
    );
    assert!(check_management_rejected(&error_reply(), &setup.native, &setup.native[..1]).is_err());
}

fn detail(overrides: Value) -> RpcReply {
    let mut plugin = json!({
        "marketplaceName": "market",
        "summary": summary("alpha@market", true),
        "description": "Install and auth policies are Codex display defaults.",
        "skills": [{"name": "alpha:review", "description": "Review code", "enabled": true}],
        "hooks": [{"key": "native-1", "eventName": "preToolUse"}],
        "mcpServers": ["search"],
    });
    for (key, value) in overrides.as_object().unwrap() {
        plugin[key] = value.clone();
    }
    result_reply(json!({"plugin": plugin}))
}

fn facts(hooks: Option<Vec<String>>) -> NativePluginFacts {
    NativePluginFacts {
        plugin_id: "alpha@market".to_owned(),
        skill_descriptions: BTreeMap::from([("review".to_owned(), "Review code".to_owned())]),
        hook_events: hooks,
        mcp_servers: Some(vec!["search".to_owned()]),
    }
}

#[test]
fn detail_items_from_native_facts_are_supported() {
    let setup = PluginSetup::new();
    let status = check_plugin_detail(
        &detail(json!({})),
        &setup.native[0],
        &facts(Some(vec!["PreToolUse".to_owned()])),
    );
    assert_eq!(status.unwrap(), CapabilityStatus::Supported);
}

#[test]
fn invented_or_dropped_detail_items_are_rejected() {
    let setup = PluginSetup::new();
    let wrong = [
        json!({"skills": [{"name": "review", "description": "Invented", "enabled": true}]}),
        json!({"skills": []}),
        json!({"hooks": []}),
        json!({"hooks": [{"key": "k", "eventName": "sessionStart"}]}),
        json!({"mcpServers": []}),
        json!({"description": "No attribution"}),
        json!({"marketplaceName": "guessed"}),
    ];
    for overrides in wrong {
        let reply = detail(overrides.clone());
        assert!(
            check_plugin_detail(
                &reply,
                &setup.native[0],
                &facts(Some(vec!["PreToolUse".to_owned()]))
            )
            .is_err(),
            "{overrides}"
        );
    }
}

#[test]
fn unobserved_hooks_require_the_method_error() {
    let setup = PluginSetup::new();
    expect_error(
        check_plugin_detail(
            &detail(json!({"hooks": []})),
            &setup.native[0],
            &facts(None),
        ),
        "hooks",
    );
    assert!(matches!(
        check_plugin_detail(&error_reply(), &setup.native[0], &facts(None)).unwrap(),
        CapabilityStatus::ExplicitUnsupported { .. }
    ));
}
