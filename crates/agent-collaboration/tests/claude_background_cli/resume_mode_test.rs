//! Resume mode and automatic ownership through the public CLI, without native model calls.
use super::*;
use agent_session_inventory::sessions::SessionsCommand;
use serde_json::json;

#[test]
fn resume_selector_accepts_only_a_complete_first_positional_uuid() {
    let command = SessionsCommand::parse(vec![SESSION.into(), "--dry-run".into()]).unwrap();
    assert_eq!(command.id.as_deref(), Some(SESSION));
    for arguments in [
        vec!["019ff05e77c078318f6840bf182509f6"],
        vec!["019ff05g-77c0-7831-8f68-40bf182509f6"],
        vec!["--local", SESSION],
        vec!["--", SESSION],
        vec!["--model", "codex", SESSION],
        vec!["deadbeef"],
    ] {
        let command = SessionsCommand::parse(arguments.iter().map(Into::into).collect())
            .expect("other positionals remain native Codex arguments");
        assert!(command.id.is_none(), "{arguments:?}");
        let expected = arguments
            .iter()
            .filter(|value| !["--", "--local"].contains(value))
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>();
        assert_eq!(command.codex_args, expected, "{arguments:?}");
    }
}

#[test]
fn resume_job_handle_is_explicit_and_new_requires_a_provider() {
    for provider in [vec![], vec!["--provider", "claude"]] {
        let mut arguments = provider;
        arguments.extend(["--id", "deadbeef"]);
        let command = SessionsCommand::parse(arguments.iter().map(Into::into).collect())
            .expect("a native background job handle is valid with --id");
        assert_eq!(command.id.as_deref(), Some("deadbeef"));
    }
    let error = SessionsCommand::parse(vec!["--new".into()]).unwrap_err();
    assert!(error.contains("--provider"), "{error}");
    for invalid in [
        "thread-name",
        "abcd",
        "deadbee",
        "deadbeeg",
        "not-a-job",
        "--malicious",
        "dead beef",
    ] {
        let error = SessionsCommand::parse(vec!["--id".into(), invalid.into()]).unwrap_err();
        assert!(
            error.contains("UUID") && error.contains("job handle"),
            "{error}"
        );
    }
}

fn write_jobs(
    fixture: &ClaudeProcessFixture,
    jobs: serde_json::Value,
) -> Result<(), Box<dyn std::error::Error>> {
    fs::write(
        fixture.root.path().join("jobs.json"),
        serde_json::to_vec(&jobs)?,
    )?;
    Ok(())
}

fn seed_transcript(fixture: &ClaudeProcessFixture) -> Result<(), Box<dyn std::error::Error>> {
    let project = fixture.root.path().join("home/.claude/projects/fixture");
    fs::create_dir_all(&project)?;
    fs::write(
        project.join(format!("{SESSION}.jsonl")),
        format!(
            "{}\n",
            json!({"sessionId":SESSION,"cwd":fixture.working_directory})
        ),
    )?;
    Ok(())
}

async fn seed_codex(
    fixture: &ClaudeProcessFixture,
    present: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let home = fixture.root.path().join("home/.codex");
    fs::create_dir_all(&home)?;
    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            sqlx::sqlite::SqliteConnectOptions::new()
                .filename(home.join("state_5.sqlite"))
                .create_if_missing(true),
        )
        .await?;
    sqlx::query("CREATE TABLE threads (id TEXT PRIMARY KEY, rollout_path TEXT, cwd TEXT, model_provider TEXT, model TEXT, reasoning_effort TEXT, source TEXT, thread_source TEXT, git_branch TEXT, git_origin_url TEXT, name TEXT, title TEXT, preview TEXT, first_user_message TEXT, created_at_ms INTEGER, updated_at_ms INTEGER, archived INTEGER NOT NULL DEFAULT 0, has_user_event INTEGER)")
        .execute(&pool).await?;
    for statement in [
        "CREATE INDEX idx_threads_created_at_ms ON threads(created_at_ms DESC, id DESC)",
        "CREATE INDEX idx_threads_updated_at_ms ON threads(updated_at_ms DESC, id DESC)",
    ] {
        sqlx::query(statement).execute(&pool).await?;
    }
    if present {
        sqlx::query("INSERT INTO threads(id,cwd,model_provider,source,created_at_ms,updated_at_ms,has_user_event) VALUES(?,?,'codex-router','cli',1,1,1)")
            .bind(SESSION).bind(fixture.working_directory.to_str()).execute(&pool).await?;
    }
    pool.close().await;
    Ok(())
}

