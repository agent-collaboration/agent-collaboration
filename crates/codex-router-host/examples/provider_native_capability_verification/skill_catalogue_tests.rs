//! J1 skill oracles against known-good and wrong evidence.
//!
//! Each wrong case is a build the Verification Design says must fail: a substituted
//! Codex catalogue, a borrowed workspace, a wrong-role source error, a disabled row with
//! no native fact. These establish apparatus sensitivity only, never product proof.
use crate::catalogue_oracle::{
    NativeSkillAuthority, check_skill_catalogue, check_wrong_context_catalogue,
};
use crate::native_file_facts::{is_skill_source, natively_disabled};
use crate::oracle_test_support::{
    NativeSetup, SKILL, error_reply, expect_error, skill_row, skills_reply, write_skill,
};
use crate::verification_models::{CapabilityStatus, ProviderEndpoint};
use serde_json::json;
use std::{fs, path::PathBuf};

const CLAUDE: ProviderEndpoint = ProviderEndpoint::ClaudeLocal;
const CURSOR: ProviderEndpoint = ProviderEndpoint::CursorLocal;

#[test]
fn native_fixture_with_user_skill_is_supported() {
    let setup = NativeSetup::new();
    let reply = skills_reply(
        &setup.workspace,
        vec![
            skill_row(SKILL, &setup.skill_file, "repo", true),
            skill_row("user-native", &setup.user_skill, "user", true),
        ],
        vec![],
    );
    let result =
        check_skill_catalogue(&reply, &setup.fixture, &setup.authority(CLAUDE, &[], true)).unwrap();
    assert_eq!(result.status, CapabilityStatus::Supported);
    assert_eq!(result.skill_count, 2);
}

#[test]
fn shared_codex_root_loaded_by_the_provider_is_legitimate() {
    // Counterexample to a path-prefix ban: Cursor's own loader reads ~/.codex/skills.
    let setup = NativeSetup::new();
    let reply = skills_reply(
        &setup.workspace,
        vec![
            skill_row(SKILL, &setup.skill_file, "repo", true),
            skill_row("shared-native", &setup.shared_skill, "user", true),
        ],
        vec![],
    );
    let authority = setup.authority(CURSOR, &["shared-native"], true);
    assert_eq!(
        check_skill_catalogue(&reply, &setup.fixture, &authority)
            .unwrap()
            .status,
        CapabilityStatus::Supported
    );
}

#[test]
fn substituted_codex_catalogue_is_rejected() {
    let setup = NativeSetup::new();
    let reply = skills_reply(
        &setup.workspace,
        vec![
            skill_row(SKILL, &setup.skill_file, "repo", true),
            skill_row("codex-only", &setup.codex_only, "user", true),
        ],
        vec![],
    );
    expect_error(
        check_skill_catalogue(&reply, &setup.fixture, &setup.authority(CLAUDE, &[], true)),
        "not loaded by the bound provider",
    );
}

#[test]
fn advertised_name_outside_the_providers_roots_is_rejected() {
    let setup = NativeSetup::new();
    let reply = skills_reply(
        &setup.workspace,
        vec![
            skill_row(SKILL, &setup.skill_file, "repo", true),
            skill_row("shared-native", &setup.shared_skill, "user", true),
        ],
        vec![],
    );
    expect_error(
        check_skill_catalogue(
            &reply,
            &setup.fixture,
            &setup.authority(CLAUDE, &["shared-native"], false),
        ),
        "outside the provider's own roots",
    );
}

#[test]
fn real_natively_disabled_file_is_a_legitimate_disabled_row() {
    let setup = NativeSetup::new();
    let skipped = write_skill(
        &setup.workspace.join(".claude/skills"),
        "skipped",
        "user-invocable: false\n",
    );
    // The provider doesn't advertise it; its own file says the loader skips it.
    let reply = skills_reply(
        &setup.workspace,
        vec![
            skill_row(SKILL, &setup.skill_file, "repo", true),
            skill_row("skipped", &skipped, "repo", false),
        ],
        vec![],
    );
    for provider in [CLAUDE, CURSOR] {
        let result = check_skill_catalogue(
            &reply,
            &setup.fixture,
            &setup.authority(provider, &[], true),
        )
        .unwrap();
        assert_eq!(result.skill_count, 2);
    }
}

