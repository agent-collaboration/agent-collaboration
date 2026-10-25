//! J4 metadata oracle, the merged ACP metadata repair's check ported to Rust: provider
//! metadata stays retained outside assistant items, and genuine prose appears in both
//! history views and both installed-client captures with no raw metadata keys.
use crate::conversation_oracle::{catalogue_names, require_complete, retained_items};
use crate::installed_client_terminal::terminal_text;
use crate::verification_models::{HistoryThread, ProviderItem, ProviderObservation};
use serde::Serialize;
use serde_json::Value;

const METADATA_KINDS: [&str; 4] = ["usage", "modeChange", "configChange", "sessionInfo"];

/// Where a live-capture offset came from. Only a driver-recorded offset was observed by
/// this verifier; an operator-supplied one is trusted as given, not verified.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum OffsetProvenance {
    /// Given on the command line; the verifier did not observe where the input began.
    OperatorSupplied,
    /// Taken by this driver's own terminal just before it submitted the ordinary turn.
    RecordedByDriver,
}

/// A raw capture byte offset with its provenance, reported as evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LiveOffset {
    pub(crate) bytes: usize,
    pub(crate) provenance: OffsetProvenance,
}

impl LiveOffset {
    /// An offset given on the command line, trusted as given.
    pub(crate) fn operator_supplied(bytes: usize) -> Self {
        Self {
            bytes,
            provenance: OffsetProvenance::OperatorSupplied,
        }
    }

    /// A mark this driver's own terminal returned just before submitting the input.
    pub(crate) fn recorded_by_driver(bytes: usize) -> Self {
        Self {
            bytes,
            provenance: OffsetProvenance::RecordedByDriver,
        }
    }
}

/// The original live client capture of the ordinary turn: its raw bytes and the byte
/// offset before the ordinary input. A resumed client redraws earlier history before
/// any input, so only bytes after this offset can show the new live reply; the whole
/// capture is still scanned for metadata leaks.
pub(crate) struct LiveCapture<'a> {
    pub(crate) raw: &'a [u8],
    pub(crate) offset: LiveOffset,
}

impl LiveCapture<'_> {
    /// Terminal text after the offset, sliced from raw bytes before decoding.
    fn after_input(&self) -> Result<String, String> {
        let after = self.raw.get(self.offset.bytes..).ok_or_else(|| {
            format!(
                "ordinary live offset {} is outside the {}-byte capture",
                self.offset.bytes,
                self.raw.len()
            )
        })?;
        Ok(terminal_text(after))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct MetadataCheck {
    pub(crate) metadata_item_ids: Vec<String>,
    pub(crate) command_catalogues: usize,
    pub(crate) usage_items: usize,
    pub(crate) assistant_item_ids: Vec<String>,
    /// The live-capture offset the reply was judged after, with its provenance.
    pub(crate) live_offset: LiveOffset,
}

/// An actual Codex assistant row `• REPLY`, ending the line or followed by the composer.
fn terminal_shows_reply(capture: &str, reply: &str) -> bool {
    capture.lines().any(|line| {
        let Some(rest) = line.trim_start().strip_prefix('•') else {
            return false;
        };
        let rest = rest.trim_start_matches([' ', '\t']);
        rest.strip_prefix(reply).is_some_and(|after| {
            let after = after.trim_start_matches([' ', '\t']);
            after.is_empty() || after.starts_with('›')
        })
    })
}

/// The merged ACP metadata repair's oracle (R7, R8): provider metadata retained outside
/// assistant items, genuine prose in both history views and both installed-client
/// captures, and no raw metadata keys in either capture.
pub(crate) fn check_metadata_observation(
    observation: &ProviderObservation,
    views: &[HistoryThread],
    live: &LiveCapture<'_>,
    reopened_capture: &str,
    expected_reply: &str,
) -> Result<MetadataCheck, String> {
    let live_after_input = live.after_input()?;
    let live_capture = terminal_text(live.raw);
    require_complete(observation)?;
    let retained = retained_items(observation);
    let metadata: Vec<&ProviderItem> = retained
        .iter()
        .filter(|item| {
            METADATA_KINDS.contains(&item.kind.kind.as_str()) || catalogue_names(item).is_some()
        })
        .collect();
    let command_catalogues = metadata
        .iter()
        .filter(|item| catalogue_names(item).is_some())
        .count();
    if command_catalogues == 0 {
        return Err("required provider metadata was not retained".to_owned());
    }
    let assistant_ids: Vec<String> = retained
        .iter()
        .filter(|item| {
            item.kind.kind == "agentMessage" && item.text.as_deref() == Some(expected_reply)
        })
        .map(|item| item.item_id.clone())
        .collect();
    if assistant_ids.is_empty() {
        return Err("ordinary assistant reply absent from original provider events".to_owned());
    }
    if views.len() != 2
        || views
            .iter()
            .any(|view| view.id != observation.target.session_id)
    {
        return Err("both read and resume views of the same thread are required".to_owned());
    }
    for view in views {
        let rendered: Vec<_> = view
            .turns
            .iter()
            .flat_map(|turn| turn.items.iter())
            .collect();
        if rendered
            .iter()
            .any(|item| metadata.iter().any(|meta| meta.item_id == item.id))
        {
            return Err("provider metadata leaked into a Codex transcript item".to_owned());
        }
        if !rendered.iter().any(|item| {
            item.kind == "agentMessage"
                && assistant_ids.contains(&item.id)
                && item.text.as_deref() == Some(expected_reply)
        }) {
            return Err("ordinary assistant reply missing from Codex history".to_owned());
        }
    }
    // The original live reply must be drawn after the input; redrawn history before it
    // never counts as live. The reopened capture is judged whole.
    if !terminal_shows_reply(&live_after_input, expected_reply) {
        return Err(
            "ordinary assistant reply absent from original live PTY after the input".to_owned(),
        );
    }
    if !terminal_shows_reply(reopened_capture, expected_reply) {
        return Err(
            "ordinary assistant reply absent from reopened installed-client PTY".to_owned(),
        );
    }
    // Leaks are judged over both whole captures, including the pre-input redraw.
    for capture in [live_capture.as_str(), reopened_capture] {
        let compact: String = capture
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect();
        for item in &metadata {
            let Some(text) = item.text.as_deref() else {
                continue;
            };
            match serde_json::from_str::<Value>(text) {
                Ok(Value::Object(payload)) => {
                    // Whitespace removal catches keys split across terminal wraps.
                    if payload
                        .keys()
                        .any(|key| compact.contains(&format!("\"{key}\":")))
                    {
                        return Err(
                            "raw provider metadata appeared in installed-client PTY".to_owned()
                        );
                    }
                }
                _ if terminal_shows_reply(capture, text) => {
                    return Err("provider metadata appeared as terminal prose".to_owned());
                }
                _ => {}
            }
        }
    }
    Ok(MetadataCheck {
        metadata_item_ids: metadata.iter().map(|item| item.item_id.clone()).collect(),
        command_catalogues,
        usage_items: metadata
            .iter()
            .filter(|item| item.kind.kind == "usage")
            .count(),
        assistant_item_ids: assistant_ids,
        live_offset: live.offset,
    })
}