fn selected_command(fixture: &ClaudeProcessFixture, arguments: &[&str]) -> Command {
    let mut command = attach_command(fixture);
    command
        .env("CODEX_HOME", fixture.root.path().join("home/.codex"))
        .env(
            "CODEX_ROUTER_DEBUG_APP_SERVER_SOCKET",
            fixture.root.path().join("debug-app-server/app-server.sock"),
        )
        .args(arguments);
    command
}

#[tokio::test]
async fn dry_run_keeps_each_background_state_without_starting_a_child() {
    for (state, expected) in [
        ("working", "claude attach deadbeef\n".to_owned()),
        ("blocked", "claude attach deadbeef\n".to_owned()),
        ("done", format!("claude --bg --resume {SESSION}\n")),
        ("failed", format!("claude --bg --resume {SESSION}\n")),
        ("stopped", format!("claude --bg --resume {SESSION}\n")),
    ] {
        let fixture = ClaudeProcessFixture::new().await.unwrap();
        let api = serve_job(&fixture, state).unwrap();
        let output = selected_command(
            &fixture,
            &["--provider", "claude", "--id", SESSION, "--dry-run"],
        )
        .output()
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{state}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected,
            "{state}"
        );
        assert!(invocations(&fixture).unwrap().is_empty());
        api.stop().await.unwrap();
    }
}

#[tokio::test]
async fn ordinary_stored_claude_resume_is_foreground_in_recorded_project() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    write_jobs(&fixture, json!([])).unwrap();
    seed_transcript(&fixture).unwrap();
    let mut command = selected_command(&fixture, &[SESSION]);
    command.current_dir(fixture.root.path());
    command.env(
        "CLAUDE_FIXTURE_FLOW",
        fixture.root.path().join("flow.jsonl"),
    );
    let output = command.output().await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = invocations(&fixture).unwrap();
    assert_eq!(
        calls.len(),
        1,
        "one foreground resume, no background start or attach"
    );
    assert_eq!(calls[0].arguments, ["--resume", SESSION]);
    assert_eq!(calls[0].working_directory, fixture.working_directory);
    api.stop().await.unwrap();
}

#[tokio::test]
async fn automatic_uuid_owner_detection_is_exact_and_requires_one_owner() {
    for (codex, claude) in [(true, false), (false, true), (false, false), (true, true)] {
        let fixture = ClaudeProcessFixture::new().await.unwrap();
        let api = serve_job(&fixture, "stopped").unwrap();
        write_jobs(&fixture, json!([])).unwrap();
        seed_codex(&fixture, codex).await.unwrap();
        if claude {
            seed_transcript(&fixture).unwrap();
        }
        let output = selected_command(&fixture, &["--id", SESSION, "--dry-run"])
            .output()
            .await
            .unwrap();
        let diagnostic = String::from_utf8_lossy(&output.stderr);
        if codex != claude {
            assert!(output.status.success(), "{codex}/{claude}: {diagnostic}");
            let chosen = String::from_utf8(output.stdout).unwrap();
            if claude {
                assert_eq!(chosen, format!("claude --resume {SESSION}\n"));
            } else {
                assert!(
                    chosen.starts_with("codex ")
                        && chosen.contains(&format!("resume -- {SESSION}")),
                    "{chosen}"
                );
            }
        } else {
            assert!(
                !output.status.success(),
                "owner ambiguity/absence must not choose Codex"
            );
            assert!(
                diagnostic.contains(if codex {
                    "--provider"
                } else {
                    "no session with that ID"
                }),
                "{diagnostic}"
            );
        }
        assert!(invocations(&fixture).unwrap().is_empty());
        api.stop().await.unwrap();
    }
}

#[tokio::test]
async fn explicit_background_handle_attaches_to_that_job_without_resuming_a_uuid() {
    for state in ["working", "blocked", "done", "failed", "stopped"] {
        let fixture = ClaudeProcessFixture::new().await.unwrap();
        let api = serve_job(&fixture, state).unwrap();
        let output = selected_command(&fixture, &["--id", "deadbeef", "--dry-run"])
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{state}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(output.stdout, b"claude attach deadbeef\n");
        assert!(invocations(&fixture).unwrap().is_empty());
        api.stop().await.unwrap();
    }
}