#[test]
fn disabled_spellings_follow_the_bound_providers_own_parser() {
    // Claude 2.1.310 reads no/off/0 as false; Cursor accepts only "false". A row the
    // provider does not treat as disabled can't be reported disabled.
    let setup = NativeSetup::new();
    for spelling in ["no", "off", "0", "\"  FALSE \"", "'false'"] {
        let name = format!(
            "skipped-{}",
            spelling
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .collect::<String>()
        );
        let skipped = write_skill(
            &setup.workspace.join(".claude/skills"),
            &name,
            &format!("user-invocable: {spelling}\n"),
        );
        let reply = skills_reply(
            &setup.workspace,
            vec![
                skill_row(SKILL, &setup.skill_file, "repo", true),
                skill_row(&name, &skipped, "repo", false),
            ],
            vec![],
        );
        assert!(
            natively_disabled(&skipped, CLAUDE),
            "Claude reads {spelling} as false"
        );
        check_skill_catalogue(&reply, &setup.fixture, &setup.authority(CLAUDE, &[], true)).unwrap();
        let cursor_disables = natively_disabled(&skipped, CURSOR);
        assert_eq!(
            cursor_disables,
            spelling.to_lowercase().contains("false"),
            "{spelling}"
        );
        if !cursor_disables {
            expect_error(
                check_skill_catalogue(&reply, &setup.fixture, &setup.authority(CURSOR, &[], true)),
                "without a qualified native disabled fact",
            );
        }
    }
}

#[test]
fn falsely_disabled_or_enabled_rows_are_rejected() {
    let setup = NativeSetup::new();
    let skipped = write_skill(
        &setup.workspace.join(".claude/skills"),
        "skipped",
        "user-invocable: false\n",
    );
    let authority = setup.authority(CURSOR, &["skipped"], false);
    let wrong = [
        // A foreign Codex skill marked disabled is still outside the roots.
        (
            skill_row("codex-only", &setup.codex_only, "user", false),
            "roots",
        ),
        // A real native skill marked disabled without any native disabled fact.
        (
            skill_row("user-native", &setup.user_skill, "user", false),
            "without a qualified native disabled fact",
        ),
        // A natively skipped file reported as enabled, even if a name matches.
        (
            skill_row("skipped", &skipped, "repo", true),
            "reported enabled",
        ),
    ];
    for (row, fragment) in wrong {
        let reply = skills_reply(
            &setup.workspace,
            vec![skill_row(SKILL, &setup.skill_file, "repo", true), row],
            vec![],
        );
        expect_error(
            check_skill_catalogue(&reply, &setup.fixture, &authority),
            fragment,
        );
    }
}

#[test]
fn disabled_file_never_shadows_an_active_definition() {
    let setup = NativeSetup::new();
    let active = write_skill(&setup.home.join(".claude/skills"), "dup", "");
    let disabled = write_skill(
        &setup.workspace.join(".claude/skills"),
        "dup",
        "user-invocable: false\n",
    );
    let authority = setup.authority(CURSOR, &["dup"], true);
    let both = skills_reply(
        &setup.workspace,
        vec![
            skill_row(SKILL, &setup.skill_file, "repo", true),
            skill_row("dup", &disabled, "repo", false),
            skill_row("dup", &active, "user", true),
        ],
        vec![],
    );
    assert_eq!(
        check_skill_catalogue(&both, &setup.fixture, &authority)
            .unwrap()
            .skill_count,
        3
    );
    let shadowed = skills_reply(
        &setup.workspace,
        vec![
            skill_row(SKILL, &setup.skill_file, "repo", true),
            skill_row("dup", &disabled, "repo", false),
        ],
        vec![],
    );
    expect_error(
        check_skill_catalogue(&shadowed, &setup.fixture, &authority),
        "shadows the active native",
    );
}

