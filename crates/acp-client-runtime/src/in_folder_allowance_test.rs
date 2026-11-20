use super::InFolderGrant;
use crate::{PermissionRequestAction, StatedPermissionTarget};
use agent_client_protocol::schema::v1::{
    Diff, PermissionOption, PermissionOptionKind, ToolCallContent, ToolCallLocation,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};

/// A provider behaviour that reads no titles (every agent but Cursor).
const NO_TITLES: fn(PermissionRequestAction, &str) -> Option<StatedPermissionTarget> = |_, _| None;
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
    // Dangling links: their targets outside do not exist yet.
    std::os::unix::fs::symlink(base.join("outside/new"), scratch.join("dangling")).unwrap();
    std::os::unix::fs::symlink(
        base.join("outside/missing-dir"),
        scratch.join("dangling-dir"),
    )
    .unwrap();
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
    let grant = InFolderGrant::new(&folders.project, &folders.scratch, &[]).unwrap();
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
        let decision = grant.allow_once(&file_call(kind, &paths), &options(), &NO_TITLES);
        assert_eq!(decision.is_some(), allowed, "{name}");
        if allowed {
            assert_eq!(decision.unwrap().0.as_ref(), "allow", "{name}");
        }
    }
    assert!(
        grant
            .allow_once(
                &file_call(ToolKind::Fetch, &[&new_scratch_file]),
                &options(),
                &NO_TITLES,
            )
            .is_none(),
        "only file kinds take the file rule"
    );
}

#[test]
fn shell_commands_are_allowed_once_only_when_every_operand_is_in_the_scratch_folder() {
    let folders = folders();
    let grant = InFolderGrant::new(&folders.project, &folders.scratch, &[]).unwrap();
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
        (format!("cp {scratch}/a {scratch}/b"), false),
        (format!("find {scratch} -delete"), false),
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
        // Only known value-less options; anything else, such as a relative
        // path glued onto an option, asks.
        (format!("sort -ooutside.txt {scratch}/a"), false),
        (format!("grep -foutside.txt {scratch}/a"), false),
        (format!("cp --target-directory {scratch}/a"), false),
        (format!("head -n 5 {scratch}/a"), false),
        (format!("mkdir -p {scratch}/x/y"), true),
        // Options belong to their utility: `file -f` reads the paths named
        // inside its operand, and `cp -r` follows symbolic links on BSD.
        (format!("file -f {scratch}/m"), false),
        (format!("grep -r {scratch}"), false),
        (format!("cp -f {scratch}/a {scratch}/b"), false),
        (format!("cp -r {scratch}/a {scratch}/b"), false),
        (format!("rmdir -p {scratch}/x/y"), false),
        (format!("cp -R {scratch}/a {scratch}/b"), false),
        // `cp` into a directory writes a derived name that may be a symlink out.
        (format!("cp {scratch}/note {scratch}/dest"), false),
        (format!("mv {scratch}/note {scratch}/dest"), true),
        (format!("ln -s {scratch}/a {scratch}/dest"), true),
        (format!("ln -s {scratch}/a {scratch}/b"), true),
        (format!("rmdir -v {scratch}/x"), true),
    ];
    for (command, allowed) in cases {
        assert_eq!(
            grant
                .allow_once(&shell_call(&command), &options(), &NO_TITLES)
                .is_some(),
            allowed,
            "{command}"
        );
    }
}

#[test]
fn without_one_allow_once_option_the_request_goes_to_approval() {
    let folders = folders();
    let grant = InFolderGrant::new(&folders.project, &folders.scratch, &[]).unwrap();
    let call = file_call(ToolKind::Edit, &[&folders.scratch.join("a")]);
    let no_allow_once = vec![PermissionOption::new(
        "always",
        "Always",
        PermissionOptionKind::AllowAlways,
    )];
    assert!(
        grant
            .allow_once(&call, &no_allow_once, &NO_TITLES)
            .is_none()
    );
    let mut two = options();
    two.push(PermissionOption::new(
        "allow-2",
        "Allow",
        PermissionOptionKind::AllowOnce,
    ));
    assert!(grant.allow_once(&call, &two, &NO_TITLES).is_none());
}

/// Oracle: R4/Q3 — a write through a dangling symlink lands outside the folder
/// once it is created, so neither a file edit nor a shell command may treat
/// the link as an ordinary missing name.
#[test]
fn a_dangling_symlink_final_or_ancestor_asks() {
    let folders = folders();
    let grant = InFolderGrant::new(&folders.project, &folders.scratch, &[]).unwrap();
    let scratch = folders.scratch.display().to_string();
    let dangling_final = folders.scratch.join("dangling");
    let dangling_ancestor = folders.scratch.join("dangling-dir/new");
    for path in [&dangling_final, &dangling_ancestor] {
        assert!(
            grant
                .allow_once(&file_call(ToolKind::Edit, &[path]), &options(), &NO_TITLES)
                .is_none(),
            "edit {}",
            path.display()
        );
    }
    for command in [
        format!("touch {scratch}/dangling"),
        format!("touch {scratch}/dangling-dir/new"),
        format!("mkdir -p {scratch}/dangling-dir/new"),
    ] {
        assert!(
            grant
                .allow_once(&shell_call(&command), &options(), &NO_TITLES)
                .is_none(),
            "{command}"
        );
    }
    // A genuinely absent name inside scratch is still allowed.
    assert!(
        grant
            .allow_once(
                &shell_call(&format!("touch {scratch}/fresh")),
                &options(),
                &NO_TITLES
            )
            .is_some()
    );
}

