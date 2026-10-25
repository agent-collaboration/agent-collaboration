//! The verifier's own apparatus: observation paging and parser bounds, the fixture
//! materializer, native plugin facts and the recorder reader.
//!
//! Fixture cases use only a throwaway Git repository in a temporary directory with
//! signing and hooks disabled; they prove the apparatus, never client or product
//! behavior.
use crate::command_reload_probe::recorded_second_skill_bytes;
use crate::native_exports::{read_bounded_bytes, read_session_recorder};
use crate::native_file_facts::native_plugin_facts;
use crate::native_fixture::{
    CANARY_PLACEHOLDER, fixture_relative_files, fixture_source, fixture_worktree_state,
    materialize_fixture,
};
use crate::router_evidence::{
    OBSERVE_MAX_BYTES, OBSERVE_MAX_EVENTS, observe_arguments, paginate_observation,
};
use crate::router_queue_observation::queued_input_ids;
use crate::verification_models::{
    NativeCapabilitiesPlan, NativePluginRow, ObservationCursor, ObserverIdentity,
    ProviderObservation,
};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
use tempfile::TempDir;

fn page(sequences: &[u64], end: &str, epoch: u64) -> ProviderObservation {
    let events: Vec<_> = sequences
        .iter()
        .map(|sequence| json!({"sequence": sequence, "event": {"kind": "turnStarted", "turnId": format!("t{sequence}")}}))
        .collect();
    serde_json::from_value(json!({
        "target": {"endpoint": {"serviceId": "service", "endpointId": "claude-local"}, "sessionId": "thread"},
        "events": events,
        "epoch": epoch,
        "endReason": end,
    }))
    .unwrap()
}

fn plan() -> NativeCapabilitiesPlan {
    let binary = json!({"path": "/candidate/bin", "sha256": "00"});
    let process =
        json!({"pid": 1, "realUid": 501, "group": 1, "birth": "b", "executable": "/candidate/bin"});
    serde_json::from_value(json!({
        "repository": "/repo",
        "sourceHead": "0".repeat(40),
        "binaries": {"codexRouter": binary, "agentCollaboration": binary, "agentSessions": binary},
        "routerRoot": "/router-root",
        "endpointSocket": "/router-root/agent-communication/router-sessions/claude-local.sock",
        "binaryQualification": "/build/record.json",
        "routerPort": 18080,
        "expectedVersion": "0.0.0",
        "routerProcess": process,
        "hostProcess": process,
        "codexCli": binary,
        "nativeExecutable": binary,
        "endpointId": "claude-local",
        "sessionId": "session",
        "fixture": "/fixture.json",
        "nativeHomeSkillRoots": [],
        "nativeWorkspaceSkillDirs": [".claude/skills"],
    }))
    .unwrap()
}

fn argument<'a>(arguments: &'a [String], flag: &str) -> &'a str {
    let index = arguments.iter().position(|word| word == flag).unwrap();
    &arguments[index + 1]
}

#[test]
fn observation_command_stays_inside_the_cli_parser_bounds() {
    // event_commands.rs: --max-events 1..=4096, --max-bytes 1..=1048576.
    let first = observe_arguments(&plan(), None);
    assert!(argument(&first, "--max-events").parse::<u32>().unwrap() <= 4096);
    assert!(argument(&first, "--max-bytes").parse::<u32>().unwrap() <= 1_048_576);
    assert_eq!((OBSERVE_MAX_EVENTS, OBSERVE_MAX_BYTES), (4096, 1_048_576));
    assert!(
        !first
            .iter()
            .any(|word| word == "--after-sequence" || word == "--epoch")
    );
    assert_eq!(
        argument(&first, "--service-directory"),
        "/router-root/agent-communication"
    );
}

#[test]
fn continuation_carries_the_observed_epoch() {
    // --after-sequence `requires = "epoch"` in the parser.
    let command = observe_arguments(
        &plan(),
        Some(ObservationCursor {
            after_sequence: 12,
            epoch: 3,
        }),
    );
    assert_eq!(argument(&command, "--after-sequence"), "12");
    assert_eq!(argument(&command, "--epoch"), "3");
}

