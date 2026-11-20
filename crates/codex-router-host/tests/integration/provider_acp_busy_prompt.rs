//! The Router's ACP socket through the real Host (D1 R12, V22): a
//! `session/prompt` for a session that is in a turn is never refused as busy.
//! With an agent that cannot steer it waits in the session's line, and its
//! response arrives when the turn it started ends. `steering_placements` and
//! `queued_placements` cover the steering and non-steering agents' other
//! placements and not-sent endings through the same Host path.

mod acp_busy_host;
mod queued_placements;
mod steering_placements;

use acp_busy_host::{AcpBusyHost, BusyAgent, agent_frames, until_agent_read};

/// Oracle: D1 R12 and V22. The second prompt must queue while the first
/// turn is held, then each prompt completes on its own turn.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn acp_busy_prompt_through_the_host_command_port() {
    let mut host = AcpBusyHost::start(&BusyAgent::Cursor(&["hold-first-prompt"])).await;
    host.send_prompt(3, "first").await;
    until_agent_read(&host.receipt, "session/prompt", 1).await;
    host.send_prompt(4, "second").await;
    let waiting = host.until_queued_or_answered(1, 4).await;
    assert_eq!(
        waiting.len(),
        1,
        "the second prompt is queued before release"
    );
    assert_eq!(waiting[0]["preview"], "second");
    let prompts_while_held = agent_frames(&host.receipt, "session/prompt", None);

    host.release("release");
    let first = host.response(3).await;
    let second = host.response(4).await;
    until_agent_read(&host.receipt, "session/prompt", 2).await;
    let prompts_after = agent_frames(&host.receipt, "session/prompt", None);
    let cancels = agent_frames(&host.receipt, "session/cancel", None);
    host.shutdown().await.expect("Host shutdown");

    assert_eq!(first["id"], 3, "{first}");
    assert_eq!(first["result"]["stopReason"], "end_turn", "{first}");
    assert_eq!(second["id"], 4, "{second}");
    assert_eq!(second["result"]["stopReason"], "end_turn", "{second}");
    assert_eq!(prompts_while_held, 1);
    assert_eq!(prompts_after, 2);
    assert_eq!(cancels, 0);
}
