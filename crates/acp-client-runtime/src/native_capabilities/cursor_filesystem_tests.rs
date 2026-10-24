use super::*;

struct NativeFiles {
    _directory: tempfile::TempDir,
    context: CursorFilesystemContext,
}

impl NativeFiles {
    fn new() -> Self {
        let directory = tempfile::tempdir_in("/tmp").expect("fixture directory");
        let home = directory.path().join("home");
        let workspace = directory.path().join("workspace");
        fs::create_dir_all(&home).expect("home");
        fs::create_dir_all(&workspace).expect("workspace");
        let context = CursorFilesystemContext::new(home, workspace).expect("context");
        Self {
            _directory: directory,
            context,
        }
    }

    fn write(&self, user: bool, relative: &str, contents: &str) -> PathBuf {
        let root = if user {
            &self.context.home
        } else {
            &self.context.workspace
        };
        let path = root.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("parent directory");
        fs::write(&path, contents).expect("skill source");
        path
    }

    fn read(&self) -> Vec<CursorFileSkill> {
        let catalogue = read_cursor_filesystem_skills(
            &self.context,
            QUALIFIED_CURSOR_VERSION,
            &self.context.workspace,
            &CancellationToken::new(),
        )
        .expect("accounted native roots");
        assert!(
            catalogue.errors.is_empty(),
            "unexpected candidate errors: {:?}",
            catalogue.errors
        );
        catalogue.skills
    }
}

#[test]
fn native_precedence_uses_user_last_and_path_identity_not_frontmatter_title() {
    let files = NativeFiles::new();
    files.write(false, ".cursor/skills/alpha/SKILL.md", "# Workspace alpha");
    let expected = files.write(
        true,
        ".agents/skills/alpha/SKILL.md",
        "---\nname: Presentation title\ndescription: |\n  Native user description\n---\nBody",
    );
    let skills = files.read();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].canonical_name, "alpha");
    assert_eq!(skills[0].path, expected);
    assert_eq!(skills[0].description, "Native user description\n");
    assert_eq!(skills[0].scope, CursorFileSkillScope::User);
}

#[test]
fn native_surface_filter_and_invocation_boolean_are_preserved() {
    let files = NativeFiles::new();
    files.write(
        false,
        ".cursor/skills/disabled/SKILL.md",
        "---\nuser-invocable: ' FALSE '\n---\n# Disabled",
    );
    files.write(
        false,
        ".cursor/skills/editor/SKILL.md",
        "---\nmetadata:\n  surfaces: [editor]\n---\n# Editor only",
    );
    files.write(
        false,
        ".cursor/skills/cli/SKILL.md",
        "---\nuser-invocable: 0\nmetadata:\n  surfaces: editor, cli\n---\n# CLI skill",
    );
    let skills = files.read();
    assert_eq!(skills.len(), 2);
    assert!(
        skills
            .iter()
            .any(|skill| skill.canonical_name == "disabled" && !skill.user_invocable)
    );
    assert!(
        skills
            .iter()
            .any(|skill| skill.canonical_name == "cli" && skill.user_invocable)
    );
}

#[test]
fn bundled_and_reserved_sources_are_excluded_and_nested_names_disambiguated() {
    let files = NativeFiles::new();
    files.write(true, ".cursor/skills-cursor/builtin/SKILL.md", "# Builtin");
    files.write(false, ".codex/skills/openai-docs/SKILL.md", "# System");
    files.write(false, ".cursor/skills/.hidden/SKILL.md", "# Hidden");
    files.write(false, ".cursor/skills/one/alpha/SKILL.md", "# One");
    files.write(false, ".cursor/skills/two/alpha/SKILL.md", "# Two");
    let skills = files.read();
    assert_eq!(
        skills
            .iter()
            .map(|skill| skill.canonical_name.as_str())
            .collect::<Vec<_>>(),
        ["one-alpha", "two-alpha"]
    );
}

#[test]
fn unreadable_metadata_never_becomes_successful_empty() {
    let files = NativeFiles::new();
    let broken = files.write(
        false,
        ".cursor/skills/alpha/SKILL.md",
        "---\nname: [broken\n---\n",
    );
    let catalogue = read_cursor_filesystem_skills(
        &files.context,
        QUALIFIED_CURSOR_VERSION,
        &files.context.workspace,
        &CancellationToken::new(),
    )
    .expect("accounted source");
    assert!(catalogue.skills.is_empty());
    assert_eq!(
        catalogue.errors,
        vec![CursorSkillFileFailure {
            path: fs::canonicalize(broken).expect("candidate path"),
            reason: CursorSkillFileFailureReason::InvalidMetadata,
        }]
    );
}

#[test]
fn version_scope_and_cancellation_are_explicit_failures() {
    let files = NativeFiles::new();
    assert!(matches!(
        read_cursor_filesystem_skills(
            &files.context,
            "unknown",
            &files.context.workspace,
            &CancellationToken::new()
        ),
        Err(CursorFilesystemReadError::UnsupportedVersion)
    ));
    assert!(matches!(
        read_cursor_filesystem_skills(
            &files.context,
            QUALIFIED_CURSOR_VERSION,
            &files.context.home,
            &CancellationToken::new()
        ),
        Err(CursorFilesystemReadError::InvalidScope)
    ));
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(matches!(
        read_cursor_filesystem_skills(
            &files.context,
            QUALIFIED_CURSOR_VERSION,
            &files.context.workspace,
            &cancelled
        ),
        Err(CursorFilesystemReadError::Cancelled)
    ));
}