#[test]
fn plugin_skill_needs_its_own_plugins_namespace() {
    let setup = NativeSetup::new();
    let plugins = setup.home.join(".claude/plugins/cache/market");
    let beta_review = write_skill(&plugins.join("beta/1.0.0/skills"), "review", "");
    let reply = skills_reply(
        &setup.workspace,
        vec![
            skill_row(SKILL, &setup.skill_file, "repo", true),
            skill_row("review", &beta_review, "user", true),
        ],
        vec![],
    );
    let authority = |advertised: &str| NativeSkillAuthority {
        provider: CLAUDE,
        advertised_names: [SKILL, advertised].map(str::to_owned).into_iter().collect(),
        home_skill_roots: Vec::new(),
        workspace_skill_dirs: vec![".claude/skills".to_owned()],
        home_command_roots: Vec::new(),
        workspace_command_dirs: Vec::new(),
        project_root_marker: None,
        plugin_namespaces: vec![
            (plugins.join("alpha/1.0.0"), "alpha".to_owned()),
            (plugins.join("beta/1.0.0"), "beta".to_owned()),
        ],
    };
    // Another plugin's same-named skill must not vouch for this one.
    expect_error(
        check_skill_catalogue(&reply, &setup.fixture, &authority("alpha:review")),
        "not loaded by the bound provider",
    );
    assert_eq!(
        check_skill_catalogue(&reply, &setup.fixture, &authority("beta:review"))
            .unwrap()
            .status,
        CapabilityStatus::Supported
    );
}

#[test]
fn workspace_roots_follow_the_providers_project_root() {
    let setup = NativeSetup::new();
    let nested = setup.workspace.join("packages/app");
    fs::create_dir_all(&nested).unwrap();
    let authority = NativeSkillAuthority {
        provider: CURSOR,
        advertised_names: Default::default(),
        home_skill_roots: Vec::new(),
        workspace_skill_dirs: vec![".cursor/skills".to_owned()],
        home_command_roots: Vec::new(),
        workspace_command_dirs: Vec::new(),
        project_root_marker: Some(".git".to_owned()),
        plugin_namespaces: Vec::new(),
    };
    // No marker anywhere: the native finder falls back to the cwd itself.
    assert_eq!(
        authority.skill_roots(&nested),
        vec![nested.join(".cursor/skills")]
    );
    fs::create_dir(setup.workspace.join(".git")).unwrap();
    assert_eq!(
        authority.skill_roots(&nested),
        vec![setup.workspace.join(".cursor/skills")]
    );
}

#[test]
fn empty_or_wrong_fixture_rows_are_rejected() {
    let setup = NativeSetup::new();
    let wrong_rows = [
        vec![],
        vec![skill_row(SKILL, &setup.second_file, "repo", true)],
        vec![skill_row(SKILL, &setup.skill_file, "repo", false)],
        vec![skill_row(SKILL, &setup.skill_file, "user", true)],
        vec![skill_row(
            SKILL,
            &PathBuf::from("/builtin").join(SKILL),
            "repo",
            true,
        )],
    ];
    for rows in wrong_rows {
        let reply = skills_reply(&setup.workspace, rows, vec![]);
        assert!(
            check_skill_catalogue(&reply, &setup.fixture, &setup.authority(CLAUDE, &[], true))
                .is_err()
        );
    }
}

