use super::*;

fn root(directory: &Path) -> ClaudeSkillRoot {
    ClaudeSkillRoot {
        directory: directory.to_owned(),
        scope: ClaudeFileSkillScope::Workspace,
    }
}
use std::path::Path;
fn write(directory: &Path, name: &str, contents: &str) -> PathBuf {
    let path = directory.join(name).join("SKILL.md");
    fs::create_dir_all(path.parent().expect("parent")).expect("skill folder");
    fs::write(&path, contents).expect("skill source");
    path
}

#[test]
fn native_capabilities_claude_file_name_is_canonical_and_effective_override_off_is_not_defaulted() {
    let directory = tempfile::tempdir_in("/tmp").expect("fixture");
    let path = write(
        directory.path(),
        "alpha",
        "---\nname: Display title\ndescription: ' Native description '\nuser-invocable: true\n---\n# Body",
    );
    let overrides = HashMap::from([("alpha".to_owned(), ClaudeSkillOverride::Off)]);
    let skills = read_claude_filesystem_skills(
        "2.1.310",
        &[root(directory.path())],
        &overrides,
        &CancellationToken::new(),
    )
    .expect("qualified files")
    .skills;
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].canonical_name, "alpha");
    assert_eq!(skills[0].path, path);
    assert_eq!(skills[0].description, "Native description");
    assert!(!skills[0].user_invocable);
}

#[test]
fn native_capabilities_claude_missing_description_uses_native_body_fallback_without_expansion() {
    let directory = tempfile::tempdir_in("/tmp").expect("fixture");
    write(
        directory.path(),
        "alpha",
        "---\ndisable-model-invocation: true\n---\n\n# Argument $ARGUMENTS !`do-not-run`\n",
    );
    let skills = read_claude_filesystem_skills(
        "2.1.310",
        &[root(directory.path())],
        &HashMap::new(),
        &CancellationToken::new(),
    )
    .expect("qualified files")
    .skills;
    assert_eq!(skills[0].description, "Argument $ARGUMENTS !`do-not-run`");
    assert!(skills[0].user_invocable);
}

#[test]
fn native_capabilities_claude_conditional_and_duplicate_sources_are_unqualified_reserved_sync_root_is_separate()
 {
    let directory = tempfile::tempdir_in("/tmp").expect("fixture");
    write(
        directory.path(),
        "alpha",
        "---\npaths: ['src/**']\n---\n# Conditional",
    );
    let catalogue = read_claude_filesystem_skills(
        "2.1.310",
        &[root(directory.path())],
        &HashMap::new(),
        &CancellationToken::new(),
    )
    .expect("accounted conditional source");
    assert!(catalogue.skills.is_empty());
    assert_eq!(
        catalogue.errors,
        vec![ClaudeSkillFileFailure {
            path: directory.path().join("alpha/SKILL.md"),
            reason: ClaudeFilesystemReadError::UnqualifiedSource,
        }]
    );
    let directory = tempfile::tempdir_in("/tmp").expect("fixture");
    write(directory.path(), "alpha", "# Alpha");
    assert_eq!(
        read_claude_filesystem_skills(
            "2.1.310",
            &[root(directory.path()), root(directory.path())],
            &HashMap::new(),
            &CancellationToken::new()
        ),
        Err(ClaudeFilesystemReadError::AmbiguousOrigin)
    );
    fs::create_dir(directory.path().join("synced")).expect("sync root");
    let skills = read_claude_filesystem_skills(
        "2.1.310",
        &[root(directory.path())],
        &HashMap::new(),
        &CancellationToken::new(),
    )
    .expect("ordinary source view")
    .skills;
    assert_eq!(skills.len(), 1);
}

#[test]
fn native_capabilities_claude_description_uses_qualified_native_display_normalisation() {
    assert_eq!(
        normalise_claude_description("Native\n\t<路径>"),
        Ok("Native &lt;路径&gt;".into())
    );
    assert_eq!(
        normalise_claude_description("Unqualified\u{200d}joiner"),
        Err(ClaudeFilesystemReadError::UnqualifiedSource)
    );
    assert_eq!(
        normalise_claude_description("\u{1b}[31mANSI"),
        Err(ClaudeFilesystemReadError::UnqualifiedSource)
    );
}

#[test]
fn native_capabilities_claude_malformed_candidate_does_not_erase_healthy_source() {
    let directory = tempfile::tempdir_in("/tmp").expect("fixture");
    write(directory.path(), "healthy", "Healthy source");
    write(directory.path(), "broken", "---\nname: [broken\n---\n");
    let catalogue = read_claude_filesystem_skills(
        "2.1.310",
        &[root(directory.path())],
        &HashMap::new(),
        &CancellationToken::new(),
    )
    .expect("accounted root");
    assert_eq!(catalogue.skills.len(), 1);
    assert_eq!(catalogue.skills[0].canonical_name, "healthy");
    assert_eq!(
        catalogue.errors,
        vec![ClaudeSkillFileFailure {
            path: directory.path().join("broken/SKILL.md"),
            reason: ClaudeFilesystemReadError::InvalidMetadata,
        }]
    );
}
