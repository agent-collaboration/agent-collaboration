//! The Router's work-roots hook through its real contract (D1 R18, V33): the
//! command the Router hands Claude runs under `sh -c` with a PreToolUse call
//! on stdin, and answers "ask" on stdout for a path outside the roots, or
//! nothing.

use super::{AskOutsideHook, install_work_roots_hook, session_work_roots};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

struct HookRig {
    _root: tempfile::TempDir,
    home: PathBuf,
    inside: PathBuf,
    outside: PathBuf,
    hook: AskOutsideHook,
}

fn rig() -> HookRig {
    let root = tempfile::tempdir().expect("hook root");
    let home = root.path().join("home");
    let inside = root.path().join("work root");
    let outside = root.path().join("outside");
    for folder in [&home, &inside, &outside] {
        std::fs::create_dir_all(folder).expect("folder");
    }
    let hook = install_work_roots_hook(&root.path().join("hooks")).expect("install the hook");
    HookRig {
        _root: root,
        home,
        inside,
        outside,
        hook: AskOutsideHook {
            node: node_test_fixture::node_executable().expect("Node"),
            hook,
        },
    }
}

/// What the hook printed for one call.
#[derive(Debug, Eq, PartialEq)]
enum Answered {
    Nothing,
    Ask(String),
}

impl Answered {
    fn is_ask(&self) -> bool {
        matches!(self, Self::Ask(_))
    }
}

/// Runs the Router's command for `roots` with `call` on stdin; returns what
/// it printed.
fn answer(rig: &HookRig, roots: &[PathBuf], call: &serde_json::Value) -> Answered {
    let mut child = Command::new("/bin/sh")
        .arg("-c")
        .arg(rig.hook.command(roots))
        .env("HOME", &rig.home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run the hook");
    child
        .stdin
        .take()
        .expect("hook stdin")
        .write_all(call.to_string().as_bytes())
        .expect("write the call");
    let output = child.wait_with_output().expect("hook output");
    assert!(
        output.status.success(),
        "the hook exits 0: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    if output.stdout.is_empty() {
        return Answered::Nothing;
    }
    let printed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("the hook prints JSON");
    assert_eq!(printed["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(printed["hookSpecificOutput"]["permissionDecision"], "ask");
    Answered::Ask(
        printed["hookSpecificOutput"]["permissionDecisionReason"]
            .as_str()
            .expect("a reason")
            .to_owned(),
    )
}

fn write_call(cwd: &Path, file: &Path) -> serde_json::Value {
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "cwd": cwd,
        "tool_input": {"file_path": file, "content": "x"},
    })
}

fn bash_call(cwd: &Path, command: &str) -> serde_json::Value {
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Bash",
        "cwd": cwd,
        "tool_input": {"command": command},
    })
}

/// Oracle: R18, "inside the work roots the agent works under its own auto":
/// a write inside a root gets no answer, even through a root whose name has a
/// space and a quote (the command's quoting holds).
#[test]
fn a_write_inside_a_root_gets_no_answer() {
    // Arrange
    let rig = rig();
    let quoted = rig.inside.join("it's here");
    std::fs::create_dir_all(&quoted).expect("quoted root");

    // Act
    let answered = answer(
        &rig,
        std::slice::from_ref(&quoted),
        &write_call(&quoted, &quoted.join("new.txt")),
    );

    // Assert
    assert_eq!(answered, Answered::Nothing);
}

/// Oracle: R18, "outside them, a write or a shell command asks": a write
/// outside every root is answered ask, naming the path.
#[test]
fn a_write_outside_every_root_asks() {
    // Arrange
    let rig = rig();
    let target = rig.outside.join("f.txt");

    // Act
    let answered = answer(
        &rig,
        std::slice::from_ref(&rig.inside),
        &write_call(&rig.inside, &target),
    );

    // Assert
    let Answered::Ask(reason) = answered else {
        panic!("a write outside asks: {answered:?}");
    };
    assert!(
        reason.contains("outside the Router's work roots"),
        "{reason}"
    );
}