#[test]
fn skill_errors_must_name_an_actual_skill_source() {
    let setup = NativeSetup::new();
    let manifest = setup
        .home
        .join("marketplaces/market/.claude-plugin/marketplace.json");
    fs::create_dir_all(manifest.parent().unwrap()).unwrap();
    fs::write(&manifest, "{}").unwrap();
    let authority = setup.authority(CLAUDE, &[], false);
    let wrong_roles = [
        PathBuf::from("builtin"),
        setup.workspace.clone(),  // a real package/workspace directory
        manifest,                 // the plugin role, not a skill source
        setup.codex_only.clone(), // a skill file outside the provider's roots
    ];
    for path in wrong_roles {
        let reply = skills_reply(
            &setup.workspace,
            vec![skill_row(SKILL, &setup.skill_file, "repo", true)],
            vec![json!({"path": path, "message": "unreadable"})],
        );
        expect_error(
            check_skill_catalogue(&reply, &setup.fixture, &authority),
            "no actual native skill source",
        );
    }
    let located = skills_reply(
        &setup.workspace,
        vec![skill_row(SKILL, &setup.skill_file, "repo", true)],
        vec![json!({"path": setup.second_file, "message": "unreadable"})],
    );
    assert_eq!(
        check_skill_catalogue(&located, &setup.fixture, &authority)
            .unwrap()
            .error_count,
        1
    );
}

#[test]
fn legacy_command_markdown_is_a_skill_source_only_under_command_roots() {
    // Claude loads `.claude/commands/**/*.md` as skills named like `group:task`.
    let setup = NativeSetup::new();
    let commands = setup.workspace.join(".claude/commands");
    let legacy = commands.join("group/task.md");
    fs::create_dir_all(legacy.parent().unwrap()).unwrap();
    fs::write(&legacy, "---\ndescription: legacy\n---\nBody\n").unwrap();
    let mut authority = setup.authority(CLAUDE, &["group:task"], true);
    let skill_roots = authority.skill_roots(&setup.workspace);
    assert!(!is_skill_source(
        &legacy,
        &skill_roots,
        &authority.command_roots(&setup.workspace)
    ));
    authority.workspace_command_dirs = vec![".claude/commands".to_owned()];
    assert!(is_skill_source(
        &legacy,
        &skill_roots,
        &authority.command_roots(&setup.workspace)
    ));
    let reply = skills_reply(
        &setup.workspace,
        vec![
            skill_row(SKILL, &setup.skill_file, "repo", true),
            skill_row("group:task", &legacy, "repo", true),
        ],
        vec![json!({"path": legacy, "message": "unreadable"})],
    );
    assert_eq!(
        check_skill_catalogue(&reply, &setup.fixture, &authority)
            .unwrap()
            .skill_count,
        2
    );
    // A plain Markdown file under a skill root is not a skill source.
    let stray = setup.workspace.join(".claude/skills/notes.md");
    fs::write(&stray, "notes").unwrap();
    assert!(!is_skill_source(
        &stray,
        &skill_roots,
        &authority.command_roots(&setup.workspace)
    ));
}

#[test]
fn explicit_error_is_unsupported_not_passed() {
    let setup = NativeSetup::new();
    let result = check_skill_catalogue(
        &error_reply(),
        &setup.fixture,
        &setup.authority(CLAUDE, &[], true),
    )
    .unwrap();
    assert!(matches!(
        result.status,
        CapabilityStatus::ExplicitUnsupported { .. }
    ));
}

#[test]
fn unloaded_fixture_leaves_no_reference() {
    let setup = NativeSetup::new();
    let mut authority = setup.authority(CLAUDE, &[], true);
    authority.advertised_names.clear();
    expect_error(
        check_skill_catalogue(&error_reply(), &setup.fixture, &authority),
        "did not load the controlled",
    );
}

#[test]
fn other_workspace_never_borrows_fixture_skills() {
    let setup = NativeSetup::new();
    let authority = setup.authority(CLAUDE, &[], true);
    let clean = skills_reply(
        &setup.empty,
        vec![skill_row("user-native", &setup.user_skill, "user", true)],
        vec![],
    );
    assert_eq!(
        check_wrong_context_catalogue(&clean, &setup.fixture, &authority)
            .unwrap()
            .status,
        CapabilityStatus::Supported
    );
    let borrowed = skills_reply(
        &setup.empty,
        vec![skill_row(SKILL, &setup.skill_file, "repo", true)],
        vec![],
    );
    assert!(check_wrong_context_catalogue(&borrowed, &setup.fixture, &authority).is_err());
}
