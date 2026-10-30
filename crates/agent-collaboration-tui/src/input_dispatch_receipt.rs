//! Verification-only, per-owner receipts. No environment lookup or global writer.
use crate::{actions::ActionId, focus_model::TargetId, surface_stack::SurfaceId};
use iocraft::prelude::*;
use serde::Serialize;
use std::{
    fs::{File, OpenOptions},
    io::{self, BufWriter, Write},
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone)]
pub struct InputDispatchReceiptSink(Arc<Mutex<ReceiptWriter>>);
struct ReceiptWriter {
    output: BufWriter<File>,
    sequence: u64,
    failure: Option<ReceiptFailure>,
}
struct ReceiptFailure {
    kind: io::ErrorKind,
    message: String,
}
impl ReceiptFailure {
    fn from_error(error: io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }
    fn to_error(&self) -> io::Error {
        io::Error::new(self.kind, self.message.clone())
    }
}

pub(crate) struct InputRouteFacts {
    pub surface: SurfaceId,
    pub target: Option<TargetId>,
    pub action: Option<ActionId>,
}
#[derive(Serialize)]
struct InputDispatchReceipt {
    sequence: u64,
    event: InputEventReadback,
    surface: SurfaceReadback,
    target: Option<String>,
    action: Option<String>,
}
#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum SurfaceReadback {
    Screen,
    Overlay { overlay_id: String },
}
impl From<SurfaceId> for SurfaceReadback {
    fn from(surface: SurfaceId) -> Self {
        match surface {
            SurfaceId::Screen => Self::Screen,
            SurfaceId::Overlay(id) => Self::Overlay {
                overlay_id: id.get().to_string(),
            },
        }
    }
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum InputEventReadback {
    Key {
        code: KeyCodeReadback,
        modifiers: Vec<ModifierReadback>,
        phase: KeyPhaseReadback,
    },
    Mouse {
        mouse: MouseKindReadback,
        column: u16,
        row: u16,
        modifiers: Vec<ModifierReadback>,
    },
    Resize {
        columns: u16,
        rows: u16,
    },
    Other {
        event: OtherInputKind,
    },
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum KeyCodeReadback {
    Text,
    Shortcut { key: char },
    Control { codepoint: u32 },
    Fixed { name: String },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum ModifierReadback {
    Shift,
    Control,
    Alt,
    Super,
    Hyper,
    Meta,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum KeyPhaseReadback {
    Press,
    Repeat,
    Release,
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum MouseKindReadback {
    Down { button: MouseButtonReadback },
    Up { button: MouseButtonReadback },
    Drag { button: MouseButtonReadback },
    Moved,
    ScrollDown,
    ScrollUp,
    ScrollLeft,
    ScrollRight,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum MouseButtonReadback {
    Left,
    Middle,
    Right,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum OtherInputKind {
    BracketedPaste,
    Unknown,
}

fn modifiers_readback(modifiers: KeyModifiers) -> Vec<ModifierReadback> {
    [
        (KeyModifiers::SHIFT, ModifierReadback::Shift),
        (KeyModifiers::CONTROL, ModifierReadback::Control),
        (KeyModifiers::ALT, ModifierReadback::Alt),
        (KeyModifiers::SUPER, ModifierReadback::Super),
        (KeyModifiers::HYPER, ModifierReadback::Hyper),
        (KeyModifiers::META, ModifierReadback::Meta),
    ]
    .into_iter()
    .filter_map(|(flag, name)| modifiers.contains(flag).then_some(name))
    .collect()
}
fn button_readback(button: MouseButton) -> MouseButtonReadback {
    match button {
        MouseButton::Left => MouseButtonReadback::Left,
        MouseButton::Middle => MouseButtonReadback::Middle,
        MouseButton::Right => MouseButtonReadback::Right,
    }
}
impl From<&TerminalEvent> for InputEventReadback {
    fn from(event: &TerminalEvent) -> Self {
        match event {
            TerminalEvent::Key(key) => Self::Key {
                code: match key.code {
                    KeyCode::Char(character) if character.is_control() => {
                        KeyCodeReadback::Control {
                            codepoint: u32::from(character),
                        }
                    }
                    KeyCode::Char(character)
                        if character.is_ascii()
                            && key.modifiers.contains(KeyModifiers::CONTROL) =>
                    {
                        KeyCodeReadback::Shortcut {
                            key: character.to_ascii_lowercase(),
                        }
                    }
                    KeyCode::Char(_) => KeyCodeReadback::Text,
                    code => KeyCodeReadback::Fixed {
                        name: code.to_string(),
                    },
                },
                modifiers: modifiers_readback(key.modifiers),
                phase: match key.kind {
                    KeyEventKind::Press => KeyPhaseReadback::Press,
                    KeyEventKind::Repeat => KeyPhaseReadback::Repeat,
                    KeyEventKind::Release => KeyPhaseReadback::Release,
                },
            },
            TerminalEvent::FullscreenMouse(mouse) => Self::Mouse {
                mouse: match mouse.kind {
                    MouseEventKind::Down(button) => MouseKindReadback::Down {
                        button: button_readback(button),
                    },
                    MouseEventKind::Up(button) => MouseKindReadback::Up {
                        button: button_readback(button),
                    },
                    MouseEventKind::Drag(button) => MouseKindReadback::Drag {
                        button: button_readback(button),
                    },
                    MouseEventKind::Moved => MouseKindReadback::Moved,
                    MouseEventKind::ScrollDown => MouseKindReadback::ScrollDown,
                    MouseEventKind::ScrollUp => MouseKindReadback::ScrollUp,
                    MouseEventKind::ScrollLeft => MouseKindReadback::ScrollLeft,
                    MouseEventKind::ScrollRight => MouseKindReadback::ScrollRight,
                },
                column: mouse.column,
                row: mouse.row,
                modifiers: modifiers_readback(mouse.modifiers),
            },
            TerminalEvent::Resize(columns, rows) => Self::Resize {
                columns: *columns,
                rows: *rows,
            },
            TerminalEvent::BracketedPaste(_) => Self::Other {
                event: OtherInputKind::BracketedPaste,
            },
            _ => Self::Other {
                event: OtherInputKind::Unknown,
            },
        }
    }
}

impl InputDispatchReceiptSink {
    /// The fixture supplies a new file in its explicitly owned directory.
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref();
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "receipt path must be absolute",
            ));
        }
        let file = OpenOptions::new().write(true).create_new(true).open(path)?;
        Ok(Self(Arc::new(Mutex::new(ReceiptWriter {
            output: BufWriter::new(file),
            sequence: 0,
            failure: None,
        }))))
    }
    pub(crate) fn record(&self, event: InputEventReadback, facts: InputRouteFacts) {
        let Ok(mut writer) = self.0.lock() else {
            return;
        };
        if writer.failure.is_some() {
            return;
        }
        if let Err(error) = writer.record(event, facts) {
            writer.failure = Some(ReceiptFailure::from_error(error));
        }
    }
    /// Surfaces any earlier write failure, even when the terminal handler could
    /// not return it. A second check never clears the failure.
    pub fn flush_and_check(&self) -> io::Result<()> {
        let mut writer = self
            .0
            .lock()
            .map_err(|_| io::Error::other("receipt writer poisoned"))?;
        if let Some(failure) = &writer.failure {
            return Err(failure.to_error());
        }
        if let Err(error) = writer.output.flush() {
            let failure = ReceiptFailure::from_error(error);
            let error = failure.to_error();
            writer.failure = Some(failure);
            return Err(error);
        }
        Ok(())
    }
}
impl ReceiptWriter {
    fn record(&mut self, event: InputEventReadback, facts: InputRouteFacts) -> io::Result<()> {
        let sequence = self
            .sequence
            .checked_add(1)
            .filter(|sequence| *sequence <= 9_007_199_254_740_991)
            .ok_or_else(|| io::Error::other("receipt sequence exceeds JSON-safe integers"))?;
        let receipt = InputDispatchReceipt {
            sequence,
            event,
            surface: facts.surface.into(),
            target: facts.target.map(|id| id.as_str().to_owned()),
            action: facts.action.map(|id| id.as_str().to_owned()),
        };
        serde_json::to_writer(&mut self.output, &receipt)?;
        self.output.write_all(b"\n")?;
        self.output.flush()?;
        self.sequence = sequence;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn facts() -> InputRouteFacts {
        InputRouteFacts {
            surface: SurfaceId::Screen,
            target: None,
            action: None,
        }
    }
    fn event() -> InputEventReadback {
        InputEventReadback::Resize {
            columns: 80,
            rows: 24,
        }
    }

    #[test]
    fn receipt_path_is_explicit_new_and_has_no_creation_fallback() {
        assert!(InputDispatchReceiptSink::create("relative.jsonl").is_err());
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input.jsonl");
        std::fs::write(&path, "retained evidence").unwrap();
        assert!(InputDispatchReceiptSink::create(&path).is_err());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "retained evidence");
        assert!(
            InputDispatchReceiptSink::create(directory.path().join("missing/input.jsonl")).is_err()
        );
        assert!(!directory.path().join("missing").exists());
    }
    #[test]
    fn receipt_sequence_stops_at_the_json_safe_boundary_and_retains_the_failure() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input.jsonl");
        let sink = InputDispatchReceiptSink::create(&path).unwrap();
        sink.0.lock().unwrap().sequence = 9_007_199_254_740_990;
        sink.record(event(), facts());
        sink.flush_and_check().unwrap();
        let line = std::fs::read_to_string(&path).unwrap();
        let receipt: serde_json::Value = serde_json::from_str(line.trim_end()).unwrap();
        assert_eq!(receipt["sequence"], 9_007_199_254_740_991u64);
        sink.record(event(), facts());
        assert!(
            sink.flush_and_check()
                .unwrap_err()
                .to_string()
                .contains("JSON-safe")
        );
        assert!(sink.flush_and_check().is_err());
        sink.record(event(), facts());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            line,
            "no wrapped or duplicate sequence is written"
        );
    }
    #[test]
    fn receipt_write_errors_are_retained_for_the_fixture_after_routing() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("input.jsonl");
        std::fs::write(&path, "").unwrap();
        let sink = InputDispatchReceiptSink(Arc::new(Mutex::new(ReceiptWriter {
            output: BufWriter::new(File::open(&path).unwrap()),
            sequence: 0,
            failure: None,
        })));
        sink.record(event(), facts());
        let first = sink.flush_and_check().unwrap_err().to_string();
        assert_eq!(sink.flush_and_check().unwrap_err().to_string(), first);
        assert!(std::fs::read(path).unwrap().is_empty());
    }
    #[test]
    fn separate_input_owners_start_their_own_sequence_at_one() {
        let directory = tempfile::tempdir().unwrap();
        for name in ["first.jsonl", "second.jsonl"] {
            let path = directory.path().join(name);
            let sink = InputDispatchReceiptSink::create(&path).unwrap();
            sink.record(event(), facts());
            sink.flush_and_check().unwrap();
            let receipt: serde_json::Value =
                serde_json::from_str(std::fs::read_to_string(path).unwrap().trim_end()).unwrap();
            assert_eq!(receipt["sequence"], 1);
        }
    }
}