/// Oracle: R18 with V33's "paths inside any root are allowed; outside asks":
/// a path that is inside by name but leads out through a symlink asks.
#[test]
fn a_symlink_out_of_a_root_asks() {
    // Arrange
    let rig = rig();
    let link = rig.inside.join("escape");
    std::os::unix::fs::symlink(&rig.outside, &link).expect("symlink out");

    // Act
    let answered = answer(
        &rig,
        std::slice::from_ref(&rig.inside),
        &write_call(&rig.inside, &link.join("f.txt")),
    );

    // Assert
    assert!(answered.is_ask(), "a symlink out of the root asks");
}

/// Oracle: R18, "a write or a shell command asks" outside the roots, and
/// "inside ... the agent's own auto" decides: a shell command naming an
/// outside path (a redirect, a cd, a relative escape) asks; one naming only
/// paths inside, or none, gets no answer.
#[test]
fn shell_commands_ask_only_when_they_name_an_outside_path() {
    // Arrange
    let rig = rig();
    let roots = [rig.inside.clone()];
    let outside = rig.outside.display().to_string();
    let asking = [
        format!("printf x > {outside}/f.txt"),
        format!("cd {outside} && rm -rf build"),
        "rm -rf ../outside/f.txt".to_owned(),
        format!("cp a.txt \"{outside}/b.txt\""),
    ];
    let quiet = [
        "cargo test -p codex-router-host".to_owned(),
        "ls ./src && cat notes.md 2>/dev/null".to_owned(),
        format!("touch '{}/new.txt'", rig.inside.display()),
        "curl -s https://example.com/x".to_owned(),
    ];

    for command in &asking {
        // Act
        let answered = answer(&rig, &roots, &bash_call(&rig.inside, command));

        // Assert
        assert!(answered.is_ask(), "asks: {command}");
    }
    for command in &quiet {
        // Act
        let answered = answer(&rig, &roots, &bash_call(&rig.inside, command));

        // Assert
        assert_eq!(answered, Answered::Nothing, "no answer: {command}");
    }
}

/// Oracle: R18 (owner, 16 Nov: the Router's roots are added to each agent's
/// own global settings, never replacing them): a folder the owner's own
/// Claude settings grant as an additional directory counts as inside.
#[test]
fn an_owner_granted_additional_directory_counts_as_inside() {
    // Arrange
    let rig = rig();
    std::fs::create_dir_all(rig.home.join(".claude")).expect("claude settings folder");
    std::fs::write(
        rig.home.join(".claude").join("settings.json"),
        serde_json::json!({"permissions": {"additionalDirectories": [rig.outside]}}).to_string(),
    )
    .expect("owner settings");

    // Act
    let answered = answer(
        &rig,
        std::slice::from_ref(&rig.inside),
        &write_call(&rig.inside, &rig.outside.join("granted.txt")),
    );

    // Assert
    assert_eq!(
        answered,
        Answered::Nothing,
        "the owner's own grant stays inside"
    );
}

/// Oracle: the hook's rule "input it can't read for certain is answered
/// ask": an unterminated quote in a command, and a call with no working
/// folder, ask.
#[test]
fn input_it_cannot_read_asks() {
    // Arrange
    let rig = rig();
    let roots = [rig.inside.clone()];
    let no_cwd = serde_json::json!({"tool_name": "Write", "tool_input": {"file_path": "/x"}});

    // Act
    let unterminated = answer(&rig, &roots, &bash_call(&rig.inside, "echo 'unterminated"));
    let unplaced = answer(&rig, &roots, &no_cwd);

    // Assert
    assert!(unterminated.is_ask(), "an unreadable command asks");
    assert!(unplaced.is_ask(), "a call with no working folder asks");
}

