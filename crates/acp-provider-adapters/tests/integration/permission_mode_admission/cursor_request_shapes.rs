//! Permission requests in the shape real Cursor sends them (D1 R4, R6, L6).
//!
//! Cursor 2026.10.22 (`formatOperation` in its ACP client) never fills ACP
//! `locations` or `rawInput`: a write names its file only in its title and a
//! `diff` content block, a delete and a read only in the title, and a shell
//! command only in the backticked title. These cases send exactly those
//! shapes, so the Router itself must answer the in-folder ones.

use super::{Fixture, json};
use acp_client_runtime::ApprovalPortOutcome;
use acp_provider_adapters::CursorAcpBehavior;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

/// Cursor's request for a new file: `Write <path>`, kind edit, one diff.
fn write_new(path: &Path) -> Value {
    json!({
        "title": format!("Write {}", path.display()),
        "kind": "edit",
        "status": "pending",
        "content": [{"type":"diff","path":path,"oldText":null,"newText":"D1\n"}],
    })
}

/// Cursor's request for an existing file: ``Edit `<path>` ``, kind edit, one diff.
fn edit_existing(path: &Path) -> Value {
    json!({
        "title": format!("Edit `{}`", path.display()),
        "kind": "edit",
        "status": "pending",
        "content": [{"type":"diff","path":path,"oldText":"D1\n","newText":"D2\n"}],
    })
}

/// Cursor's delete: ``Delete `<path>` ``, kind edit, no content.
fn delete(path: &Path) -> Value {
    json!({"title": format!("Delete `{}`", path.display()), "kind": "edit", "status": "pending"})
}

/// Cursor's read: ``Read `<path>` ``, kind read, no content.
fn read(path: &Path) -> Value {
    json!({"title": format!("Read `{}`", path.display()), "kind": "read", "status": "pending"})
}

/// Cursor's shell request: the command in backticks, kind execute, the
/// reason as text content.
fn shell(command: &str) -> Value {
    json!({
        "title": format!("`{command}`"),
        "kind": "execute",
        "status": "pending",
        "content": [{"type":"content","content":{"type":"text","text":"Not in allowlist: rm"}}],
    })
}

/// Oracle: D1 spec R6 "In Router auto with workspace-write access: creating,
/// writing and deleting in the scratch folder runs without asking" and R4
/// (the working folder too), for Cursor's actual request shapes; anything
/// that names a path outside, or that the Router can't read for certain,
/// still asks (L6).
#[tokio::test]
async fn cursor_shaped_in_folder_actions_run_without_asking() {
    let fixture = Fixture::start(
        "cursor",
        "no-directories",
        Arc::new(CursorAcpBehavior),
        ApprovalPortOutcome::Selected {
            option_id: "reject".to_owned(),
            note: None,
        },
        &[],
    )
    .await;
    let own = fixture.scratch("session-a");
    fixture
        .create(Some(&own), Some("auto"))
        .await
        .expect("Cursor auto start");
    let note = own.join("note.txt");
    let outside = Path::new("/tmp/d1-cursor-outside.txt");
    let cases = [
        (write_new(&note), false),
        (edit_existing(&note), false),
        (edit_existing(&fixture.project.join("src/lib.rs")), false),
        (read(&note), false),
        (delete(&note), false),
        (shell(&format!("rm {}", note.display())), false),
        (write_new(outside), true),
        (delete(outside), true),
        (read(outside), true),
        // What Cursor actually ran in L6: shell syntax always asks.
        (
            shell(&format!(
                "mkdir -p \"{0}\" && printf '%s\\n' 'D1' > \"{0}/note.txt\"",
                own.display()
            )),
            true,
        ),
        // A title naming the path twice is not read for certain.
        (
            json!({"title": format!("Delete `{}` `{}`", note.display(), outside.display()), "kind": "edit"}),
            true,
        ),
        // A diff inside does not cover a title-named path outside.
        (
            json!({
                "title": format!("Delete `{}`", outside.display()),
                "kind": "edit",
                "content": [{"type":"diff","path":note,"oldText":null,"newText":""}],
            }),
            true,
        ),
    ];
    let mut expected_asked = 0;
    let mut expected_answers = Vec::new();
    for (tool_call, asks) in &cases {
        fixture.ask("fixture-session-1", tool_call).await;
        if *asks {
            expected_asked += 1;
        }
        assert_eq!(fixture.approvals_asked(), expected_asked, "{tool_call}");
        let option = if *asks { "reject" } else { "allow" };
        expected_answers.push(json!({
            "sessionId":"fixture-session-1",
            "answer":{"outcome":{"outcome":"selected","optionId":option}},
        }));
    }
    fixture.client.shutdown().await;
    assert_eq!(
        fixture.receipt()["permissionAnswers"],
        json!(expected_answers)
    );
}
