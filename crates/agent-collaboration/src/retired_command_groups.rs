//! Command groups this build no longer carries. Each refuses with "not available in this
//! build" and exit 2, so a retired command never falls through to the session picker.
use std::ffi::OsString;

/// The local wake and schedule command groups. They come back with central wakes and
/// schedules.
pub const RETIRED_COMMAND_GROUPS: [&str; 8] = [
    "automation",
    "delivery",
    "revision",
    "operation",
    "run",
    "schedule",
    "wake",
    "instruction",
];

const NOT_AVAILABLE: &str = "not available in this build";
const USAGE_EXIT: i32 = 2;

/// Refuses `arguments` when its first word names a retired group, `--help` included, and
/// returns the exit code. Returns `None` for every other command.
#[must_use]
pub fn refuse_retired_command_group(arguments: &[OsString]) -> Option<i32> {
    let group = arguments.first()?.to_str()?;
    if !RETIRED_COMMAND_GROUPS.contains(&group) {
        return None;
    }
    if arguments.iter().any(|argument| argument == "--json") {
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