/// Oracle: R18's session list, "the session's project tmp/ ... any roots its
/// creator names at start": the configured roots, then the project's tmp/,
/// then the named roots, each once.
#[test]
fn a_sessions_roots_add_its_project_tmp_and_named_roots_once() {
    // Arrange
    let configured = vec![PathBuf::from("/a"), PathBuf::from("/b")];
    let named = vec![PathBuf::from("/b"), PathBuf::from("/c")];

    // Act
    let roots = session_work_roots(&configured, Path::new("/project"), &named);

    // Assert
    assert_eq!(
        roots,
        vec![
            PathBuf::from("/a"),
            PathBuf::from("/b"),
            PathBuf::from("/project/tmp"),
            PathBuf::from("/c"),
        ]
    );
}

/// Oracle: D1 R18 and the Verifier's f11cb0c38 finding: a relative path is a
/// path in the call's working folder. With that folder outside the roots, a
/// relative redirection and a relative Write ask; with it inside, the same
/// calls get no answer; and a command that changes into an outside folder
/// before writing asks.
#[test]
fn relative_paths_are_judged_in_the_calls_working_folder() {
    // Arrange
    let rig = rig();
    let roots = [rig.inside.clone()];
    let relative_write = |cwd: &Path| {
        serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Write",
            "cwd": cwd,
            "tool_input": {"file_path": "relative.txt", "content": "x"},
        })
    };
    let change_then_write = format!("cd {} && printf x > x.txt", rig.outside.display());

    // Act
    let outside_redirect = answer(
        &rig,
        &roots,
        &bash_call(&rig.outside, "printf x > bare.txt"),
    );
    let outside_write = answer(&rig, &roots, &relative_write(&rig.outside));
    let inside_redirect = answer(&rig, &roots, &bash_call(&rig.inside, "printf x > bare.txt"));
    let inside_write = answer(&rig, &roots, &relative_write(&rig.inside));
    let changed_folder = answer(&rig, &roots, &bash_call(&rig.inside, &change_then_write));

    // Assert
    assert!(
        outside_redirect.is_ask(),
        "a relative redirection outside asks"
    );
    assert!(outside_write.is_ask(), "a relative Write outside asks");
    assert_eq!(
        inside_redirect,
        Answered::Nothing,
        "a relative redirection inside"
    );
    assert_eq!(inside_write, Answered::Nothing, "a relative Write inside");
    assert!(
        changed_folder.is_ask(),
        "cd to an outside folder, then a write, asks"
    );
}

/// Oracle: the Lead's 16 Nov rule for a working folder outside the roots:
/// a command that could write (a redirection, or a program that isn't
/// read-only) asks; a read-only one with nothing redirected gets no answer.
#[test]
fn outside_the_roots_only_read_only_commands_pass() {
    // Arrange
    let rig = rig();
    let roots = [rig.inside.clone()];

    // Act
    let listing = answer(&rig, &roots, &bash_call(&rig.outside, "ls"));
    let building = answer(&rig, &roots, &bash_call(&rig.outside, "cargo build"));
    let piped = answer(&rig, &roots, &bash_call(&rig.outside, "echo x | tee"));

    // Assert
    assert_eq!(listing, Answered::Nothing, "ls only reads");
    assert!(building.is_ask(), "cargo build could write");
    assert!(piped.is_ask(), "tee could write");
}

/// Oracle: R18's outside-write rule and the Reviewer's D2: a symlink inside a
/// root whose target doesn't exist yet (a write through it would create the
/// file outside) can't be resolved, so the write asks.
#[test]
fn a_dangling_symlink_inside_a_root_asks() {
    // Arrange
    let rig = rig();
    let link = rig.inside.join("dangling");
    std::os::unix::fs::symlink(rig.outside.join("not-yet"), &link).expect("dangling symlink");

    // Act
    let written = answer(
        &rig,
        std::slice::from_ref(&rig.inside),
        &write_call(&rig.inside, &link),
    );
    let redirected = answer(
        &rig,
        std::slice::from_ref(&rig.inside),
        &bash_call(&rig.inside, "printf x > dangling"),
    );

    // Assert
    assert!(written.is_ask(), "a Write through a dangling symlink asks");
    assert!(
        redirected.is_ask(),
        "a redirection through a dangling symlink asks"
    );
}