#[test]
fn result_limited_pages_are_followed_to_the_deadline() {
    let pages = [
        page(&[1, 2], "resultLimitReached", 3),
        page(&[3], "deadlineReached", 3),
    ];
    let mut cursors = Vec::new();
    let merged = paginate_observation(
        |cursor, index| {
            cursors.push(cursor);
            Ok(pages[index].clone())
        },
        None,
    )
    .unwrap();
    assert_eq!(
        merged
            .events
            .iter()
            .map(|entry| entry.sequence)
            .collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(merged.end_reason, "deadlineReached");
    assert_eq!(
        cursors,
        [
            None,
            Some(ObservationCursor {
                after_sequence: 2,
                epoch: 3
            })
        ]
    );
}

#[test]
fn incomplete_observations_never_pass_as_complete() {
    let start = Some(ObservationCursor {
        after_sequence: 5,
        epoch: 3,
    });
    let endless: Vec<_> = (1..70)
        .map(|index| page(&[index], "resultLimitReached", 3))
        .collect();
    let cases = [
        (
            vec![page(&[6], "deadlineReached", 4)],
            start,
            "epoch changed",
        ),
        (vec![page(&[1], "resyncRequired", 3)], None, "resync"),
        (vec![page(&[], "resultLimitReached", 3)], None, "resync"),
        (endless, None, "page bound"),
    ];
    for (pages, cursor, fragment) in cases {
        let error = paginate_observation(|_, index| Ok(pages[index].clone()), cursor).unwrap_err();
        assert!(error.contains(fragment), "{error:?}");
    }
}

fn git(directory: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "user.name=verifier",
        ])
        .args(["-c", "user.email=verifier@example.invalid", "-C"])
        .arg(directory)
        .args(arguments)
        .output()
        .unwrap();
    assert!(
        status.status.success(),
        "git {arguments:?}: {}",
        String::from_utf8_lossy(&status.stderr)
    );
}

/// A throwaway main repository and a genuine detached linked worktree of it.
struct WorktreeSetup {
    _directory: TempDir,
    root: PathBuf,
    main: PathBuf,
    worktree: PathBuf,
    home: PathBuf,
}

impl WorktreeSetup {
    fn new(tracked: &[(&str, &str)]) -> Self {
        let directory = TempDir::new().unwrap();
        let root = fs::canonicalize(directory.path()).unwrap();
        let main = root.join("main");
        fs::create_dir(&main).unwrap();
        git(&main, &["init", "-q"]);
        for (path, contents) in tracked {
            fs::create_dir_all(main.join(path).parent().unwrap()).unwrap();
            fs::write(main.join(path), contents).unwrap();
            git(&main, &["add", path]);
        }
        git(
            &main,
            &["commit", "-q", "--allow-empty", "-m", "fixture base"],
        );
        let worktree = root.join("worktree");
        git(
            &main,
            &[
                "worktree",
                "add",
                "-q",
                "--detach",
                worktree.to_str().unwrap(),
            ],
        );
        Self {
            home: root.join("home"),
            _directory: directory,
            root,
            main,
            worktree,
        }
    }

