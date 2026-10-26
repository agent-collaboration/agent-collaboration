//! J2-J4 oracles: client selection, rejection, native invocation, command lists,
//! canonical history, display operations and retained metadata.
//!
//! Expected values come from the adopted design (Specification C2-C4, Verification
//! Design J2-J4) and from the controlled fixture: its native skill name, file and
//! body-only canary, and the arguments the driver typed.
use crate::native_file_facts::same_path;
use crate::verification_models::{FixtureRecord, OperationRow, ProviderItem, ProviderObservation};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use session_event_model::StopReason;
use std::{collections::BTreeMap, path::Path};

/// Router-owned operation kinds an explicit unloaded resume may add (C4, R7).
const RESUME_OPERATION_KINDS: [&str; 2] = ["conversationLoad", "conversationResume"];

pub(crate) fn require_complete(observation: &ProviderObservation) -> Result<(), String> {
    if observation.end_reason != "deadlineReached" {
        return Err(
            "observation incomplete: page, reset or disconnect needs reconciliation".to_owned(),
        );
    }
    Ok(())
}

/// Latest version of each item, in first-seen hub order.
pub(crate) fn retained_items(observation: &ProviderObservation) -> Vec<ProviderItem> {
    let mut events: Vec<_> = observation.events.iter().collect();
    events.sort_by_key(|entry| entry.sequence);
    let mut order = Vec::new();
    let mut items = BTreeMap::new();
    for entry in events {
        if let ("itemStarted" | "itemUpdated", Some(item)) =
            (entry.event.kind.as_str(), &entry.event.item)
        {
            if !items.contains_key(&item.item_id) {
                order.push(item.item_id.clone());
            }
            items.insert(item.item_id.clone(), item.clone());
        }
    }
    order
        .into_iter()
        .filter_map(|id| items.remove(&id))
        .collect()
}

