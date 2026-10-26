//! J1 native source membership for Claude 2.1.310: an exact enumeration of the entries
//! the native loader reads, judged on logical and canonical identity, not physical
//! containment. Anchors (installed 2.1.310 bytes): ordinary roots `AL` (immediate
//! directory or symlink entries with `SKILL.md`), legacy commands `jEo`/`b0`
//! (recursive `.md`, following symlinks). Real files only; apparatus sensitivity, not
//! native product proof.
use crate::catalogue_oracle::check_skill_catalogue;
use crate::native_file_facts::claude_source_candidates;
use crate::oracle_test_support::{
    NativeSetup, SKILL, expect_error, skill_row, skills_reply, write_skill,
};
use crate::verification_models::{CapabilityStatus, ProviderEndpoint};
use serde_json::json;
use std::{fs, os::unix::fs::symlink, path::PathBuf};
use tempfile::TempDir;

fn canonical_temp() -> (TempDir, PathBuf) {
    let directory = TempDir::new().unwrap();
    let root = fs::canonicalize(directory.path()).unwrap();
    (directory, root)
}

#[test]
fn ordinary_entries_are_immediate_folders_or_symlinks_with_a_skill_file() {
    let (_directory, base) = canonical_temp();
    let root = base.join("skills");
    let direct = write_skill(&root, "direct", "");
    let outside = write_skill(&base.join("elsewhere"), "linked-target", "");
    symlink(outside.parent().unwrap(), root.join("linked")).unwrap();
    let stray = write_skill(&root.join("group/nested"), "deep", "");
    fs::create_dir_all(root.join("empty-folder")).unwrap();
    fs::write(root.join("notes.md"), "notes").unwrap();
    let sources = claude_source_candidates(std::slice::from_ref(&root), &[], &[]);
    // The direct folder and file forms.
    assert!(sources.is_skill(&direct));
    assert!(sources.is_skill(direct.parent().unwrap()));
    // A native-followed symlink: its logical path and its physical origin outside.
    assert!(sources.is_skill(&root.join("linked/SKILL.md")));
    assert!(sources.is_skill(&root.join("linked")));
    assert!(sources.is_skill(&outside));
    // Inside the root but never an entry the loader reads.
    assert!(!sources.is_skill(&stray));
    assert!(!sources.is_skill(&root.join("group")));
    assert!(!sources.is_skill(&root.join("empty-folder")));
    assert!(!sources.is_skill(&root.join("notes.md")));
    assert!(!sources.is_skill(&root));
}

#[test]
fn legacy_commands_are_found_recursively_through_symlinks_and_cycles_end() {
    let (_directory, base) = canonical_temp();
    let commands = base.join("commands");
    let nested = commands.join("group/sub/task.md");
    fs::create_dir_all(nested.parent().unwrap()).unwrap();
    fs::write(&nested, "---\ndescription: task\n---\n").unwrap();
    let outside = base.join("shared-commands");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("shared.md"), "shared").unwrap();
    symlink(&outside, commands.join("linked")).unwrap();
    // A cycle back to the root must not loop forever.
    symlink(&commands, commands.join("group/loop")).unwrap();
    let skill_root = base.join("skills");
    fs::create_dir_all(&skill_root).unwrap();
    fs::write(skill_root.join("not-a-command.md"), "x").unwrap();
    let sources = claude_source_candidates(
        std::slice::from_ref(&skill_root),
        std::slice::from_ref(&commands),
        &[],
    );
    assert!(sources.is_command(&nested));
    assert!(sources.is_command(&commands.join("linked/shared.md")));
    assert!(sources.is_command(&outside.join("shared.md")));
    // A .md outside every command root has no command role.
    assert!(!sources.is_command(&skill_root.join("not-a-command.md")));
    assert!(!sources.is_source(&skill_root.join("not-a-command.md")));
}

#[test]
fn plugin_skill_sources_are_the_declared_files_only() {
    let (_directory, base) = canonical_temp();
    let plugin = base.join("cache/market/alpha/1.0.0");
    let declared = write_skill(&plugin.join("skills"), "review", "");
    let arbitrary = write_skill(&plugin.join("docs"), "not-declared", "");
    fs::write(plugin.join("README.md"), "readme").unwrap();
    let sources = claude_source_candidates(&[], &[], std::slice::from_ref(&declared));
    assert!(sources.is_skill(&declared));
    assert!(sources.is_skill(declared.parent().unwrap()));
    assert!(!sources.is_skill(&arbitrary));
    assert!(!sources.is_source(&plugin.join("README.md")));
    assert!(!sources.is_skill(&plugin));
}

#[test]
fn claude_rows_and_errors_are_judged_by_native_membership() {
    let setup = NativeSetup::new();
    let authority = setup.authority(ProviderEndpoint::ClaudeLocal, &["linked-skill"], false);
    let skills = setup.workspace.join(".claude/skills");
    let outside = write_skill(&setup.home.join("outside"), "linked-skill", "");
    symlink(outside.parent().unwrap(), skills.join("linked-skill")).unwrap();
    // A symlinked skill whose physical origin lies outside every root is a native
    // source under either spelling.
    for path in [skills.join("linked-skill/SKILL.md"), outside] {
        let reply = skills_reply(
            &setup.workspace,
            vec![
                skill_row(SKILL, &setup.skill_file, "repo", true),
                skill_row("linked-skill", &path, "repo", true),
            ],
            vec![],
        );
        assert_eq!(
            check_skill_catalogue(&reply, &setup.fixture, &authority)
                .unwrap()
                .status,
            CapabilityStatus::Supported
        );
    }
    // A stray nested SKILL.md inside a root is not a native entry.
    let stray = write_skill(&skills.join("group"), "linked-skill", "");
    let reply = skills_reply(
        &setup.workspace,
        vec![
            skill_row(SKILL, &setup.skill_file, "repo", true),
            skill_row("linked-skill", &stray, "repo", true),
        ],
        vec![],
    );
    expect_error(
        check_skill_catalogue(&reply, &setup.fixture, &authority),
        "outside the provider's own roots",
    );
    // An error naming a stray nested file is not a native skill source either.
    let errored = skills_reply(
        &setup.workspace,
        vec![skill_row(SKILL, &setup.skill_file, "repo", true)],
        vec![json!({"path": stray, "message": "unreadable"})],
    );
    expect_error(
        check_skill_catalogue(&errored, &setup.fixture, &authority),
        "no actual native skill source",
    );
}

#[test]
fn reserved_synced_entry_is_never_an_ordinary_skill() {
    // Installed 2.1.310 `AL`: `r==="skills"&&Q3(Se.name)` skips the sync-owned root's
    // reserved name in ordinary skill scans. No account-sync qualification is claimed.
    let (_directory, base) = canonical_temp();
    let root = base.join("skills");
    let synced = write_skill(&root, "synced", "");
    let mixed_case = write_skill(&root, "Synced", "");
    let ordinary = write_skill(&root, "ordinary", "");
    let sources = claude_source_candidates(std::slice::from_ref(&root), &[], &[]);
    assert!(sources.is_skill(&ordinary));
    for reserved in [&synced, &mixed_case] {
        assert!(!sources.is_skill(reserved), "{}", reserved.display());
        assert!(!sources.is_skill(reserved.parent().unwrap()));
    }
}