#[tokio::test]
async fn unknown_state_and_incomplete_discovery_remain_refusals_in_dry_run() {
    for incomplete in [false, true] {
        let fixture = ClaudeProcessFixture::new().await.unwrap();
        let api = serve_job(
            &fixture,
            if incomplete {
                "stopped"
            } else {
                "future-state"
            },
        )
        .unwrap();
        if incomplete {
            fs::rename(
                fixture.root.path().join("peers"),
                fixture.root.path().join("peers-held"),
            )
            .unwrap();
            fs::write(fixture.root.path().join("peers"), "unreadable directory").unwrap();
        }
        let output = selected_command(
            &fixture,
            &["--provider", "claude", "--id", SESSION, "--dry-run"],
        )
        .output()
        .await
        .unwrap();
        assert!(
            !output.status.success(),
            "dry-run must use the same safety decision"
        );
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(
            error.contains(if incomplete { "incomplete" } else { "unknown" }),
            "{error}"
        );
        assert!(invocations(&fixture).unwrap().is_empty());
        api.stop().await.unwrap();
    }
}

#[tokio::test]
async fn unique_uuid_prefix_resolves_across_providers_and_ambiguous_prefix_lists_candidates() {
    for (codex, claude) in [(true, false), (false, true), (true, true)] {
        let fixture = ClaudeProcessFixture::new().await.unwrap();
        let api = serve_job(&fixture, "stopped").unwrap();
        write_jobs(&fixture, json!([])).unwrap();
        seed_codex(&fixture, codex).await.unwrap();
        if claude {
            seed_transcript(&fixture).unwrap();
        }
        let output = selected_command(&fixture, &["--id", "00000000", "--dry-run"])
            .output()
            .await
            .unwrap();
        if codex && claude {
            assert!(!output.status.success());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(
                error.contains("codex")
                    && error.contains("claude")
                    && error.contains(SESSION)
                    && error.contains("--provider"),
                "{error}"
            );
            assert!(
                error.contains("fixture"),
                "candidate title/project must be included: {error}"
            );
        } else {
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let command = String::from_utf8(output.stdout).unwrap();
            assert!(command.contains(SESSION), "{command}");
            assert_eq!(command.starts_with("claude "), claude);
        }
        assert!(invocations(&fixture).unwrap().is_empty());
        api.stop().await.unwrap();
    }
}

#[tokio::test]
async fn background_handle_precedes_a_matching_session_prefix() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    seed_codex(&fixture, true).await.unwrap();
    write_jobs(
        &fixture,
        json!([{"kind":"background","id":"00000000","sessionId":SESSION,"cwd":fixture.working_directory,"state":"stopped","startedAt":1,"name":"Job first"}]),
    ).unwrap();
    let output = selected_command(&fixture, &["--id", "00000000", "--dry-run"])
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, b"claude attach 00000000\n");
    assert!(invocations(&fixture).unwrap().is_empty());
    api.stop().await.unwrap();
}

#[tokio::test]
async fn ordinary_claude_without_recorded_cwd_refuses_instead_of_guessing_project_path() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    write_jobs(&fixture, json!([])).unwrap();
    seed_transcript(&fixture).unwrap();
    fs::write(
        fixture
            .root
            .path()
            .join(format!("home/.claude/projects/fixture/{SESSION}.jsonl")),
        "{}\n",
    )
    .unwrap();
    let output = selected_command(&fixture, &["--provider", "claude", "--id", SESSION])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("directory"), "{error}");
    assert!(invocations(&fixture).unwrap().is_empty());
    api.stop().await.unwrap();
}

#[tokio::test]
async fn ambiguous_prefix_lists_every_codex_match_with_its_title() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    write_jobs(&fixture, json!([])).unwrap();
    seed_codex(&fixture, false).await.unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(fixture.root.path().join("home/.codex/state_5.sqlite")),
    )
    .await
    .unwrap();
    let mut expected = Vec::new();
    for index in 0..10 {
        let id = format!("aabbccdd-0000-4000-8000-{index:012}");
        let title = format!("Retained session {index}");
        sqlx::query("INSERT INTO threads(id,title,source,created_at_ms,updated_at_ms,has_user_event) VALUES(?,?,'cli',1,1,1)")
            .bind(&id).bind(&title).execute(&pool).await.unwrap();
        expected.push((id, title));
    }
    pool.close().await;
    let output = selected_command(&fixture, &["--id", "aabbccdd", "--dry-run"])
        .output()
        .await
        .unwrap();
    assert!(!output.status.success());
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    for (id, title) in expected {
        assert!(
            diagnostic.contains(&format!("codex  {id}  {title}")),
            "{diagnostic}"
        );
    }
    assert!(output.stdout.is_empty());
    assert!(invocations(&fixture).unwrap().is_empty());
    api.stop().await.unwrap();
}

