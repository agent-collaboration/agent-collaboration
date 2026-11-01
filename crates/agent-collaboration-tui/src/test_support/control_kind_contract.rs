//! Source-reused R15 runner. Consumers construct their real controls and supply
//! independent outcome/style oracles; this module owns input and observation.
use super::{
    event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot},
    frame_grid::{FrameGrid, FrameStyle},
    grids::STANDARD_GRIDS,
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::surfaces::{
    focus_model::TargetId, surface_handle::SurfaceStackHandle,
};
use iocraft::{ElementExt, TerminalEvent, prelude::*};
use std::sync::{Arc, Mutex};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlKind {
    Button,
    TextField,
    FilterField,
    ComboBox,
    SegmentedSwitch,
    ScrollingList,
    FormField,
    DialogKeyHints,
}
impl ControlKind {
    pub const ALL: [Self; 8] = [
        Self::Button,
        Self::TextField,
        Self::FilterField,
        Self::ComboBox,
        Self::SegmentedSwitch,
        Self::ScrollingList,
        Self::FormField,
        Self::DialogKeyHints,
    ];
}

/// Filled by the consumer's existing stack observer after its real root mounts.
#[derive(Clone, Default)]
pub struct ObservedControlStack(Arc<Mutex<Option<SurfaceStackHandle>>>);
impl ObservedControlStack {
    pub fn record(&self, stack: SurfaceStackHandle) {
        *self.0.lock().unwrap() = Some(stack);
    }

    pub fn observer(&self) -> Handler<SurfaceStackHandle> {
        let observed = self.clone();
        Handler::from(move |stack| observed.record(stack))
    }

    fn target_cells(&self, target: &TargetId) -> Vec<(u16, u16)> {
        let stack = self
            .0
            .lock()
            .unwrap()
            .expect("real consumer stack observed");
        let published = stack
            .hit_regions()
            .published()
            .expect("target style requires a published real frame");
        let surface = stack.snapshot().top_id();
        let cells: Vec<_> = published
            .targets
            .iter()
            .filter(|region| region.surface == surface && region.target == *target)
            .flat_map(|region| region.cells.iter().copied())
            .collect();
        assert!(
            !cells.is_empty(),
            "styled target has no visible published cells: {}",
            target.as_str()
        );
        cells
    }

    fn pointer_event(&self, target: &TargetId, kind: MouseEventKind) -> TerminalEvent {
        let stack = self
            .0
            .lock()
            .unwrap()
            .expect("real consumer stack observed");
        let published = stack
            .hit_regions()
            .published()
            .expect("pointer input requires a published real frame");
        let surface = stack.snapshot().top_id();
        let point = published
            .targets
            .iter()
            .find(|region| region.surface == surface && region.target == *target)
            .and_then(|region| region.cells.first())
            .expect("the real control must publish visible target cells");
        TerminalEvent::FullscreenMouse(FullscreenMouseEvent::new(kind, point.0, point.1))
    }
}

pub enum ControlInput {
    Event(TerminalEvent),
    Pointer {
        target: TargetId,
        kind: MouseEventKind,
    },
}

/// Style is asserted on every displayed cell of every occurrence of this text.
/// Literals/contract roles belong to the caller, never to the rendered subject.
pub enum ControlPaint {
    Contains(String),
    Absent(String),
    Styled {
        text: String,
        style: FrameStyle,
    },
    StyledInTarget {
        target: TargetId,
        text: String,
        style: FrameStyle,
    },
}

pub struct ControlStep {
    pub input: ControlInput,
    /// True only when the contracted outcome changes the actual serialized frame.
    pub redraw: bool,
    pub expected_snapshot: SurfaceSnapshot,
    /// Cumulative handler observations, including this event.
    pub expected_handlers: Vec<String>,
    pub paint: Vec<ControlPaint>,
}

pub struct ControlKindContract {
    pub screen: &'static str,
    pub kind: ControlKind,
    pub viewport: (u16, u16),
    pub initial_paint: Vec<ControlPaint>,
    pub steps: Vec<ControlStep>,
}

/// A kind is declared once for a screen; real instances retain their case data.
pub enum ControlKindDeclaration {
    OnScreen {
        kind: ControlKind,
        contracts: Vec<ControlKindContract>,
    },
    NotOnScreen {
        kind: ControlKind,
        reason: &'static str,
    },
}
impl ControlKindDeclaration {
    fn kind(&self) -> ControlKind {
        match self {
            Self::OnScreen { kind, .. } | Self::NotOnScreen { kind, .. } => *kind,
        }
    }
}

