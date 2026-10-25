//! J4 canonical history oracle (Specification C4, Verification Design J4).
//!
//! History retains the canonical submitted command and task; it does not promise one
//! user row per input. The expected rows are the original retained user facts from the
//! public provider events: every user item carrying the canonical command and task,
//! each with its own identity. An echoing provider therefore leaves two rows with equal
//! text and distinct IDs (the submitted input and the native echo), and both must
//! survive. `thread/read` and `thread/resume` must show exactly that identity set with
//! its content: no missing, changed or fabricated row, and no identity repeated.
use crate::conversation_oracle::{expected_native_reply, require_complete, retained_items};
use crate::verification_models::{FixtureRecord, HistoryItem, HistoryThread, ProviderObservation};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HistoryCheck {
    /// Original retained user identities carrying the canonical command and task.
    pub(crate) user_item_ids: Vec<String>,
    /// The original retained native effect's identity, shown once in each view.
    pub(crate) reply_item_id: String,
    pub(crate) trimmed_outer_whitespace: bool,
}

fn user_text(item: &HistoryItem) -> String {
    item.content
        .iter()
        .filter(|part| part.kind == "text")
        .filter_map(|part| part.text.as_deref())
        .collect()
}

/// The canonical command with its task, exactly as submitted or with only the outer
/// argument whitespace trimmed (the one difference native replay may make).
struct CanonicalTask {
    exact: String,
    trimmed: String,
}

impl CanonicalTask {
    fn new(fixture: &FixtureRecord, arguments: &str) -> Self {
        Self {
            exact: format!("/{} {arguments}", fixture.skill_name),
            trimmed: format!("/{} {}", fixture.skill_name, arguments.trim()),
        }
    }

    fn matches(&self, text: &str) -> bool {
        text == self.exact || text == self.trimmed
    }
}

/// J4: read and resume show the original retained canonical user facts and the one
/// native effect (C4). The same Session may hold other invocations, so only rows
/// carrying this invocation's canonical text, or an expected identity, are judged.
pub(crate) fn check_selected_history(
    original: &ProviderObservation,
    views: &[HistoryThread],
    session_id: &str,
    fixture: &FixtureRecord,
    arguments: &str,
) -> Result<HistoryCheck, String> {
    if views.len() != 2 || views.iter().any(|view| view.id != session_id) {
        return Err("both read and resume views of the same thread are required".to_owned());
    }
    require_complete(original)?;
    let canonical = CanonicalTask::new(fixture, arguments);
    let reply = expected_native_reply(&fixture.canary, arguments);
    // One identity is one fact: repeated events for an item (started, then updated as
    // native chunks arrive) refine that fact, so its latest retained version is judged.
    // An identity whose final text is not canonical is not expected, so a history row
    // showing it with canonical text is fabricated, never blessed. In history, by
    // contrast, an identity shown twice is a repeated row and fails.
    let retained = retained_items(original);
    let expected: BTreeMap<String, String> = retained
        .iter()
        .filter(|item| item.kind.kind == "userMessage")
        .filter_map(|item| Some((item.item_id.clone(), item.text.clone()?)))
        .filter(|(_, text)| canonical.matches(text))
        .collect();
    if expected.is_empty() {
        return Err("original events retain no canonical submitted command and task".to_owned());
    }
    let original_replies: Vec<&str> = retained
        .iter()
        .filter(|item| {
            item.kind.kind == "agentMessage"
                && item.text.as_deref().map(str::trim) == Some(reply.as_str())
        })
        .map(|item| item.item_id.as_str())
        .collect();
    let [original_reply] = original_replies.as_slice() else {
        return Err("native effect missing or repeated in the original events".to_owned());
    };
    let mut trimmed_outer_whitespace = false;
    for view in views {
        let items: Vec<&HistoryItem> = view
            .turns
            .iter()
            .flat_map(|turn| turn.items.iter())
            .collect();
        let mut shown: BTreeMap<&str, usize> = BTreeMap::new();
        for item in items.iter().filter(|item| item.kind == "userMessage") {
            let text = user_text(item);
            if expected.contains_key(&item.id) {
                if !canonical.matches(&text) {
                    return Err(format!(
                        "history changed the command or task of user item {}",
                        item.id
                    ));
                }
                trimmed_outer_whitespace |= text != canonical.exact;
                *shown.entry(item.id.as_str()).or_default() += 1;
            } else if canonical.matches(&text) {
                return Err(format!(
                    "history shows canonical user item {} absent from the original events",
                    item.id
                ));
            }
        }
        if let Some((id, _)) = shown.iter().find(|(_, count)| **count > 1) {
            return Err(format!("history repeated user item {id}"));
        }
        if let Some(missing) = expected.keys().find(|id| !shown.contains_key(id.as_str())) {
            return Err(format!("history lost user item {missing}"));
        }
        let replies: Vec<&&HistoryItem> = items
            .iter()
            .filter(|item| {
                item.kind == "agentMessage"
                    && item.text.as_deref().map(str::trim) == Some(reply.as_str())
            })
            .collect();
        match replies.as_slice() {
            [shown_reply] if shown_reply.id == *original_reply => {}
            [_] => return Err("history shows the native effect under another identity".to_owned()),
            _ => return Err("native effect missing or repeated in history".to_owned()),
        }
    }
    Ok(HistoryCheck {
        user_item_ids: expected.into_keys().collect(),
        reply_item_id: (*original_reply).to_owned(),
        trimmed_outer_whitespace,
    })
}
