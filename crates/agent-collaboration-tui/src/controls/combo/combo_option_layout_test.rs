use crate::test_support::{
    event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
    frame_grid::FrameGrid,
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    actions::{
        ActionDef, ActionId, ActionRegistry, Availability, AvailabilityFn, Placement, ScopeId,
    },
    controls::{
        combo::{ComboBox, ComboBoxBinding, ComboBoxControl, ComboBoxState},
        fields::button_target,
        list::{OptionKey, SelectOption},
    },
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{SurfaceId, SurfaceModel},
    },
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
use unicode_width::UnicodeWidthStr;
const REASON: &str = "Selected machine does not provide managed ACP support.";
fn action() -> ActionId {
    ActionId::new("option-layout-open").unwrap()
}
fn field() -> TargetId {
    TargetId::new("option-layout-client").unwrap()
}
fn key(code: KeyCode) -> TerminalEvent {
    TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, code))
}
#[derive(Clone)]
struct OptionFixture {
    stack: SurfaceStackHandle,
    value: State<ComboBoxState>,
    picks: State<usize>,
}
#[derive(Props)]
struct OptionSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    width: u16,
    height: u16,
    long_label: bool,
    prefix_options: usize,
    handle: Arc<Mutex<Option<OptionFixture>>>,
}
#[component]
fn OptionScene(props: &OptionSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let value = hooks.use_state(|| {
        let mut alpha = SelectOption::available(
            OptionKey::new("alpha").unwrap(),
            if props.long_label {
                "Deliberately long client label that exceeds this phone row"
            } else {
                "Claude Code"
            },
        );
        alpha.secondary = Some("/remote/only/project".to_owned());
        let beta = SelectOption {
            key: OptionKey::new("beta").unwrap(),
            label: "Cursor · managed ACP".into(),
            secondary: None,
            availability: Availability::Unavailable {
                reason: REASON.into(),
            },
        };
        let mut options = vec![alpha];
        options.extend((0..props.prefix_options).map(|index| {
            SelectOption::available(
                OptionKey::new(format!("prefix-{index}")).unwrap(),
                format!("Other client {index}"),
            )
        }));
        options.push(beta);
        ComboBoxState::new(options, Some(OptionKey::new("alpha").unwrap())).unwrap()
    });
    let picks = hooks.use_state(|| 0usize);
    let stack = use_surface_stack(&mut hooks, |stack| {
        let control = ComboBoxControl::new(ComboBoxBinding {
            parent: SurfaceContext {
                stack,
                surface: SurfaceId::Screen,
            },
            parent_scroll: None,
            field: field(),
            value,
            on_pick: Handler::from(move |_| {
                let mut picks = picks;
                picks.set(picks.get() + 1);
            }),
        });
        let open = ActionDef {
            risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
            id: action(),
            title: "Client".into(),
            toolbar_label: None,
            keys: vec!["F3".parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Toolbar,
            run: Handler::from(move |()| {
                control.open().unwrap();
            }),
        };
        SurfaceModel {
            focus: FocusModel::new(vec![button_target(field(), &open)]).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("option-layout").unwrap(), vec![open])
                .unwrap(),
            primary_action: None,
        }
    });
    *props.handle.lock().unwrap() = Some(OptionFixture {
        stack,
        value,
        picks,
    });
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:props.width,height:props.height,quit_action:ActionId::new("quit").unwrap(),on_routed:move |_|journal.processed(SurfaceSnapshot(vec![(stack.snapshot().top_id(),picks.get().to_le_bytes().to_vec())])) ) {OverlayHost {ComboBox(id:field(),value)}})
}
fn line(grid: &FrameGrid, needle: &str) -> (usize, String) {
    let top = grid
        .cells
        .iter()
        .position(|row| row.iter().any(|cell| cell.text == "╭"))
        .unwrap();
    let bottom = grid
        .cells
        .iter()
        .rposition(|row| row.iter().any(|cell| cell.text == "╰"))
        .unwrap();
    (top..=bottom)
        .map(|row| (row, grid.row_text(row)))
        .find(|(_, text)| text.contains(needle))
        .unwrap_or_else(|| panic!("missing {needle}\n{}", grid.round_frame_text()))
}
fn column(text: &str, needle: &str) -> usize {
    text.get(..text.find(needle).unwrap()).unwrap().width()
}
async fn assert_offscreen_reason_reveal(width: u16, height: u16, mode: InputMode) {
    // The expanded option must actually begin below the painted viewport.
    let prefix_options = 10usize;
    let handle = Arc::new(Mutex::new(None));
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(OptionScene(journal,width,height,prefix_options,handle:handle.clone()));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        driver
            .drive(&mut frames, mode, vec![(key(KeyCode::Enter), true)], true)
            .await;
        let fixture = handle.lock().unwrap().clone().unwrap();
        let popup = fixture.stack.snapshot().top_id();
        let painted = |target: &TargetId| {
            fixture
                .stack
                .hit_regions()
                .published()
                .unwrap()
                .targets
                .iter()
                .any(|region| {
                    region.surface == popup && region.target == *target && !region.cells.is_empty()
                })
        };
        assert!(painted(&field().child("option:alpha")));
        assert!(
            !painted(&field().child("option:beta")),
            "the reason-bearing option starts beyond the native viewport"
        );
        driver
            .drive(
                &mut frames,
                mode,
                (0..=prefix_options)
                    .map(|_| (key(KeyCode::Down), true))
                    .collect(),
                true,
            )
            .await;
        assert_eq!(fixture.value.read().highlight().unwrap().as_str(), "beta");
        assert!(
            painted(&field().child("option:beta")),
            "reveal paints the previously offscreen reason-bearing option at {width}x{height} {mode:?}\n{}",
            driver.frames.last().unwrap().round_frame_text()
        );
        let panel = driver.frames.last().unwrap().round_frame_text();
        let normalized = panel
            .replace(['│', '█'], " ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            normalized.contains(REASON),
            "reveal includes the complete expanded option at {width}x{height} {mode:?}\n{panel}"
        );
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn dropdown_labels_stay_on_one_row_and_reasons_and_overflow_secondary_use_indented_muted_lines()
 {
    for mode in [InputMode::Burst, InputMode::FrameSeparated] {
        for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
            let handle = Arc::new(Mutex::new(None));
            let (mut driver, config, journal) = EventDriver::new();
            let mut ui = element!(OptionScene(journal,width,height,handle:handle.clone()));
            let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                driver.initial(&mut frames).await;
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Enter), true)], true)
                    .await;
                let fixture = handle.lock().unwrap().clone().unwrap();
                let grid = driver.frames.last().unwrap();
                let (alpha_row, alpha) = line(grid, "Claude Code");
                let (path_row, path) = line(grid, "/remote/only/project");
                let (filter_row, filter) = line(grid, "Filter:");
                let muted = grid
                    .cell(column(&filter, "Filter:"), filter_row)
                    .unwrap()
                    .style
                    .foreground;
                assert_eq!(
                    path_row,
                    alpha_row + usize::from(width == 32),
                    "only overflowing secondary moves below its whole label"
                );
                if width == 32 {
                    assert_eq!(
                        column(&path, "/remote/only/project"),
                        column(&alpha, "Claude Code"),
                        "secondary is indented under the label"
                    );
                }
                for offset in 0.."/remote/only/project".len() {
                    assert_eq!(
                        grid.cell(column(&path, "/remote/only/project") + offset, path_row)
                            .unwrap()
                            .style
                            .foreground,
                        muted
                    );
                }
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Down), true)], true)
                    .await;
                let grid = driver.frames.last().unwrap();
                let (beta_row, beta) = line(grid, "Cursor · managed ACP");
                let (reason_row, reason) = line(grid, "Selected machine");
                assert_eq!(
                    reason_row,
                    beta_row + 1,
                    "reason owns the row below the complete label\n{}",
                    grid.round_frame_text()
                );
                assert_eq!(
                    column(&reason, "Selected machine"),
                    column(&beta, "Cursor · managed ACP")
                );
                assert_eq!(
                    grid.cell(column(&reason, "Selected machine"), reason_row)
                        .unwrap()
                        .style
                        .foreground,
                    muted,
                    "reason is muted"
                );
                let panel = grid.round_frame_text().replace(['│', '█'], " ");
                assert!(
                    panel
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                        .contains(REASON),
                    "complete wrapped reason remains reachable\n{panel}"
                );
                let before = fixture.stack.snapshot().top_id();
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Enter), false)], false)
                    .await;
                assert_eq!(
                    fixture.stack.snapshot().top_id(),
                    before,
                    "unavailable option never picks"
                );
                assert_eq!(fixture.picks.get(), 0);
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Up), true)], true)
                    .await;
                driver
                    .drive(
                        &mut frames,
                        mode,
                        vec![(TerminalEvent::BracketedPaste("only/project".into()), true)],
                        true,
                    )
                    .await;
                driver
                    .drive(&mut frames, mode, vec![(key(KeyCode::Enter), true)], true)
                    .await;
                assert_eq!(fixture.stack.snapshot().top_id(), SurfaceId::Screen);
                assert_eq!(fixture.value.read().value().unwrap().as_str(), "alpha");
                assert_eq!(fixture.picks.get(), 1);
            })
            .await
            .unwrap();
            assert_offscreen_reason_reveal(width, height, mode).await;
        }
    }
}
#[tokio::test]
async fn an_overlong_dropdown_label_uses_one_ellipsized_row_before_its_secondary() {
    let (mut driver, config, journal) = EventDriver::new();
    let mut ui = element!(OptionScene(journal,width:32u16,height:24u16,long_label:true));
    let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![(key(KeyCode::Enter), true)],
                true,
            )
            .await;
        let grid = driver.frames.last().unwrap();
        let (label_row, label) = line(grid, "Deliberately");
        assert!(
            label.contains('…'),
            "overlong label has an explicit ellipsis\n{label}"
        );
        let (path_row, _) = line(grid, "/remote/only/project");
        assert_eq!(path_row, label_row + 1);
    })
    .await
    .unwrap();
}