pub struct ScreenControlContracts {
    pub screen: &'static str,
    pub kinds: Vec<ControlKindDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NotOnScreenControl {
    pub screen: &'static str,
    pub kind: ControlKind,
    pub reason: &'static str,
}

#[derive(Debug, Default, Eq, PartialEq)]
pub struct ControlMatrixSummary {
    pub passed_contracts: usize,
    pub not_on_screen: Vec<NotOnScreenControl>,
}

#[derive(Clone, Copy)]
pub struct ControlInstanceRequest {
    pub screen: &'static str,
    pub kind: ControlKind,
    pub viewport: (u16, u16),
}

/// Every screen declares all eight kinds once. Real kinds require all four grids;
/// absent kinds require a reason and never count as a passed runtime contract.
pub async fn assert_control_kind_matrix<TElement: ElementExt + Send>(
    screens: Vec<ScreenControlContracts>,
    build: impl Fn(ControlInstanceRequest, EventJournal, ObservedControlStack) -> TElement,
) -> ControlMatrixSummary {
    assert!(!screens.is_empty(), "empty R15 matrix");
    let mut summary = ControlMatrixSummary::default();
    for (index, screen) in screens.iter().enumerate() {
        assert!(
            screens[..index]
                .iter()
                .all(|previous| previous.screen != screen.screen),
            "R15 matrix declares screen {} more than once",
            screen.screen
        );
        for required in ControlKind::ALL {
            let count = screen
                .kinds
                .iter()
                .filter(|declaration| declaration.kind() == required)
                .count();
            assert!(
                count > 0,
                "R15 matrix missing {required:?} for {}",
                screen.screen
            );
            assert_eq!(
                count, 1,
                "R15 matrix duplicates {required:?} for {}",
                screen.screen
            );
        }
        for declaration in &screen.kinds {
            match declaration {
                ControlKindDeclaration::NotOnScreen { kind, reason } => {
                    assert!(
                        !reason.trim().is_empty(),
                        "R15 NotOnScreen needs a reason for {} {kind:?}",
                        screen.screen
                    );
                    summary.not_on_screen.push(NotOnScreenControl {
                        screen: screen.screen,
                        kind: *kind,
                        reason,
                    });
                }
                ControlKindDeclaration::OnScreen { kind, contracts } => {
                    assert!(
                        !contracts.is_empty(),
                        "R15 real kind {kind:?} has no contracts for {}",
                        screen.screen
                    );
                    for contract in contracts {
                        assert_eq!(
                            contract.screen, screen.screen,
                            "R15 real contract belongs to a different screen"
                        );
                        assert_eq!(
                            contract.kind, *kind,
                            "R15 real contract belongs to a different kind"
                        );
                    }
                    for viewport in STANDARD_GRIDS {
                        assert!(
                            contracts
                                .iter()
                                .any(|contract| contract.viewport == viewport),
                            "R15 matrix missing standard grid {viewport:?} for {} {kind:?}",
                            screen.screen
                        );
                    }
                }
            }
        }
    }
    for absent in &summary.not_on_screen {
        eprintln!(
            "R15 NotOnScreen: {} {:?}: {}",
            absent.screen, absent.kind, absent.reason
        );
    }
    for screen in screens {
        for declaration in screen.kinds {
            match declaration {
                ControlKindDeclaration::OnScreen { contracts, .. } => {
                    for contract in contracts {
                        let request = ControlInstanceRequest {
                            screen: contract.screen,
                            kind: contract.kind,
                            viewport: contract.viewport,
                        };
                        assert_control_kind_contract(contract, |journal, observed| {
                            build(request, journal, observed)
                        })
                        .await;
                        summary.passed_contracts += 1;
                    }
                }
                ControlKindDeclaration::NotOnScreen { .. } => {}
            }
        }
    }
    eprintln!(
        "R15 matrix: {} real contracts passed; {} NotOnScreen declarations",
        summary.passed_contracts,
        summary.not_on_screen.len()
    );
    summary
}

fn assert_paint(
    frame: &FrameGrid,
    observed: &ObservedControlStack,
    assertions: &[ControlPaint],
    context: &str,
) {
    for assertion in assertions {
        match assertion {
            ControlPaint::Contains(text) | ControlPaint::Absent(text) => {
                assert!(!text.is_empty(), "empty paint oracle: {context}");
                let present = (0..frame.height).any(|row| frame.row_text(row).contains(text));
                assert_eq!(
                    present,
                    matches!(assertion, ControlPaint::Contains(_)),
                    "paint {text:?}: {context}\n{frame:?}"
                );
            }
            ControlPaint::Styled { text, style } => {
                assert!(!text.is_empty(), "empty style oracle: {context}");
                let width = text.width();
                let mut occurrences = 0;
                for row in &frame.cells {
                    for column in 0..row.len() {
                        if row[column].continuation {
                            continue;
                        }
                        let suffix: String = row[column..]
                            .iter()
                            .filter(|cell| !cell.continuation)
                            .map(|cell| cell.text.as_str())
                            .collect();
                        if !suffix.starts_with(text) {
                            continue;
                        }
                        occurrences += 1;
                        for cell in row.iter().skip(column).take(width) {
                            assert_eq!(&cell.style, style, "style for {text:?}: {context}");
                        }
                    }
                }
                assert!(occurrences > 0, "missing styled text {text:?}: {context}");
            }
            ControlPaint::StyledInTarget {
                target,
                text,
                style,
            } => {
                assert!(!text.is_empty(), "empty target style oracle: {context}");
                let cells = observed.target_cells(target);
                let width = text.width();
                let mut occurrences = 0;
                for (row_index, row) in frame.cells.iter().enumerate() {
                    for column in 0..row.len() {
                        if row[column].continuation || column + width > row.len() {
                            continue;
                        }
                        if !(column..column + width).all(|cell| {
                            u16::try_from(cell)
                                .ok()
                                .zip(u16::try_from(row_index).ok())
                                .is_some_and(|point| cells.contains(&point))
                        }) {
                            continue;
                        }
                        let suffix: String = row[column..]
                            .iter()
                            .filter(|cell| !cell.continuation)
                            .map(|cell| cell.text.as_str())
                            .collect();
                        if !suffix.starts_with(text) {
                            continue;
                        }
                        occurrences += 1;
                        for cell in row.iter().skip(column).take(width) {
                            assert_eq!(&cell.style, style, "target style for {text:?}: {context}");
                        }
                    }
                }
                assert!(
                    occurrences > 0,
                    "missing styled text {text:?} inside target {}: {context}",
                    target.as_str()
                );
            }
        }
    }
}

/// The factory mounts the consumer's real instance with its existing observers.
/// Every input must be acknowledged by InputOwner and match the literal outcome;
/// pointer events use current measured hit cells, never guessed coordinates.
pub async fn assert_control_kind_contract<TElement: ElementExt + Send>(
    contract: ControlKindContract,
    build: impl FnOnce(EventJournal, ObservedControlStack) -> TElement,
) {
    assert!(
        !contract.initial_paint.is_empty(),
        "a control needs an initial paint oracle"
    );
    assert!(
        !contract.steps.is_empty(),
        "a control needs a real input scenario"
    );
    let context = format!(
        "{} {:?} {:?}",
        contract.screen, contract.kind, contract.viewport
    );
    let (mut driver, config, journal) = EventDriver::new();
    let observed = ObservedControlStack::default();
    let mut ui = build(journal, observed.clone());
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    let scenario = async {
        driver.initial(&mut frames).await;
        let frame = driver.frames.last().unwrap();
        assert_eq!(
            (frame.width, frame.height),
            (
                usize::from(contract.viewport.0),
                usize::from(contract.viewport.1)
            ),
            "real viewport: {context}"
        );
        assert_paint(frame, &observed, &contract.initial_paint, &context);
        for (index, step) in contract.steps.into_iter().enumerate() {
            let event = match step.input {
                ControlInput::Event(event) => event,
                ControlInput::Pointer { target, kind } => observed.pointer_event(&target, kind),
            };
            let acknowledgements = driver
                .drive_checked(
                    &mut frames,
                    InputMode::FrameSeparated,
                    vec![(event, step.redraw)],
                    step.redraw,
                    |acknowledgement| {
                        // Diagnose a wrong result before waiting for its expected paint.
                        assert_eq!(
                            acknowledgement.snapshot, step.expected_snapshot,
                            "outcome: {context} step {index}"
                        );
                        assert_eq!(
                            acknowledgement.handlers, step.expected_handlers,
                            "handlers: {context} step {index}"
                        );
                    },
                )
                .await;
            assert_eq!(
                acknowledgements.len(),
                1,
                "exactly one real input: {context} step {index}"
            );
            assert_paint(
                driver.frames.last().unwrap(),
                &observed,
                &step.paint,
                &context,
            );
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(5), scenario)
        .await
        .unwrap_or_else(|_| panic!("control scenario did not complete: {context}"));
}
