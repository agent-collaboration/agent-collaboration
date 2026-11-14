use super::InFolderGrant;
use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionKind, ToolCallLocation, ToolCallUpdate, ToolCallUpdateFields,
    ToolKind,
};
use std::path::{Path, PathBuf};

struct Folders {
    _root: tempfile::TempDir,
    root: PathBuf,
    project: PathBuf,
    scratch: PathBuf,
}

fn folders() -> Folders {
    let root = tempfile::tempdir().unwrap();
    let base = std::fs::canonicalize(root.path()).unwrap();
    let project = base.join("project");
    let scratch = base.join("scratch").join("session-a");
    for folder in [
        &project,
        &scratch,
        &base.join("scratch/session-ab"),
        &base.join("outside"),
    ] {
        std::fs::create_dir_all(folder).unwrap();
    }
    std::os::unix::fs::symlink(base.join("outside"), scratch.join("escape")).unwrap();
    std::os::unix::fs::symlink(&scratch, base.join("scratch-link")).unwrap();
    Folders {
        _root: root,
        root: base,
        project,
        scratch,
    }
}

fn options() -> Vec<PermissionOption> {
    vec![
        PermissionOption::new("allow", "Allow", PermissionOptionKind::AllowOnce),
        PermissionOption::new("always", "Always", PermissionOptionKind::AllowAlways),
        PermissionOption::new("reject", "Reject", PermissionOptionKind::RejectOnce),
    ]
}

fn file_call(kind: ToolKind, paths: &[&Path]) -> ToolCallUpdate {
    ToolCallUpdate::new(
        "call",
        ToolCallUpdateFields::new().kind(kind).locations(
            paths
                .iter()
                .map(|path| ToolCallLocation::new(path.to_path_buf()))
                .collect::<Vec<_>>(),
        ),
    )
}

fn shell_call(command: &str) -> ToolCallUpdate {
    ToolCallUpdate::new(
        "call",
        ToolCallUpdateFields::new()
            .kind(ToolKind::Execute)
            .raw_input(serde_json::json!({ "command": command })),
    )
}

#[test]
fn file_actions_inside_the_project_or_scratch_folder_are_allowed_once_and_nothing_else() {
    let folders = folders();
    let grant = InFolderGrant::new(&folders.project, &folders.scratch).unwrap();
    let new_scratch_file = folders.scratch.join("new/deeper/note.txt");
    let project_file = folders.project.join("src/lib.rs");
    let sibling = folders.root.join("scratch/session-ab/file");
    let dot_dot = folders.scratch.join("../session-ab/file");
    let through_symlink = folders.scratch.join("escape/file");
    let via_linked_root = folders.root.join("scratch-link/file");
    let system_tmp = PathBuf::from("/tmp/d1-outside");
    let cases: [(&str, ToolKind, Vec<&Path>, bool); 11] = [
        (
            "scratch edit",
            ToolKind::Edit,
            vec![&new_scratch_file],
            true,
        ),
        (
            "scratch delete",
            ToolKind::Delete,
            vec![&new_scratch_file],
            true,
        ),
        ("project read", ToolKind::Read, vec![&project_file], true),
        (
            "move within the grant",
            ToolKind::Move,
            vec![&project_file, &new_scratch_file],
            true,
        ),
        (
            "linked root resolving into scratch",
            ToolKind::Edit,
            vec![&via_linked_root],
            true,
        ),
        ("prefix sibling", ToolKind::Edit, vec![&sibling], false),
        ("dot-dot escape", ToolKind::Edit, vec![&dot_dot], false),
        (
            "symlink escape",
            ToolKind::Edit,
            vec![&through_symlink],
            false,
        ),
        ("system tmp", ToolKind::Edit, vec![&system_tmp], false),
        (
            "one location outside",
            ToolKind::Edit,
            vec![&new_scratch_file, &system_tmp],
            false,
        ),
        ("no locations", ToolKind::Edit, vec![], false),
    ];
    for (name, kind, paths, allowed) in cases {
        let decision = grant.allow_once(&file_call(kind, &paths), &options());
        assert_eq!(decision.is_some(), allowed, "{name}");
        if allowed {
            assert_eq!(decision.unwrap().0.as_ref(), "allow", "{name}");
        }
    }
    assert!(
        grant
            .allow_once(
                &file_call(ToolKind::Fetch, &[&new_scratch_file]),
                &options()
            )
            .is_none(),
        "only file kinds take the file rule"
    );
}