fn turn_started_ids(observation: &ProviderObservation) -> Vec<String> {
    observation
        .events
        .iter()
        .filter(|entry| entry.event.kind == "turnStarted")
        .filter_map(|entry| entry.event.turn_id.clone())
        .collect()
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdvertisedCommand {
    name: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CommandCatalogueNotice {
    available_commands: Vec<AdvertisedCommand>,
}

/// Names from one retained `available_commands_update` Notice, or None for other items.
pub(crate) fn catalogue_names(item: &ProviderItem) -> Option<Vec<String>> {
    if item.kind.kind != "notice" {
        return None;
    }
    let notice: CommandCatalogueNotice = serde_json::from_str(item.text.as_deref()?).ok()?;
    Some(
        notice
            .available_commands
            .into_iter()
            .map(|command| command.name)
            .collect(),
    )
}

/// Names from the provider's latest full list (ACP wire1 replacement).
pub(crate) fn advertised_command_names(
    observation: &ProviderObservation,
) -> Result<Vec<String>, String> {
    retained_items(observation)
        .iter()
        .filter_map(catalogue_names)
        .next_back()
        .ok_or_else(|| "provider advertised no command catalogue".to_owned())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CommandCatalogueCheck {
    pub(crate) catalogue_item_ids: Vec<String>,
    pub(crate) current_names: Vec<String>,
    pub(crate) removed_names: Vec<String>,
    /// Identical successive lists leave replacement unobserved, never passed.
    pub(crate) replacement_observed: bool,
}

/// J3: successive full lists replace earlier state; the latest one is current (C3).
pub(crate) fn check_command_catalogues(
    observation: &ProviderObservation,
    fixture: &FixtureRecord,
) -> Result<CommandCatalogueCheck, String> {
    require_complete(observation)?;
    let catalogues: Vec<(String, Vec<String>)> = retained_items(observation)
        .iter()
        .filter_map(|item| Some((item.item_id.clone(), catalogue_names(item)?)))
        .collect();
    let Some(((_, current), earlier)) = catalogues.split_last() else {
        return Err("no command catalogue retained".to_owned());
    };
    if !current.contains(&fixture.skill_name) {
        return Err("current command list lacks the controlled native command".to_owned());
    }
    let mut removed: Vec<String> = earlier
        .iter()
        .flat_map(|(_, names)| names.iter())
        .filter(|name| !current.contains(name))
        .cloned()
        .collect();
    removed.sort();
    removed.dedup();
    Ok(CommandCatalogueCheck {
        catalogue_item_ids: catalogues.iter().map(|(id, _)| id.clone()).collect(),
        current_names: current.clone(),
        replacement_observed: !removed.is_empty(),
        removed_names: removed,
    })
}

/// A positive body-execution claim needs a fresh Session (VD J2/J3): its history before
/// the drive holds no fixture canary, so the effect can't come from earlier context
/// in which the skill body or its reply already appeared.
pub(crate) fn require_fresh_session(
    before: &ProviderObservation,
    fixture: &FixtureRecord,
) -> Result<(), String> {
    require_complete(before)?;
    let contaminated = retained_items(before).iter().any(|item| {
        let text = item.text.as_deref().unwrap_or_default();
        text.contains(&fixture.canary) || text.contains(&fixture.second_canary)
    });
    if contaminated {
        return Err(
            "Session history already holds a fixture canary: not a fresh Session".to_owned(),
        );
    }
    Ok(())
}

pub(crate) fn expected_native_reply(canary: &str, arguments: &str) -> String {
    format!("{canary}::{}", arguments.trim())
}

pub(crate) fn new_operations<'a>(
    before: &[OperationRow],
    after: &'a [OperationRow],
) -> Vec<&'a OperationRow> {
    after
        .iter()
        .filter(|row| {
            !before
                .iter()
                .any(|known| known.operation_id == row.operation_id)
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SelectedTurnCheck {
    pub(crate) turn_ids: Vec<String>,
    pub(crate) prompt_operation_ids: Vec<String>,
    pub(crate) reply_item_id: String,
}

/// J2 positive and J3 Router invocation: one prompt reaches the native body effect.
pub(crate) fn check_native_invocation(
    after: &ProviderObservation,
    fixture: &FixtureRecord,
    arguments: &str,
    supplied_texts: &[String],
    operations_before: &[OperationRow],
    operations_after: &[OperationRow],
) -> Result<SelectedTurnCheck, String> {
    require_complete(after)?;
    if supplied_texts
        .iter()
        .any(|text| text.contains(&fixture.canary))
    {
        return Err("canary was supplied as input; the effect proves nothing".to_owned());
    }
    let prompts: Vec<_> = new_operations(operations_before, operations_after)
        .into_iter()
        .filter(|row| row.operation_kind == "conversationPrompt")
        .collect();
    if prompts.len() != 1 {
        return Err("expected exactly one Router prompt operation for the input".to_owned());
    }
    let turns = turn_started_ids(after);
    if turns.is_empty() {
        return Err("no provider turn started for the input".to_owned());
    }
    let expected = expected_native_reply(&fixture.canary, arguments);
    let replies: Vec<_> = retained_items(after)
        .into_iter()
        .filter(|item| {
            item.kind.kind == "agentMessage"
                && item.text.as_deref().map(str::trim) == Some(expected.as_str())
        })
        .collect();
    let [reply] = replies.as_slice() else {
        return Err("argument-dependent native effect absent or repeated".to_owned());
    };
    Ok(SelectedTurnCheck {
        turn_ids: turns,
        prompt_operation_ids: prompts.iter().map(|row| row.operation_id.clone()).collect(),
        reply_item_id: reply.item_id.clone(),
    })
}

/// J2 negatives (R6, C2): a known-invalid selection fails explicitly and sends nothing.
/// A silent drop is not an explicit rejection: the client must show an error (its
/// wording is not asserted). `ordinary_prompts` counts unselected prompts the case
/// itself submitted, such as the running turn a selected steer targets.
/// The declared Session's queued input IDs before and after one rejected selection,
/// read through the public `_session/queue/list`.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QueueObservation {
    pub(crate) before: Vec<String>,
    pub(crate) after: Vec<String>,
}

pub(crate) fn check_rejected_input(
    after: &ProviderObservation,
    fixture: &FixtureRecord,
    operations_before: &[OperationRow],
    operations_after: &[OperationRow],
    queue: &QueueObservation,
    client_error_seen: bool,
    ordinary_prompts: usize,
) -> Result<Vec<String>, String> {
    if !client_error_seen {
        return Err("selection was dropped silently; no explicit rejection shown".to_owned());
    }
    require_complete(after)?;
    // A queued selection adds no prompt or turn while it waits, so the delivery queue
    // itself must be unchanged: same queued IDs in the same order.
    if queue.before != queue.after {
        return Err("rejected selection changed the Router delivery queue".to_owned());
    }
    let created = new_operations(operations_before, operations_after);
    let prompts = created
        .iter()
        .filter(|row| row.operation_kind == "conversationPrompt")
        .count();
    if prompts != ordinary_prompts {
        return Err("rejected selection reached a Router prompt operation".to_owned());
    }
    if turn_started_ids(after).len() != ordinary_prompts {
        return Err("rejected selection started a provider turn".to_owned());
    }
    if retained_items(after).iter().any(|item| {
        let text = item.text.as_deref().unwrap_or_default();
        text.contains(&fixture.canary) || text.contains(&fixture.second_canary)
    }) {
        return Err("rejected selection produced a native skill effect".to_owned());
    }
    Ok(created.iter().map(|row| row.operation_id.clone()).collect())
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OrdinaryCompletion {
    pub(crate) turn_id: String,
    pub(crate) reply_item_id: String,
}

/// Whether any turn has ended in the observation; the bounded wait stops there.
pub(crate) fn any_turn_ended(observation: &ProviderObservation) -> bool {
    observation
        .events
        .iter()
        .any(|entry| entry.event.kind == "turnEnded")
}

/// J2 selected steer: the ordinary turn the steer targeted still completed natively,
/// judged from public provider events rather than the client's screen (Codex 0.163
/// stops drawing that turn after a failed steer). Exactly one turn started, exactly one
/// assistant message is the expected reply, and that turn ended normally.
pub(crate) fn check_ordinary_completion(
    after: &ProviderObservation,
    reply: &str,
) -> Result<OrdinaryCompletion, String> {
    require_complete(after)?;
    let [turn_id] = turn_started_ids(after)
        .try_into()
        .map_err(|_: Vec<String>| "expected exactly one ordinary turn".to_owned())?;
    let replies: Vec<ProviderItem> = retained_items(after)
        .into_iter()
        .filter(|item| {
            item.kind.kind == "agentMessage" && item.text.as_deref().map(str::trim) == Some(reply)
        })
        .collect();
    let [reply_item] = replies.as_slice() else {
        return Err("ordinary reply missing, different or repeated".to_owned());
    };
    let endings: Vec<_> = after
        .events
        .iter()
        .filter(|entry| {
            entry.event.kind == "turnEnded"
                && entry.event.turn_id.as_deref() == Some(turn_id.as_str())
        })
        .collect();
    let [ending] = endings.as_slice() else {
        return Err("ordinary turn has no single end".to_owned());
    };
    let normal = ending.event.outcome.as_ref().is_some_and(|outcome| {
        outcome.kind == "ended" && outcome.stop_reason == Some(StopReason::EndTurn)
    });
    if !normal {
        return Err("ordinary turn did not end normally".to_owned());
    }
    Ok(OrdinaryCompletion {
        turn_id,
        reply_item_id: reply_item.item_id.clone(),
    })
}

/// True when the selected input was admitted after the ordinary turn had ended: the
/// client sent a fresh turn instead of a steer, so the case did not run.
pub(crate) fn selected_steer_raced(before: &[OperationRow], after: &[OperationRow]) -> bool {
    let prompts: Vec<_> = new_operations(before, after)
        .into_iter()
        .filter(|row| row.operation_kind == "conversationPrompt")
        .collect();
    match prompts.as_slice() {
        [first, second, ..] => first
            .terminal_at_ms
            .is_some_and(|ended| second.admitted_at_ms > ended),
        _ => false,
    }
}

/// UserInput item lists from the client's own `from_tui` op records, in order.
/// Codex 0.163 logs the complete AppCommand before dispatch and sends those items
/// unchanged as turn/start or turn/steer input.
pub(crate) fn recorded_user_turns(records: &[Value]) -> Vec<Vec<Value>> {
    fn visit(value: &Value, turns: &mut Vec<Vec<Value>>) {
        match value {
            Value::Object(object) => {
                if let Some(Value::Array(items)) = object.get("items")
                    && items.iter().all(|item| item.get("type").is_some())
                {
                    turns.push(items.clone());
                    return;
                }
                object.values().for_each(|child| visit(child, turns));
            }
            Value::Array(children) => children.iter().for_each(|child| visit(child, turns)),
            _ => {}
        }
    }
    let mut turns = Vec::new();
    for record in records {
        if record.get("dir").and_then(Value::as_str) == Some("from_tui")
            && record.get("kind").and_then(Value::as_str) == Some("op")
            && let Some(payload) = record.get("payload")
        {
            visit(payload, &mut turns);
        }
    }
    turns
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RecordedSelectionCheck {
    pub(crate) item_types: Vec<String>,
    pub(crate) skill_names: Vec<String>,
    pub(crate) text: String,
    pub(crate) text_element_count: usize,
}

/// The real client sent the intended structured selection, so the case was reached:
/// the last of `expected_turns` user turns holds one Text equal to what the driver
/// typed (outer whitespace aside) and one Skill per expected fixture skill, each with
/// its real file origin.
pub(crate) fn check_recorded_selection(
    records: &[Value],
    fixture: &FixtureRecord,
    expected_text: &str,
    expected_skills: &[&str],
    expected_turns: usize,
) -> Result<RecordedSelectionCheck, String> {
    let turns = recorded_user_turns(records);
    if turns.len() != expected_turns {
        return Err(format!(
            "expected {expected_turns} recorded user turns, found {}",
            turns.len()
        ));
    }
    let items = turns.last().cloned().unwrap_or_default();
    let field = |item: &Value, key: &str| item.get(key).and_then(Value::as_str).map(str::to_owned);
    let texts: Vec<&Value> = items
        .iter()
        .filter(|item| field(item, "type").as_deref() == Some("text"))
        .collect();
    let skills: Vec<&Value> = items
        .iter()
        .filter(|item| field(item, "type").as_deref() == Some("skill"))
        .collect();
    let [text_item] = texts.as_slice() else {
        return Err("client input is not one Text plus Skill items".to_owned());
    };
    if skills.len() + 1 != items.len() {
        return Err("client input is not one Text plus Skill items".to_owned());
    }
    let text = field(text_item, "text").unwrap_or_default();
    if text.trim() != expected_text.trim() {
        return Err("client Text differs from the typed input".to_owned());
    }
    let mut names: Vec<String> = skills
        .iter()
        .filter_map(|item| field(item, "name"))
        .collect();
    names.sort();
    let mut expected: Vec<String> = expected_skills
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    expected.sort();
    if names != expected {
        return Err("client Skill items differ from the picker selections".to_owned());
    }
    for item in &skills {
        let origin = if field(item, "name").as_deref() == Some(fixture.second_skill_name.as_str()) {
            &fixture.second_skill_file
        } else {
            &fixture.skill_file
        };
        let folder = origin.parent().unwrap_or(origin);
        let path = field(item, "path").unwrap_or_default();
        if !(same_path(Path::new(&path), origin) || same_path(Path::new(&path), folder)) {
            return Err("client Skill item carries a different origin".to_owned());
        }
    }
    let elements = text_item
        .get("text_elements")
        .or_else(|| text_item.get("textElements"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    Ok(RecordedSelectionCheck {
        item_types: items
            .iter()
            .filter_map(|item| field(item, "type"))
            .collect(),
        skill_names: names,
        text,
        text_element_count: elements,
    })
}

/// J4: pure loaded display adds no Router operation or turn (R7). An explicit unloaded
/// resume may add its existing load, never a prompt. Router records, not native counters.
pub(crate) fn check_display_operations(
    before: &[OperationRow],
    after: &[OperationRow],
    display_events: &ProviderObservation,
    unloaded_resume: bool,
) -> Result<Vec<String>, String> {
    require_complete(display_events)?;
    let created = new_operations(before, after);
    if let Some(row) = created.iter().find(|row| {
        !(unloaded_resume && RESUME_OPERATION_KINDS.contains(&row.operation_kind.as_str()))
    }) {
        return Err(format!(
            "history display started Router operation {}",
            row.operation_kind
        ));
    }
    if !turn_started_ids(display_events).is_empty() {
        return Err("history display started a provider turn".to_owned());
    }
    Ok(created
        .iter()
        .map(|row| row.operation_kind.clone())
        .collect())
}

/// J3 consumer limit: installed Codex refuses an arbitrary provider slash command.
pub(crate) fn check_picker_refusal(screen: &str, command_name: &str) -> Result<(), String> {
    if screen.contains(&format!("Unrecognized command '/{command_name}'")) {
        Ok(())
    } else {
        Err("installed Codex did not show its local slash refusal".to_owned())
    }
}
