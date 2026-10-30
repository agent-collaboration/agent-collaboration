//! Explicit debug receipt route. No environment discovery and no production writer.
use super::sample_model::{CompletionKind, SampleData};
use agent_collaboration_tui::{
    input_dispatch_receipt::InputDispatchReceiptSink, surface_handle::SurfaceStackHandle,
    surface_stack::SurfaceId,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::unix::fs::MetadataExt,
    path::PathBuf,
    sync::{Arc, Mutex},
};
#[derive(Clone)]
pub struct SampleReceipts {
    pub sink: InputDispatchReceiptSink,
    writer: Arc<Mutex<ReceiptWriter>>,
}
struct ReceiptWriter {
    file: File,
    input_ordinal: u64,
    observation: u64,
    failure: Option<String>,
    completion_ready: bool,
    serialized_frames: u64,
    last_draw: Option<(u64, u64)>,
}
pub use super::sample_readback::ObservationCause;
use super::sample_readback::{
    EffectReadback, FocusTargetReadback, HitMapReadback, OverlayReadback, OverlayRegistryReadback,
    RegistriesReadback, RegistryReadback, ScrollReadback, SettingReadback, ShowcaseRecord,
    StateReadback, SurfaceReadback, TargetRoleReadback,
};
fn append_record(file: &mut File, record: &ShowcaseRecord<'_>) -> io::Result<()> {
    let mut bytes = serde_json::to_vec(record).map_err(io::Error::other)?;
    bytes.push(b'\n');
    file.write_all(&bytes)?;
    file.flush()
}
impl SampleReceipts {
    pub fn mark_completion_ready(&self) {
        self.writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .completion_ready = true;
    }
    pub fn from_arguments(args: &[String]) -> io::Result<Option<Self>> {
        if args.is_empty() {
            return Ok(None);
        }
        let [flag, path] = args else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "expected --receipt-directory ABSOLUTE_EMPTY_OWNED_DIRECTORY",
            ));
        };
        if flag != "--receipt-directory" {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unknown sample option",
            ));
        }
        let directory = PathBuf::from(path);
        let metadata = fs::symlink_metadata(&directory)?;
        let output = std::process::Command::new("/usr/bin/id")
            .arg("-u")
            .output()?;
        let uid = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u32>()
            .map_err(io::Error::other)?;
        if !directory.is_absolute()
            || !metadata.is_dir()
            || metadata.file_type().is_symlink()
            || metadata.uid() != uid
            || fs::read_dir(&directory)?.next().is_some()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "receipt directory must be empty, absolute, owned and not a symlink",
            ));
        }
        let sink = InputDispatchReceiptSink::create(directory.join("input-dispatch.jsonl"))?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(directory.join("showcase-state.jsonl"))?;
        Ok(Some(Self {
            sink,
            writer: Arc::new(Mutex::new(ReceiptWriter {
                file,
                input_ordinal: 0,
                observation: 0,
                failure: None,
                completion_ready: false,
                serialized_frames: 0,
                last_draw: None,
            })),
        }))
    }
    pub fn observe(
        &self,
        help: &agent_collaboration_tui::controls::HelpControl,
        data: &SampleData,
        stack: SurfaceStackHandle,
        cause: ObservationCause,
    ) {
        let data = *data;
        let mut writer = self
            .writer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if writer.failure.is_some() {
            return;
        }
        writer.observation += 1;
        if cause == ObservationCause::Input {
            writer.input_ordinal += 1;
        }
        let snapshot = stack.snapshot();
        let model = data.model.read();
        let choices = data.choices.read();
        let value = StateReadback {
            observation: writer.observation,
            input_ordinal: writer.input_ordinal,
            cause,
            completion_ready: writer.completion_ready,
            selection: data
                .selected
                .read()
                .selected()
                .map(|key| key.as_str().to_owned()),
            detail_selection: data
                .detail_selected
                .read()
                .selected()
                .map(|key| key.as_str().to_owned()),
            filter: data.filter.read().text().to_owned(),
            filter_cursor: data.filter.read().cursor(),
            draft: data.draft.read().text().to_owned(),
            cursor: data.draft.read().cursor(),
            committed: data.committed.read().clone(),
            choice: choices.value().map(|key| key.as_str().to_owned()),
            query: choices.query().text().to_owned(),
            highlight: choices.highlight().map(|key| key.as_str().to_owned()),
            effects: model
                .effects
                .iter()
                .map(|record| EffectReadback {
                    action: record.action,
                    target: record.target.clone(),
                })
                .collect(),
            setting: match model.setting {
                super::sample_model::SampleSetting::All => SettingReadback::All,
                super::sample_model::SampleSetting::Local => SettingReadback::Local,
            },
            title: model.live_title.clone(),
            primary_title: model.primary_title.clone(),
            start_enabled: model.start_enabled,
            quit_enabled: model.quit_enabled,
            alternate_primary: model.alternate_primary,
            completion_count: model.completion_count,
            pending_completion: model.queued_completion != CompletionKind::None,
            surface: SurfaceReadback::from(snapshot.top_id()),
            focus_targets: snapshot
                .top()
                .focus
                .targets()
                .iter()
                .map(|target| FocusTargetReadback {
                    target_id: target.id.as_str().to_owned(),
                    role: match target.role {
                        agent_collaboration_tui::focus_model::TargetRole::Focusable => {
                            TargetRoleReadback::Focusable
                        }
                        agent_collaboration_tui::focus_model::TargetRole::PointerOnly => {
                            TargetRoleReadback::PointerOnly
                        }
                    },
                    eligible: target.is_enabled(),
                })
                .collect(),
            focus: snapshot
                .top()
                .focus
                .current()
                .map(|target| target.as_str().to_owned()),
            overlays: snapshot
                .overlays()
                .iter()
                .map(|overlay| OverlayReadback {
                    overlay_id: overlay.id.get().to_string(),
                    kind: (&overlay.kind).into(),
                    primary: overlay
                        .model
                        .primary_action
                        .as_ref()
                        .map(|action| action.as_str().to_owned()),
                })
                .collect(),
            scroll: ScrollReadback {
                main: data
                    .main_scroll
                    .read()
                    .as_ref()
                    .map(|scroll| scroll.position().offset),
                dialog: data
                    .dialog_scroll
                    .read()
                    .as_ref()
                    .map(|scroll| scroll.position().offset),
                detail: data
                    .detail_control
                    .read()
                    .as_ref()
                    .map(|control| control.scroll().position().offset),
            },
            viewport: stack.viewport(),
            hit_map: stack.hit_regions().published().map(HitMapReadback::from),
            registries: RegistriesReadback {
                screen: snapshot
                    .surface(SurfaceId::Screen)
                    .map(RegistryReadback::from_model),
                overlays: snapshot
                    .overlays()
                    .iter()
                    .map(|overlay| OverlayRegistryReadback {
                        surface: SurfaceReadback::from(SurfaceId::Overlay(overlay.id)),
                        current: RegistryReadback::from_model(&overlay.model),
                        base: help
                            .source(SurfaceId::Overlay(overlay.id))
                            .as_ref()
                            .map(RegistryReadback::from_registry),
                    })
                    .collect(),
            },
        };
        if cause == ObservationCause::Frame {
            writer.last_draw = Some((value.observation, value.input_ordinal));
        }
        let written = append_record(
            &mut writer.file,
            &ShowcaseRecord::State { snapshot: &value },
        );
        if let Err(error) = written {
            writer.failure = Some(error.to_string());
        }
    }
    pub fn serialized_frame(&self) -> io::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| io::Error::other("sample receipt writer poisoned"))?;
        let (draw_observation, input_ordinal) = writer
            .last_draw
            .ok_or_else(|| io::Error::other("serialized frame has no completed public draw"))?;
        writer.serialized_frames = writer
            .serialized_frames
            .checked_add(1)
            .ok_or_else(|| io::Error::other("serialization identity exhausted"))?;
        let sequence = writer.serialized_frames;
        append_record(
            &mut writer.file,
            &ShowcaseRecord::SerializedFrame {
                sequence,
                draw_observation,
                input_ordinal,
            },
        )
    }
    pub fn flush_and_check(&self) -> io::Result<()> {
        self.sink.flush_and_check()?;
        let writer = self
            .writer
            .lock()
            .map_err(|_| io::Error::other("sample receipt writer poisoned"))?;
        if let Some(error) = &writer.failure {
            return Err(io::Error::other(error.clone()));
        }
        writer.file.sync_all()
    }
}

/// Observe framing only; bytes are forwarded unchanged to the real stdout/PTY.
pub struct SampleOutput {
    stdout: io::Stdout,
    receipts: Option<SampleReceipts>,
    tail: Vec<u8>,
    in_frame: bool,
}
impl SampleOutput {
    pub fn new(receipts: Option<SampleReceipts>) -> Self {
        Self {
            stdout: io::stdout(),
            receipts,
            tail: Vec::new(),
            in_frame: false,
        }
    }
}
impl Write for SampleOutput {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let marker = b"\x1b[?2026l";
        let mut joined = self.tail.clone();
        joined.extend_from_slice(bytes);
        for window in joined.windows(marker.len()) {
            if window == b"\x1b[?2026h" {
                if self.in_frame {
                    return Err(io::Error::other("nested synchronized sample output"));
                }
                self.in_frame = true;
            } else if window == marker {
                if self.in_frame
                    && let Some(receipts) = &self.receipts
                {
                    receipts.serialized_frame()?;
                }
                self.in_frame = false;
            }
        }
        self.stdout.write_all(bytes)?;
        self.tail = joined.into_iter().rev().take(marker.len() - 1).collect();
        self.tail.reverse();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stdout.flush()
    }
}