#[cfg(unix)]
#[test]
fn workspace_symlink_escape_is_excluded_user_escape_reads_only_direct_files() {
    use std::os::unix::fs::symlink;
    let files = NativeFiles::new();
    let outside = files._directory.path().join("outside");
    fs::create_dir_all(outside.join("nested")).expect("outside directory");
    fs::write(outside.join("SKILL.md"), "# External skill").expect("external skill");
    fs::write(outside.join("nested/SKILL.md"), "# Not recursively loaded").expect("nested skill");
    fs::create_dir_all(files.context.workspace.join(".cursor/skills")).expect("workspace root");
    fs::create_dir_all(files.context.home.join(".cursor/skills")).expect("user root");
    symlink(
        &outside,
        files.context.workspace.join(".cursor/skills/escape"),
    )
    .expect("workspace link");
    symlink(&outside, files.context.home.join(".cursor/skills/escape")).expect("user link");
    let skills = files.read();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].canonical_name, "escape");
    assert_eq!(skills[0].scope, CursorFileSkillScope::User);
}

#[test]
fn native_capabilities_cursor_discovery_uses_nearest_git_marker_from_launch_workspace() {
    let files = NativeFiles::new();
    let native_root = files._directory.path();
    fs::create_dir(native_root.join(".git")).expect("native git marker");
    let skill = native_root.join(".cursor/skills/root-native/SKILL.md");
    fs::create_dir_all(skill.parent().expect("parent")).expect("root skill directory");
    fs::write(&skill, "# Root native skill").expect("native root skill");
    files.write(
        false,
        ".cursor/skills/nested-only/SKILL.md",
        "# Not in native ACP discovery root",
    );
    let skills = files.read();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].canonical_name, "root-native");
    assert_eq!(
        skills[0].path,
        fs::canonicalize(skill).expect("native physical root")
    );
}

#[test]
fn native_capabilities_cursor_disabled_file_does_not_shadow_an_available_native_definition() {
    let files = NativeFiles::new();
    let active = files.write(false, ".cursor/skills/alpha/SKILL.md", "# Workspace alpha");
    let disabled = files.write(
        true,
        ".cursor/skills/alpha/SKILL.md",
        "---\nuser-invocable: false\n---\n# Disabled user alpha",
    );
    let skills = files.read();
    assert_eq!(skills.len(), 2);
    assert!(skills.iter().any(|skill| skill.user_invocable
        && skill.path == fs::canonicalize(&active).expect("native workspace path")));
    assert!(
        skills
            .iter()
            .any(|skill| !skill.user_invocable && skill.path == disabled)
    );
}

#[test]
fn native_capabilities_cursor_malformed_candidate_preserves_other_native_files() {
    // Closest principle scenario: the field the schema did not know. The real
    // producer must retain both the usable row and its located failure.
    let files = NativeFiles::new();
    files.write(
        false,
        ".cursor/skills/healthy/SKILL.md",
        "Healthy native skill",
    );
    let broken = files.write(
        false,
        ".cursor/skills/broken/SKILL.md",
        "---\nname: [broken\n---\n",
    );
    let catalogue = read_cursor_filesystem_skills(
        &files.context,
        QUALIFIED_CURSOR_VERSION,
        &files.context.workspace,
        &CancellationToken::new(),
    )
    .expect("accounted root");
    assert_eq!(catalogue.skills.len(), 1);
    assert_eq!(catalogue.skills[0].canonical_name, "healthy");
    assert_eq!(
        catalogue.errors,
        vec![CursorSkillFileFailure {
            path: fs::canonicalize(broken).expect("native candidate path"),
            reason: CursorSkillFileFailureReason::InvalidMetadata,
        }]
    );
}

#[cfg(unix)]
#[test]
fn native_capabilities_cursor_dangling_non_skill_links_preserve_healthy_files() {
    use std::os::unix::fs::symlink;
    let files = NativeFiles::new();
    let healthy = files.write(
        true,
        ".cursor/skills/healthy/SKILL.md",
        "Healthy user skill",
    );
    for name in ["removed-folder", "removed-note.md"] {
        symlink(
            files.context.home.join("missing-target").join(name),
            files.context.home.join(".cursor/skills").join(name),
        )
        .expect("dangling non-skill link");
    }

    let skills = files.read();

    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].canonical_name, "healthy");
    assert_eq!(skills[0].path, healthy);
    assert_eq!(skills[0].scope, CursorFileSkillScope::User);
}

#[cfg(unix)]
#[test]
fn native_capabilities_cursor_dangling_skill_file_remains_an_explicit_failure() {
    use std::os::unix::fs::symlink;
    let files = NativeFiles::new();
    files.write(
        true,
        ".cursor/skills/healthy/SKILL.md",
        "Healthy user skill",
    );
    let broken = files.context.home.join(".cursor/skills/SKILL.md");
    symlink(files.context.home.join("missing-skill.md"), &broken)
        .expect("dangling in-view skill source");

    let result = read_cursor_filesystem_skills(
        &files.context,
        QUALIFIED_CURSOR_VERSION,
        &files.context.workspace,
        &CancellationToken::new(),
    );

    assert_eq!(
        result,
        Err(CursorFilesystemReadError::Unreadable { path: broken })
    );
}