#[tokio::test]
async fn forced_provider_resolves_uuid_collision_and_handle_executes_only_attach() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    seed_codex(&fixture, true).await.unwrap();
    seed_transcript(&fixture).unwrap();
    for provider in ["codex", "claude"] {
        let output = selected_command(
            &fixture,
            &["--provider", provider, "--id", SESSION, "--dry-run"],
        )
        .output()
        .await
        .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).starts_with("claude "),
            provider == "claude"
        );
    }
    let output = selected_command(&fixture, &["--id", "deadbeef"])
        .output()
        .await
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = invocations(&fixture).unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].arguments, ["attach", "deadbeef"]);
    assert_eq!(calls[0].working_directory, fixture.working_directory);
    api.stop().await.unwrap();
}

#[tokio::test]
async fn full_uuid_owner_lookup_does_not_truncate_substring_candidates() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    write_jobs(&fixture, json!([])).unwrap();
    seed_codex(&fixture, true).await.unwrap();
    seed_transcript(&fixture).unwrap();
    let pool = sqlx::SqlitePool::connect_with(
        sqlx::sqlite::SqliteConnectOptions::new()
            .filename(fixture.root.path().join("home/.codex/state_5.sqlite")),
    )
    .await
    .unwrap();
    for index in 0..10 {
        sqlx::query("INSERT INTO threads(id,source,created_at_ms,updated_at_ms,has_user_event) VALUES(?,'cli',2,2,1)")
            .bind(format!("{SESSION}-lookalike-{index}")).execute(&pool).await.unwrap();
    }
    pool.close().await;
    let output = selected_command(&fixture, &["--id", SESSION, "--dry-run"])
        .output()
        .await
        .unwrap();
    assert!(
        !output.status.success(),
        "both providers own the exact UUID; a truncated lookup must not select Claude"
    );
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(
        error.contains("codex") && error.contains("claude") && error.contains("--provider"),
        "{error}"
    );
    assert!(output.stdout.is_empty());
    assert!(invocations(&fixture).unwrap().is_empty());
    api.stop().await.unwrap();
}

#[tokio::test]
async fn ordinary_stored_claude_resume_forwards_harness_arguments() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "stopped").unwrap();
    write_jobs(&fixture, json!([])).unwrap();
    seed_transcript(&fixture).unwrap();
    let arguments = [
        "--model",
        "claude-haiku-5-5",
        "--effort",
        "low",
        "--name",
        "literal --bg value",
    ];
    let mut command = selected_command(&fixture, &["--provider", "claude", "--id", SESSION, "--"]);
    command.args(arguments).env(
        "CLAUDE_FIXTURE_FLOW",
        fixture.root.path().join("flow.jsonl"),
    );
    let output = command.output().await.unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = invocations(&fixture).unwrap();
    assert_eq!(calls.len(), 1);
    let mut expected = vec!["--resume", SESSION];
    expected.extend(arguments);
    assert_eq!(calls[0].arguments, expected);
    assert_eq!(calls[0].working_directory, fixture.working_directory);
    api.stop().await.unwrap();
}

#[tokio::test]
async fn claude_resume_dry_run_includes_harness_arguments_without_launching() {
    let fixture = ClaudeProcessFixture::new().await.unwrap();
    let api = serve_job(&fixture, "working").unwrap();
    let output = selected_command(
        &fixture,
        &[
            "--provider",
            "claude",
            "--id",
            SESSION,
            "--dry-run",
            "--",
            "--model",
            "haiku",
            "--effort",
            "low",
        ],
    )
    .output()
    .await
    .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        b"claude attach deadbeef --model haiku --effort low\n"
    );
    assert!(invocations(&fixture).unwrap().is_empty());
    api.stop().await.unwrap();
}
