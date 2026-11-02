use crate::test_support::{
    event_driver::{EventJournal, SurfaceSnapshot, record_draw},
    frame_grid::FrameGrid,
};
use crate::{
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    controls::{
        combo::{ComboBox, ComboBoxBinding, ComboBoxControl, ComboBoxState},
        fields::{FilterField, MenuButton, TextField, TextFieldState, button_target},
        list::{OptionKey, SelectOption},
    },
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowControl {
    Text,
    Filter,
    Combo,
}

fn field() -> TargetId {
    TargetId::new("window-field").unwrap()
}
fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
pub(super) fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}

#[derive(Clone)]
pub(super) struct WindowFixture {
    stack: SurfaceStackHandle,
    text: State<TextFieldState>,
    pub(super) combo: State<ComboBoxState>,
    pub(super) control: WindowControl,
}
impl WindowFixture {
    pub(super) fn editor(&self) -> TextFieldState {
        if self.control == WindowControl::Combo {
            self.combo.read().query().clone()
        } else {
            self.text.read().clone()
        }
    }
}

#[derive(Props)]
pub(super) struct WindowSceneProps {
    #[iocraft(required)]
    pub(super) journal: EventJournal,
    #[iocraft(required)]
    pub(super) control: Option<WindowControl>,
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) handle: Arc<Mutex<Option<WindowFixture>>>,
}