/// Oracle: R4/Q3 (review N1) — comparing two scratch directories reads their
/// children by name and follows a child link to an outside file, so it asks.
#[test]
fn diff_on_scratch_directories_with_a_child_link_out_asks() {
    let folders = folders();
    let grant = InFolderGrant::new(&folders.project, &folders.scratch, &[]).unwrap();
    let left = folders.scratch.join("left");
    let right = folders.scratch.join("right");
    std::fs::create_dir_all(&left).unwrap();
    std::fs::create_dir_all(&right).unwrap();
    std::fs::write(left.join("note"), "inside").unwrap();
    std::fs::write(folders.root.join("outside/secret"), "outside").unwrap();
    std::os::unix::fs::symlink(folders.root.join("outside/secret"), right.join("note")).unwrap();
    let command = format!("diff {} {}", left.display(), right.display());
    assert!(
        grant
            .allow_once(&shell_call(&command), &options(), &NO_TITLES)
            .is_none(),
        "{command}"
    );
}

/// Oracle: R4/Q3 (review N3) — sort spills large input to temporary files in
/// the system temporary folder, outside scratch, so it asks.
#[test]
fn sort_on_a_scratch_file_asks() {
    let folders = folders();
    let grant = InFolderGrant::new(&folders.project, &folders.scratch, &[]).unwrap();
    let command = format!("sort {}/input", folders.scratch.display());
    assert!(
        grant
            .allow_once(&shell_call(&command), &options(), &NO_TITLES)
            .is_none(),
        "{command}"
    );
}

/// A request whose only named path is a diff's, with an optional title.
fn diff_call(kind: ToolKind, diff_path: &Path, title: &str) -> ToolCallUpdate {
    ToolCallUpdate::new(
        "call",
        ToolCallUpdateFields::new()
            .kind(kind)
            .title(title)
            .content(vec![ToolCallContent::Diff(Diff::new(diff_path, "D1\n"))]),
    )
}

/// Oracle: R4/R6 — a file write names its path in ACP `diff` content when it
/// sends no `locations` (Cursor's writes); that path takes the same rule, and
/// every named path must be inside.
#[test]
fn a_diff_path_is_a_named_path() {
    let folders = folders();
    let grant = InFolderGrant::new(&folders.project, &folders.scratch, &[]).unwrap();
    let inside = folders.scratch.join("note.txt");
    let outside = folders.root.join("outside/note.txt");
    assert!(
        grant
            .allow_once(
                &diff_call(ToolKind::Edit, &inside, "Write"),
                &options(),
                &NO_TITLES
            )
            .is_some(),
        "an in-scratch diff is allowed"
    );
    assert!(
        grant
            .allow_once(
                &diff_call(ToolKind::Edit, &outside, "Write"),
                &options(),
                &NO_TITLES
            )
            .is_none(),
        "a diff outside asks"
    );
    let mixed = ToolCallUpdate::new(
        "call",
        ToolCallUpdateFields::new()
            .kind(ToolKind::Edit)
            .locations(vec![ToolCallLocation::new(inside)])
            .content(vec![ToolCallContent::Diff(Diff::new(&outside, "D1\n"))]),
    );
    assert!(
        grant.allow_once(&mixed, &options(), &NO_TITLES).is_none(),
        "an inside location does not cover a diff outside"
    );
}

