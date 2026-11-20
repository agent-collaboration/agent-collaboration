//! Command groups this build no longer carries. Each refuses with "not available in this
//! build" and exit 2, so a retired command never falls through to the session picker.
use std::ffi::OsString;

const RETIRED_COMMAND_GROUPS: &[&str] = &[
    "automation",
    "delivery",
    "revision",
    "operation",
    "run",
    "schedule",
    "wake",
    "instruction",
    "board",
    "request-effects",
    "workspace",
    "project",
    "repository",
    "post",
    "thread",
    "inbox",
    "search",
    "workstream",
    "task",
    "subtask",
    "work-trail",
    "adr",
    "fact",
    "feed",
    "role",
    "view",
    "process",
    "move",
    "hook",
    "message",
    "show",
    "approval",
    "question",
];

const NOT_AVAILABLE: &str = "not available in this build";
const USAGE_EXIT: i32 = 2;

/// Refuses `arguments` when the first word of its first argument names a retired group, and
/// returns the exit code. The first argument may hold the whole command as one quoted string
/// (`"wake send --json"`); its words count as arguments, so `--help` and `--json` inside it are
/// honoured. Returns `None` for every other command.
#[must_use]
pub fn refuse_retired_command_group(arguments: &[OsString]) -> Option<i32> {
    let first = arguments.first()?.to_str()?;
    let group = first.split_ascii_whitespace().next()?;
    if !RETIRED_COMMAND_GROUPS.contains(&group) {
        return None;
    }
    let json = first.split_ascii_whitespace().any(|word| word == "--json")
        || arguments
            .iter()
            .skip(1)
            .any(|argument| argument == "--json");
    if json {
        println!(
            "{}",
            serde_json::json!({"kind":"error","error":{
                "kind":"invalidUsage","stage":"validation","message":NOT_AVAILABLE,
                "nextAction":"correctRequest","details":{"kind":"none"}
            }})
        );
    } else {
        eprintln!("agent-collaboration {group}: {NOT_AVAILABLE}");
    }
    Some(USAGE_EXIT)
}
