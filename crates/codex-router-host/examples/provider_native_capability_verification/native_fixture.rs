//! Materialize the controlled native skills into the prepared fixture worktree.
//!
//! The fixture workspace is a genuine linked Git worktree the Lead created at the
//! declared candidate HEAD: Codex inherits the main repository's existing trust only
//! through a verified worktree registration, and Cursor and Claude resolve its Git root
//! natively. This writer only adds new files (create-new, never overwrite) and creates
//! the separately owned empty comparison directory; it never touches an installation.
use crate::verification_models::FixtureRecord;
use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Command,
};

pub(crate) const FIXTURE_SKILLS: [&str; 2] = ["router-native-arguments", "router-native-second"];
pub(crate) const CANARY_PLACEHOLDER: &str = "NATIVE_CAPABILITY_CANARY";
pub(crate) const VISIBLE_FILE: &str = "README.verification.txt";

/// Permanent Markdown fixture templates, from the repository's scripts location.
pub(crate) fn fixture_source(repository: &Path) -> PathBuf {
    repository.join("scripts/verification/fixtures/provider_native_capabilities")
}

fn fresh_canary() -> Result<String, String> {
    let mut bytes = [0_u8; 8];
    fs::File::open("/dev/urandom")
        .and_then(|mut source| source.read_exact(&mut bytes))
        .map_err(|error| format!("no random source: {error}"))?;
    Ok(format!(
        "NATIVE_SKILL_{}",
        bytes
            .iter()
            .map(|byte| format!("{byte:02X}"))
            .collect::<String>()
    ))
}

pub(crate) fn write_private(path: &Path, contents: &[u8]) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|error| format!("cannot create {}: {error}", path.display()))?;
    file.write_all(contents)
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn git_lines(workspace: &Path, arguments: &[&str]) -> Result<Vec<String>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(workspace)
        .args(arguments)
        .output()
        .map_err(|error| format!("git could not run: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git {} failed in the fixture workspace",
            arguments.join(" ")
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect())
}

/// The worktree's HEAD and every file it adds beyond that commit (untracked, ignored
/// or modified), relative to the worktree, after checking it is a genuine linked
/// worktree rooted exactly at `workspace`: its `.git` is a file and Git reports a
/// private git directory apart from the shared common directory.
pub(crate) fn fixture_worktree_state(workspace: &Path) -> Result<(String, Vec<PathBuf>), String> {
    if !workspace.join(".git").is_file() {
        return Err("fixture workspace is not a linked Git worktree".to_owned());
    }
    let facts = git_lines(
        workspace,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
            "HEAD",
        ],
    )?;
    let [top_level, git_dir, common_dir, head] = facts.as_slice() else {
        return Err("git did not describe the fixture workspace".to_owned());
    };
    let resolve = |path: &str| fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));
    if resolve(top_level) != resolve(&workspace.display().to_string()) {
        return Err("fixture workspace is not the worktree root".to_owned());
    }
    if resolve(git_dir) == resolve(common_dir) {
        return Err("fixture workspace is a main checkout, not a linked worktree".to_owned());
    }
    let mut changes: Vec<PathBuf> = git_lines(
        workspace,
        &[
            "status",
            "--porcelain",
            "--untracked-files=all",
            "--ignored=matching",
        ],
    )?
    .iter()
    .map(|line| PathBuf::from(line.get(3..).unwrap_or_default()))
    .collect();
    changes.sort();
    Ok((head.clone(), changes))
}

/// The files materialization adds, relative to the workspace, sorted.
pub(crate) fn fixture_relative_files(fixture: &FixtureRecord) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = [
        &fixture.skill_file,
        &fixture.second_skill_file,
        &fixture.visible_file,
    ]
    .iter()
    .filter_map(|file| {
        file.strip_prefix(&fixture.workspace)
            .ok()
            .map(Path::to_owned)
    })
    .collect();
    files.sort();
    files
}

/// Add the controlled project skills to the prepared, clean fixture worktree and create
/// the empty comparison directory. Each skill body gets a fresh canary that appears
/// nowhere else, so a reply carrying it can only come from the provider applying it.
pub(crate) fn materialize_fixture(
    templates: &Path,
    workspace: &Path,
    empty_workspace: &Path,
    record: &Path,
    home: &Path,
) -> Result<FixtureRecord, String> {
    for owned in [workspace, empty_workspace] {
        if !owned.is_absolute() || owned.starts_with(home) {
            return Err(
                "fixture workspaces are absolute and outside the home directory".to_owned(),
            );
        }
    }
    if empty_workspace.starts_with(workspace) || workspace.starts_with(empty_workspace) {
        return Err("the empty comparison directory is owned apart from the worktree".to_owned());
    }
    let (workspace_head, changes) = fixture_worktree_state(workspace)?;
    if !changes.is_empty() {
        return Err("fixture worktree is not clean before materialization".to_owned());
    }
    DirBuilder::new()
        .mode(0o700)
        .create(empty_workspace)
        .map_err(|error| format!("cannot create {}: {error}", empty_workspace.display()))?;
    let mut canaries = Vec::new();
    let mut files = Vec::new();
    for name in FIXTURE_SKILLS {
        let canary = fresh_canary()?;
        let body = fs::read_to_string(templates.join(name).join("SKILL.md"))
            .map_err(|error| format!("fixture template {name} unreadable: {error}"))?;
        if !body.contains(CANARY_PLACEHOLDER) {
            return Err(format!("fixture template {name} has no canary placeholder"));
        }
        let target = workspace.join(".claude/skills").join(name).join("SKILL.md");
        if let Some(folder) = target.parent() {
            DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(folder)
                .map_err(|error| format!("cannot create {}: {error}", folder.display()))?;
        }
        write_private(
            &target,
            body.replace(CANARY_PLACEHOLDER, &canary).as_bytes(),
        )?;
        canaries.push(canary);
        files.push(target);
    }
    let [skill_name, second_skill_name] = FIXTURE_SKILLS;
    let visible = workspace.join(VISIBLE_FILE);
    write_private(
        &visible,
        b"Visible workspace file for the @ file-only control.\n",
    )?;
    let [canary, second_canary] =
        <[String; 2]>::try_from(canaries).map_err(|_| "fixture canaries incomplete".to_owned())?;
    let [skill_file, second_skill_file] =
        <[PathBuf; 2]>::try_from(files).map_err(|_| "fixture files incomplete".to_owned())?;
    let fixture = FixtureRecord {
        workspace: workspace.to_owned(),
        workspace_head,
        empty_workspace: empty_workspace.to_owned(),
        visible_file: visible,
        skill_name: skill_name.to_owned(),
        skill_file,
        canary,
        second_skill_name: second_skill_name.to_owned(),
        second_skill_file,
        second_canary,
    };
    let json = serde_json::to_vec_pretty(&fixture).map_err(|error| error.to_string())?;
    write_private(record, &json)?;
    Ok(fixture)
}