#[component]
pub(super) fn WindowScene(
    props: &WindowSceneProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let control = props.control.unwrap();
    let text = hooks.use_state(TextFieldState::default);
    let combo = hooks.use_state(|| {
        ComboBoxState::new(
            vec![SelectOption::available(
                OptionKey::new("alpha").unwrap(),
                "Alpha",
            )],
            Some(OptionKey::new("alpha").unwrap()),
        )
        .unwrap()
    });
    let stack = use_surface_stack(&mut hooks, |stack| {
        let id = if control == WindowControl::Combo {
            "open"
        } else {
            "menu"
        };
        let run = if control == WindowControl::Combo {
            let binding = ComboBoxControl::new(ComboBoxBinding {
                parent: SurfaceContext {
                    stack,
                    surface: SurfaceId::Screen,
                },
                parent_scroll: None,
                field: field(),
                value: combo,
                on_pick: Handler::default(),
            });
            Handler::from(move |()| {
                binding.open().unwrap();
            })
        } else {
            Handler::default()
        };
        let definition = ActionDef {
            risk: crate::actions::ActionRisk::Ordinary,
            toolbar_label: None,
            id: action(id),
            title: if control == WindowControl::Combo {
                "Choose"
            } else {
                "Menu"
            }
            .into(),
            keys: vec![
                if control == WindowControl::Combo {
                    "F3"
                } else {
                    "F10"
                }
                .parse()
                .unwrap(),
            ],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Toolbar,
            run,
        };
        let targets = if control == WindowControl::Combo {
            vec![button_target(field(), &definition)]
        } else {
            vec![
                TargetSpec::new(field()).with_input(if control == WindowControl::Filter {
                    TextFieldState::filter_input_handler(text, Handler::default())
                } else {
                    TextFieldState::input_handler(text, Handler::default())
                }),
                button_target(TargetId::for_action(&action("menu")), &definition),
            ]
        };
        SurfaceModel {
            focus: FocusModel::new(targets).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("editor-window").unwrap(), vec![definition])
                .unwrap(),
            primary_action: None,
        }
    });
    let fixture = WindowFixture {
        stack,
        text,
        combo,
        control,
    };
    *props.handle.lock().unwrap() = Some(fixture.clone());
    let journal = props.journal.clone();
    let filter_minimum = if props.width == 32 { 11 } else { 14 };
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:action("quit"),on_routed:move |_| {
        let value=fixture.editor();
        journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),format!("{}|{}",value.text(),value.cursor()).into_bytes())]));
    }) {
        OverlayHost {
            View(width:100pct,height:100pct,border_style:BorderStyle::Round,padding:1,flex_direction:FlexDirection::Column) {
                #(if control==WindowControl::Combo {element!(ComboBox(id:field(),value:combo)).into_any()}else{
                    element!(View(width:100pct,column_gap:1,flex_shrink:0.0_f32) {
                        View(min_width:0,flex_shrink:0.0_f32) {MenuButton(id:TargetId::for_action(&action("menu")),action:action("menu"))}
                        View(width:28,min_width:filter_minimum,flex_shrink:1.0_f32,max_width:28) {
                            #(if control==WindowControl::Filter {element!(FilterField(id:field(),value:text)).into_any()}else{element!(TextField(id:field(),value:text)).into_any()})
                        }
                        View(min_width:0,flex_grow:1.0_f32,flex_shrink:0.0_f32,justify_content:JustifyContent::End) {
                            Text(content:"0/6",color:Color::Cyan,wrap:TextWrap::NoWrap)
                        }
                    }).into_any()
                })
                Text(content:"Body sentinel",wrap:TextWrap::NoWrap)
            }
        }
    })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct EditorBand {
    pub(super) row: usize,
    pub(super) left: usize,
    pub(super) right: usize,
}
pub(super) fn band(fixture: &WindowFixture, frame: &FrameGrid) -> EditorBand {
    let target = if fixture.control == WindowControl::Combo {
        field().child("filter")
    } else {
        field()
    };
    let map = fixture.stack.hit_regions().published().unwrap();
    let region = map
        .targets
        .iter()
        .find(|region| {
            region.surface == fixture.stack.snapshot().top_id() && region.target == target
        })
        .unwrap();
    let left = usize::from(
        region
            .cells
            .iter()
            .map(|(column, _)| *column)
            .min()
            .unwrap(),
    );
    let right = usize::from(
        region
            .cells
            .iter()
            .map(|(column, _)| *column)
            .max()
            .unwrap(),
    );
    let row = usize::from(region.cells.first().unwrap().1);
    assert!(
        region
            .cells
            .iter()
            .all(|(_, found)| usize::from(*found) == row)
    );
    if fixture.control == WindowControl::Combo {
        let line = frame.row_text(row);
        let label = line.find("Filter:").unwrap();
        let editor_left = line.get(..label).unwrap().width() + 8;
        let border = (editor_left..=right + 1)
            .find(|column| {
                frame
                    .cell(*column, row)
                    .is_some_and(|cell| cell.text == "│")
            })
            .expect("query is bounded by its dropdown border");
        EditorBand {
            row,
            left: editor_left,
            right: border - 1,
        }
    } else {
        EditorBand { row, left, right }
    }
}
pub(super) fn content(frame: &FrameGrid, area: EditorBand) -> Vec<(String, bool)> {
    (area.left..=area.right)
        .map(|column| {
            let cell = frame.cell(column, area.row).unwrap();
            (cell.text.clone(), cell.continuation)
        })
        .collect()
}
pub(super) fn assert_caret(frame: &FrameGrid, area: EditorBand) -> usize {
    let carets = frame
        .cells
        .iter()
        .enumerate()
        .flat_map(|(row, cells)| {
            cells.iter().enumerate().filter_map(move |(column, cell)| {
                (cell.style.underline
                    && cell.style.foreground == Some(Color::AnsiValue(11))
                    && !cell.continuation)
                    .then_some((column, row))
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        carets.len(),
        1,
        "one painted gold caret inside {area:?}\n{}",
        frame.row_text(area.row)
    );
    let (column, row) = carets[0];
    assert!(
        row == area.row && column >= area.left && column <= area.right,
        "caret {carets:?} outside {area:?}"
    );
    let cell = frame.cell(column, row).unwrap();
    assert!(
        column + cell.text.width().max(1) <= area.right + 1,
        "whole caret glyph fits"
    );
    assert!(
        !frame.cell(area.left, area.row).unwrap().continuation,
        "window never begins inside a wide glyph"
    );
    column
}
pub(super) fn assert_outside_unchanged(before: &FrameGrid, after: &FrameGrid, area: EditorBand) {
    assert_eq!((before.width, before.height), (after.width, after.height));
    for row in 0..before.height {
        for column in 0..before.width {
            if row != area.row || column < area.left || column > area.right {
                assert_eq!(
                    before.cell(column, row),
                    after.cell(column, row),
                    "editor cannot overprint ({column},{row}) outside {area:?}\n{}",
                    after.row_text(area.row)
                );
            }
        }
    }
}