/// Oracle: R4/R6 — the target an agent states only in its title counts as a
/// named path or command, under the same rule; one of the wrong class asks.
#[test]
fn a_stated_target_takes_the_same_rule() {
    let folders = folders();
    let grant = InFolderGrant::new(&folders.project, &folders.scratch, &[]).unwrap();
    let inside = folders.scratch.join("note.txt");
    let outside = folders.root.join("outside/note.txt");
    let titled = |kind: ToolKind| {
        ToolCallUpdate::new(
            "call",
            ToolCallUpdateFields::new().kind(kind).title("titled"),
        )
    };
    let file = |path: PathBuf| {
        move |action: PermissionRequestAction, _: &str| {
            (action == PermissionRequestAction::FileAccess)
                .then(|| StatedPermissionTarget::File(path.clone()))
        }
    };
    let command = |line: String| {
        move |action: PermissionRequestAction, _: &str| {
            (action == PermissionRequestAction::ShellCommand)
                .then(|| StatedPermissionTarget::Command(line.clone()))
        }
    };
    let as_command =
        |_: PermissionRequestAction, _: &str| Some(StatedPermissionTarget::Command("ls".into()));
    let as_file = |_: PermissionRequestAction, _: &str| {
        Some(StatedPermissionTarget::File(PathBuf::from("/tmp")))
    };
    let scratch = folders.scratch.display();
    let cases: [(&str, ToolCallUpdate, &super::StatedTargetReader<'_>, bool); 7] = [
        (
            "stated file inside",
            titled(ToolKind::Edit),
            &file(inside.clone()),
            true,
        ),
        (
            "stated file outside",
            titled(ToolKind::Edit),
            &file(outside.clone()),
            false,
        ),
        (
            "stated read inside",
            titled(ToolKind::Read),
            &file(inside.clone()),
            true,
        ),
        (
            "diff inside, stated outside",
            diff_call(ToolKind::Edit, &inside, "titled"),
            &file(outside),
            false,
        ),
        (
            "a command for a file",
            titled(ToolKind::Edit),
            &as_command,
            false,
        ),
        (
            "stated command in scratch",
            titled(ToolKind::Execute),
            &command(format!("rm {scratch}/note.txt")),
            true,
        ),
        (
            "a file for a command",
            titled(ToolKind::Execute),
            &as_file,
            false,
        ),
    ];
    for (case, call, read_title, allowed) in cases {
        assert_eq!(
            grant.allow_once(&call, &options(), read_title).is_some(),
            allowed,
            "{case}"
        );
    }
    let raw_inside_stated_outside = ToolCallUpdate::new(
        "call",
        ToolCallUpdateFields::new()
            .kind(ToolKind::Execute)
            .title("titled")
            .raw_input(serde_json::json!({ "command": format!("rm {scratch}/note.txt") })),
    );
    assert!(
        grant
            .allow_once(
                &raw_inside_stated_outside,
                &options(),
                &command("rm /tmp/d1-outside".to_owned())
            )
            .is_none(),
        "every named command must stay in scratch"
    );
}

/// Oracle: D1 R18 and V33, "paths inside any root are allowed; outside
/// asks", with "only working + scratch counted" as the wrong build: a file
/// action or a file utility's operand inside a work root is allowed once; one
/// outside every folder still asks; a work root missing on this computer is
/// left out without dropping the grant.
#[test]
fn work_roots_widen_the_inside_for_file_actions_and_command_operands() {
    // Arrange
    let folders = folders();
    let work_root = folders.root.join("worktrees");
    std::fs::create_dir_all(&work_root).unwrap();
    let missing_root = folders.root.join("not-on-this-computer");
    let grant = InFolderGrant::new(
        &folders.project,
        &folders.scratch,
        &[work_root.clone(), missing_root],
    )
    .expect("a grant despite a missing root");
    let in_root = work_root.join("other-checkout/src/lib.rs");
    let outside = folders.root.join("outside/file");
    let cases = [
        (
            "edit in a work root",
            file_call(ToolKind::Edit, &[&in_root]),
            true,
        ),
        (
            "edit outside",
            file_call(ToolKind::Edit, &[&outside]),
            false,
        ),
        (
            "utility on a work-root path",
            shell_call(&format!("rm -rf {}", in_root.display())),
            true,
        ),
        (
            "utility outside",
            shell_call(&format!("rm -rf {}", outside.display())),
            false,
        ),
    ];

    for (case, call, allowed) in cases {
        // Act
        let answered = grant.allow_once(&call, &options(), &NO_TITLES);

        // Assert
        assert_eq!(answered.is_some(), allowed, "{case}");
    }
}

/// Oracle: Lead V3 and design note §11.3, a held mode change: run A's
/// confirmed change to auto, arriving after A ended and run B started with
/// its grant off, never turns B's allowance on.
#[test]
fn an_ended_runs_mode_change_never_turns_on_the_newer_runs_grant() {
    use crate::acp_transport::TransportRun;
    use provider_session_machines::{
        SessionGeneration,
        session_owner::{OwnerIncarnation, TurnMembership},
    };
    use std::num::NonZeroUsize;

    let folders = folders();
    let (wire, _output) = crate::owner_wire::OwnerAcpWire::new(
        NonZeroUsize::MIN,
        std::sync::Arc::new(crate::test_behavior::TestProviderBehavior),
    );
    let first = TransportRun {
        incarnation: OwnerIncarnation(uuid::Uuid::now_v7()),
        generation: SessionGeneration::from_number(1),
    };
    let second = TransportRun {
        generation: SessionGeneration::from_number(2),
        ..first
    };
    wire.open_route(first, "session-a".into());
    wire.apply_membership(first.incarnation, TurnMembership::Retire(first.generation));
    wire.open_route(second, "session-a".into());
    wire.activate_route(second);
    let grant = InFolderGrant::new(&folders.project, &folders.scratch, &[]).expect("grant");
    wire.register_grant(
        second,
        super::SessionInFolderGrant {
            grant: grant.clone(),
            router_auto: false,
        },
    );

    wire.register_grant(
        first,
        super::SessionInFolderGrant {
            grant: grant.clone(),
            router_auto: true,
        },
    );
    assert!(wire.in_folder_grant(second).is_none());
    wire.register_grant(
        second,
        super::SessionInFolderGrant {
            grant,
            router_auto: true,
        },
    );
    assert!(wire.in_folder_grant(second).is_some());
}