    fn templates(&self) -> PathBuf {
        fixture_source(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../..")))
    }

    fn materialize(
        &self,
        workspace: &Path,
        empty: &str,
        record: &str,
    ) -> Result<crate::verification_models::FixtureRecord, String> {
        materialize_fixture(
            &self.templates(),
            workspace,
            &self.root.join(empty),
            &self.root.join(record),
            &self.home,
        )
    }
}

#[test]
fn materialized_skills_carry_fresh_distinct_canaries_in_the_prepared_worktree() {
    let setup = WorktreeSetup::new(&[]);
    let fixture = setup
        .materialize(&setup.worktree, "empty", "record.json")
        .unwrap();
    let body = fs::read_to_string(&fixture.skill_file).unwrap();
    let second = fs::read_to_string(&fixture.second_skill_file).unwrap();
    assert!(body.contains(&fixture.canary) && second.contains(&fixture.second_canary));
    assert_ne!(fixture.canary, fixture.second_canary);
    assert!(!format!("{body}{second}").contains(CANARY_PLACEHOLDER));
    assert!(body.contains("$ARGUMENTS"));
    assert_eq!(
        fixture.skill_file,
        setup
            .worktree
            .join(".claude/skills/router-native-arguments/SKILL.md")
    );
    assert_eq!(fs::read_dir(setup.root.join("empty")).unwrap().count(), 0);
    let mode = fs::metadata(setup.root.join("record.json"))
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    // The worktree adds exactly the declared files on top of its recorded commit.
    let (head, changes) = fixture_worktree_state(&setup.worktree).unwrap();
    assert_eq!(head, fixture.workspace_head);
    assert_eq!(changes, fixture_relative_files(&fixture));
    // The reload probe removes the second skill only when it still holds exactly these
    // recorded bytes, and restores exactly them.
    let recorded = recorded_second_skill_bytes(crate::repository_root(), &fixture).unwrap();
    assert_eq!(
        fs::read_to_string(&fixture.second_skill_file).unwrap(),
        recorded
    );
    assert!(recorded.contains(&fixture.second_canary));
}

#[test]
fn only_a_clean_genuine_linked_worktree_is_accepted() {
    let setup = WorktreeSetup::new(&[]);
    // The main checkout is not a linked worktree.
    let error = setup
        .materialize(&setup.main, "empty-main", "main.json")
        .unwrap_err();
    assert!(error.contains("not a linked Git worktree"), "{error}");
    // A plain directory with a forged pointer file is not one either.
    let forged = setup.root.join("forged");
    fs::create_dir(&forged).unwrap();
    fs::write(forged.join(".git"), "gitdir: /nonexistent\n").unwrap();
    assert!(
        setup
            .materialize(&forged, "empty-forged", "forged.json")
            .is_err()
    );
    // A worktree with any change beyond its commit is refused before writing.
    fs::write(setup.worktree.join("stray.txt"), "stray").unwrap();
    let error = setup
        .materialize(&setup.worktree, "empty-dirty", "dirty.json")
        .unwrap_err();
    assert!(error.contains("not clean"), "{error}");
    assert!(!setup.root.join("empty-dirty").exists());
}

#[test]
fn materialization_never_overwrites_an_existing_file() {
    let path = ".claude/skills/router-native-arguments/SKILL.md";
    let setup = WorktreeSetup::new(&[(path, "committed\n")]);
    let error = setup
        .materialize(&setup.worktree, "empty", "record.json")
        .unwrap_err();
    assert!(error.contains("cannot create"), "{error}");
    assert_eq!(
        fs::read_to_string(setup.worktree.join(path)).unwrap(),
        "committed\n"
    );
    assert!(!setup.root.join("record.json").exists());
}

#[test]
fn comparison_directory_is_fresh_and_owned_apart() {
    let setup = WorktreeSetup::new(&[]);
    fs::create_dir(setup.root.join("taken")).unwrap();
    assert!(
        setup
            .materialize(&setup.worktree, "taken", "taken.json")
            .is_err()
    );
    let inside = setup.worktree.join("empty");
    let error = materialize_fixture(
        &setup.templates(),
        &setup.worktree,
        &inside,
        &setup.root.join("r.json"),
        &setup.home,
    )
    .unwrap_err();
    assert!(error.contains("apart from the worktree"), "{error}");
    let in_home = setup.home.join("worktree");
    let error = materialize_fixture(
        &setup.templates(),
        &in_home,
        &setup.root.join("e"),
        &setup.root.join("h.json"),
        &setup.home,
    )
    .unwrap_err();
    assert!(error.contains("outside the home directory"), "{error}");
}

fn plugin(root: &Path) -> NativePluginRow {
    let install = root.join("alpha");
    fs::create_dir_all(install.join(".claude-plugin")).unwrap();
    fs::create_dir_all(install.join("skills/review")).unwrap();
    fs::write(
        install.join("skills/review/SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nBody\n",
    )
    .unwrap();
    NativePluginRow {
        id: "alpha@market".to_owned(),
        enabled: true,
        install_path: install,
        read_from_folder: None,
        version: None,
        scope: None,
    }
}

#[test]
fn declared_and_absent_plugin_components() {
    let directory = TempDir::new().unwrap();
    let row = plugin(directory.path());
    fs::write(
        row.install_path.join(".claude-plugin/plugin.json"),
        json!({"name": "alpha", "mcpServers": {"search": {}}}).to_string(),
    )
    .unwrap();
    fs::create_dir(row.install_path.join("hooks")).unwrap();
    fs::write(
        row.install_path.join("hooks/hooks.json"),
        json!({"hooks": {"PreToolUse": [], "Stop": []}}).to_string(),
    )
    .unwrap();
    let facts = native_plugin_facts(&row).unwrap();
    assert_eq!(
        facts.skill_descriptions,
        BTreeMap::from([("review".to_owned(), "Review code".to_owned())])
    );
    assert_eq!(
        facts.hook_events,
        Some(vec!["PreToolUse".to_owned(), "Stop".to_owned()])
    );
    assert_eq!(facts.mcp_servers, Some(vec!["search".to_owned()]));
}

#[test]
fn no_declaration_is_known_empty_and_unreadable_is_unobserved() {
    let directory = TempDir::new().unwrap();
    let row = plugin(directory.path());
    fs::write(
        row.install_path.join(".claude-plugin/plugin.json"),
        json!({"name": "alpha", "hooks": "missing/hooks.json"}).to_string(),
    )
    .unwrap();
    let facts = native_plugin_facts(&row).unwrap();
    assert_eq!(facts.hook_events, None);
    assert_eq!(facts.mcp_servers, Some(Vec::new()));
}

#[test]
fn multi_line_description_is_not_guessed() {
    let directory = TempDir::new().unwrap();
    let row = plugin(directory.path());
    fs::write(
        row.install_path.join("skills/review/SKILL.md"),
        "---\nname: review\ndescription: >\n  folded\n---\n",
    )
    .unwrap();
    assert!(
        native_plugin_facts(&row)
            .unwrap_err()
            .contains("not one line")
    );
}

#[test]
fn recorder_reader_requires_the_clients_own_log_within_its_bound() {
    let directory = TempDir::new().unwrap();
    let log = directory.path().join("session.jsonl");
    assert!(read_session_recorder(&log).is_err());
    fs::write(&log, "{\"dir\":\"meta\",\"kind\":\"session_start\"}\n\n{\"dir\":\"from_tui\",\"kind\":\"op\",\"payload\":{}}\n").unwrap();
    let kinds: Vec<_> = read_session_recorder(&log)
        .unwrap()
        .iter()
        .map(|record| record["kind"].clone())
        .collect();
    assert_eq!(kinds, [json!("session_start"), json!("op")]);
    assert!(read_bounded_bytes(&log, 8).unwrap_err().contains("bound"));
}

#[test]
fn queue_list_reply_is_ids_in_order_and_an_error_is_never_an_empty_queue() {
    let listed = json!({"jsonrpc": "2.0", "id": 2, "result": {"items": [
        {"inputId": "input-1", "position": 0, "preview": "a"},
        {"inputId": "input-2", "position": 1, "preview": "b"},
    ]}});
    assert_eq!(queued_input_ids(&listed).unwrap(), ["input-1", "input-2"]);
    let empty = json!({"jsonrpc": "2.0", "id": 2, "result": {"items": []}});
    assert_eq!(queued_input_ids(&empty).unwrap(), Vec::<String>::new());
    let refused = json!({"jsonrpc": "2.0", "id": 2, "error": {"code": -32002, "message": "Session not found"}});
    assert!(
        queued_input_ids(&refused)
            .unwrap_err()
            .contains("unobservable")
    );
    assert!(queued_input_ids(&json!({"jsonrpc": "2.0", "id": 2, "result": {}})).is_err());
}

#[test]
fn queue_observer_is_only_the_verifiers_own_session() {
    let session = json!({"kind": "session", "session": {
        "endpoint": {"serviceId": "service", "endpointId": "claude-local"}, "sessionId": "verifier",
    }});
    let observer: ObserverIdentity = serde_json::from_value(session.clone()).unwrap();
    // Presented unchanged as the ACP actor (message_board Identity::Session shape).
    assert_eq!(serde_json::to_value(&observer).unwrap(), session);
    let human = json!({"kind": "human", "humanId": "owner"});
    assert!(serde_json::from_value::<ObserverIdentity>(human).is_err());
}
