//! Capture both real CLI renderers; a typed receipt carries its outcome with exit 0.
use super::*;

const CHILD: &str =
    "board_commands::board_execution::board_write_output_tests::forward_cli_output_child";

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions compare captured bytes and process status.
async fn forwarding_receipts_render_their_kind_and_keep_typed_reply_exit_zero()
-> Result<(), Box<dyn std::error::Error>> {
    for machine in [false, true] {
        for result in [
            json!({"kind":"notHeld"}),
            json!({"kind":"holderUnreachable","decision":{"lastSeen":"2026-10-22T00:00:00Z"}}),
            json!({"kind":"holderUnreachable","decision":{"lastSeen":null}}),
            json!({"kind":"unanswered"}),
            json!({"kind":"overloaded"}),
        ] {
            let expected = json!({"result":result,"reader":null});
            let output = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                tokio::process::Command::new(std::env::current_exe()?)
                    .args(["--exact", CHILD, "--ignored", "--nocapture"])
                    .env("BOARD_FORWARD_RENDER", serde_json::to_string(&expected)?)
                    .env("BOARD_FORWARD_MACHINE", if machine { "1" } else { "0" })
                    .kill_on_drop(true)
                    .output(),
            )
            .await??;
            assert_eq!(output.status.code(), Some(0));
            let stdout = String::from_utf8(output.stdout)?;
            let first = stdout.find('{').ok_or("CLI JSON output")?;
            let last = stdout.rfind('}').ok_or("CLI JSON ending")?;
            let printed: Value =
                serde_json::from_str(stdout.get(first..=last).ok_or("CLI JSON boundaries")?)?;
            let receipt = if machine {
                &printed["result"]["record"]
            } else {
                &printed
            };
            assert_eq!(
                receipt, &expected,
                "CLI changed the typed forwarding outcome"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "reexecuted only by the bounded rendering parent with synthetic receipt input"]
fn forward_cli_output_child() -> Result<(), Box<dyn std::error::Error>> {
    let result: Value = serde_json::from_str(&std::env::var("BOARD_FORWARD_RENDER")?)?;
    let machine = std::env::var("BOARD_FORWARD_MACHINE")? == "1";
    std::process::exit(write_result(result, machine));
}