#[test]
fn shell_commands_are_allowed_once_only_when_every_operand_is_in_the_scratch_folder() {
    let folders = folders();
    let grant = InFolderGrant::new(&folders.project, &folders.scratch).unwrap();
    let scratch = folders.scratch.display().to_string();
    let project = folders.project.display().to_string();
    let cases = [
        (format!("mkdir -p {scratch}/build"), true),
        (format!("rm {scratch}/a {scratch}/b"), true),
        (format!("ls {scratch}"), true),
        (format!("cp {scratch}/a {project}/a"), false),
        (format!("rm {scratch}/escape/file"), false),
        (format!("rm {scratch}/../session-ab/file"), false),
        (format!("cat {scratch}/a > /tmp/out"), false),
        (format!("ls {scratch} && rm -rf /"), false),
        (format!("echo $HOME {scratch}"), false),
        (format!("python3 {scratch}/x.py notes"), false),
        (format!("{scratch}/run.sh"), false),
        ("git status".to_owned(), false),
        ("touch /tmp/d1-outside".to_owned(), false),
        (String::new(), false),
        // File utilities that run no code given to them are allowed on scratch.
        (format!("cat {scratch}/a"), true),
        (format!("cp {scratch}/a {scratch}/b"), true),
        (format!("find {scratch} -delete"), true),
        // A program that runs code from its operands could act anywhere once
        // the agent writes that code into scratch, so it asks (Q3).
        (format!("bash {scratch}/run.sh"), false),
        (format!("sh {scratch}/run.sh"), false),
        (format!("zsh {scratch}/run.sh"), false),
        (format!("python3 {scratch}/x.py"), false),
        (format!("node {scratch}/x.js"), false),
        (format!("env {scratch}/run.sh"), false),
        (format!("xargs {scratch}/run.sh"), false),
        (format!("find {scratch} -exec {scratch}/run.sh"), false),
        (format!("find {scratch} -execdir {scratch}/run.sh"), false),
        (format!("find {scratch} -ok {scratch}/run.sh"), false),
        (format!("find {scratch} -okdir {scratch}/run.sh"), false),
        (format!("find {scratch} -fprint {scratch}/out"), false),
        (format!("find {scratch} -fprintf {scratch}/out"), false),
        (format!("find {scratch} -fls {scratch}/out"), false),
        // A path glued onto an option is a path too; it asks.
        (format!("sort -o/etc/x {scratch}/a"), false),
        (format!("cp -t/elsewhere {scratch}/a"), false),
        (format!("mv -t/elsewhere {scratch}/a"), false),
        (format!("grep -f/etc/passwd {scratch}/a"), false),
        (format!("sort -o /etc/x {scratch}/a"), false),
        (format!("rm -rf {scratch}/dir"), true),
        (format!("ls -la {scratch}"), true),
    ];
    for (command, allowed) in cases {
        assert_eq!(
            grant
                .allow_once(&shell_call(&command), &options())
                .is_some(),
            allowed,
            "{command}"
        );
    }
}

#[test]
fn without_one_allow_once_option_the_request_goes_to_approval() {
    let folders = folders();
    let grant = InFolderGrant::new(&folders.project, &folders.scratch).unwrap();
    let call = file_call(ToolKind::Edit, &[&folders.scratch.join("a")]);
    let no_allow_once = vec![PermissionOption::new(
        "always",
        "Always",
        PermissionOptionKind::AllowAlways,
    )];
    assert!(grant.allow_once(&call, &no_allow_once).is_none());
    let mut two = options();
    two.push(PermissionOption::new(
        "allow-2",
        "Allow",
        PermissionOptionKind::AllowOnce,
    ));
    assert!(grant.allow_once(&call, &two).is_none());
}
