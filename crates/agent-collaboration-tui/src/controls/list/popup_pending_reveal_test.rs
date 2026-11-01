use crate::test_support::{
    event_driver::{EventDriver, EventJournal, InputMode, SurfaceSnapshot, record_draw},
    frame_grid::FrameGrid,
    list_scene::key,
    recording_backend::recording_terminal_render_loop,
};
use agent_collaboration_tui::{
    DialogMargins, DialogPresentation,
    actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
    controls::{
        combo::{ComboBox, ComboBoxBinding, ComboBoxControl, ComboBoxState},
        fields::{FormField, FormFields, button_target},
        list::{OptionKey, ScrollHandle, ScrollPosition, SelectOption},
    },
    dialog::{DialogFrame, PinnedContext},
    input::input_owner::InputOwner,
    surfaces::{
        focus_model::{FocusModel, TargetId, TargetSpec},
        overlay_host::OverlayHost,
        surface_handle::{SurfaceContext, SurfaceStackHandle, use_surface_stack},
        surface_stack::{
            OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
        },
    },
};
use futures_util::StreamExt;
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};

const QUERY: &str = "zz-route-absent";
fn field() -> TargetId {
    TargetId::new("fork-destination").unwrap()
}
fn action(name: &str) -> ActionId {
    ActionId::new(name).unwrap()
}
#[derive(Clone)]
struct Fixture {
    stack: SurfaceStackHandle,
    parent: SurfaceId,
    scroll: ScrollHandle,
    value: State<ComboBoxState>,
    before_open: Arc<Mutex<Option<ScrollPosition>>>,
}
#[derive(Props)]
struct ForkBodyProps {
    #[iocraft(required)]
    scroll: ScrollHandle,
    #[iocraft(required)]
    value: State<ComboBoxState>,
}
#[component]
fn ForkBody(props: &ForkBodyProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    element!(DialogFrame(title:"Fork session".to_owned(),context:Some(PinnedContext::new("Feature design session [thread-a]")),padding:Some(0),scroll:props.scroll.clone(),scroll_target:TargetId::new("fork-body").unwrap()) {
        FormFields(minimum_label_width:Some(FormFields::label_width_for(["Source","Directory","Destination"]))) {
            FormField(label:"Source".to_owned()) {Text(content:"This machine")}
            FormField(label:"Directory".to_owned()) {Text(content:"/repo/project-a") Text(content:"Directory origin: captured session")}
            FormField(label:"Destination".to_owned(),has_focus:context.is_focused(&field())) {ComboBox(id:field(),value:props.value)}
        }
    })
}
fn open_parent(
    stack: SurfaceStackHandle,
    value: State<ComboBoxState>,
    output: Arc<Mutex<Option<Fixture>>>,
) {
    let scroll = ScrollHandle::new(stack);
    let before_open = Arc::new(Mutex::new(None));
    let opening_scroll = scroll.clone();
    let opening_observation = before_open.clone();
    let open = ActionDef {
        risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
        id: action("destination"),
        title: "Destination".into(),
        toolbar_label: None,
        keys: Vec::new(),
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Hidden,
        run: Handler::from(move |()| {
            *opening_observation.lock().unwrap() = Some(opening_scroll.position());
            ComboBoxControl::new(ComboBoxBinding {
                parent: SurfaceContext {
                    stack,
                    surface: stack.snapshot().top_id(),
                },
                field: field(),
                parent_scroll: Some(opening_scroll.clone()),
                value,
                on_pick: Handler::default(),
            })
            .open()
            .unwrap();
        }),
    };
    let cancel = ActionDef {
        risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
        id: action("cancel"),
        title: "Cancel".into(),
        toolbar_label: None,
        keys: vec!["Esc".parse().unwrap()],
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Toolbar,
        run: Handler::from(move |()| {
            stack.close_top();
        }),
    };
    let fork = ActionDef {
        risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
        id: action("fork"),
        title: "Fork".into(),
        toolbar_label: None,
        keys: vec!["Ctrl+Enter".parse().unwrap()],
        availability: AvailabilityFn::default(),
        choice: None,
        placement: Placement::Toolbar,
        run: Handler::default(),
    };
    let model = SurfaceModel {
        focus: FocusModel::new(vec![
            button_target(field(), &open),
            button_target(TargetId::for_action(&cancel.id), &cancel),
            button_target(TargetId::for_action(&fork.id), &fork),
            TargetSpec::new(TargetId::new("fork-body").unwrap())
                .pointer_only()
                .with_input(scroll.input_handler()),
        ])
        .unwrap(),
        actions: ActionRegistry::new(ScopeId::new("fork").unwrap(), vec![open, cancel, fork])
            .unwrap(),
        primary_action: Some(action("fork")),
    };
    let body_scroll = scroll.clone();
    let parent = stack
        .open_overlay(OverlaySpec {
            kind: OverlayKind::Dialog,
            model,
            view: OverlayView::new(move || {
                element!(ForkBody(scroll:body_scroll.clone(),value)).into_any()
            }),
            layout: OverlayLayout::Dialog {
                presentation: DialogPresentation::cells(42, 10),
                margins: DialogMargins {
                    left: 3,
                    right: 3,
                    top: 1,
                    bottom: 1,
                },
            },
        })
        .unwrap();
    *output.lock().unwrap() = Some(Fixture {
        stack,
        parent: SurfaceId::Overlay(parent),
        scroll,
        value,
        before_open,
    });
}
#[derive(Props)]
struct PendingForkSceneProps {
    #[iocraft(required)]
    journal: EventJournal,
    handle: Arc<Mutex<Option<Fixture>>>,
}
#[component]
fn PendingForkScene(
    props: &PendingForkSceneProps,
    mut hooks: Hooks,
) -> impl Into<AnyElement<'static>> {
    record_draw(&mut hooks, &props.journal);
    let value = hooks.use_state(|| {
        ComboBoxState::new(
            vec![
                SelectOption::available(OptionKey::new("all").unwrap(), "All"),
                SelectOption::available(OptionKey::new("interactive").unwrap(), "Interactive"),
            ],
            Some(OptionKey::new("all").unwrap()),
        )
        .unwrap()
    });
    let output = props.handle.clone();
    let stack = use_surface_stack(&mut hooks, move |stack| {
        let open = ActionDef {
            risk: agent_collaboration_tui::actions::ActionRisk::Ordinary,
            id: action("open-fork"),
            title: "Fork".into(),
            toolbar_label: None,
            keys: vec!["Ctrl+Shift+Enter".parse().unwrap()],
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Hidden,
            run: Handler::from(move |()| open_parent(stack, value, output.clone())),
        };
        SurfaceModel {
            focus: FocusModel::new(Vec::new()).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("screen").unwrap(), vec![open]).unwrap(),
            primary_action: None,
        }
    });
    let journal = props.journal.clone();
    element!(InputOwner(stack,width:48u16,height:12u16,quit_action:action("quit"),on_routed:move |_|journal.processed(SurfaceSnapshot(Vec::new()))) {
        OverlayHost {Text(content:"Session browser")}
    })
}
struct BurstObservation {
    first: FrameGrid,
    settled: FrameGrid,
    scroll: ScrollPosition,
    before_open: ScrollPosition,
    parent_field_focused: bool,
    query: String,
    anchor_cells: Vec<(u16, u16)>,
    popup_cells: Vec<(u16, u16)>,
}
async fn run_burst() -> BurstObservation {
    let (mut driver, config, journal) = EventDriver::new();
    let handle = Arc::new(Mutex::new(None));
    let raw = Arc::new(Mutex::new(Vec::new()));
    let capture = raw.clone();
    let mut ui = element!(PendingForkScene(journal,handle:handle.clone()));
    let mut frames = Box::pin(
        recording_terminal_render_loop(&mut ui, config)
            .inspect(move |canvas| capture.lock().unwrap().push(canvas.clone())),
    );
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        driver.initial(&mut frames).await;
        let start = raw.lock().unwrap().len();
        let mut fork = KeyEvent::new(KeyEventKind::Press, KeyCode::Enter);
        fork.modifiers = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        let acknowledgements = driver
            .drive(
                &mut frames,
                InputMode::Burst,
                vec![
                    (TerminalEvent::Key(fork), true),
                    (key(KeyCode::Enter), true),
                    (TerminalEvent::BracketedPaste(QUERY.to_owned()), true),
                ],
                true,
            )
            .await;
        assert_eq!(acknowledgements.len(), 3);
        assert!(
            acknowledgements
                .windows(2)
                .all(|pair| pair[0].drawn_frames == pair[1].drawn_frames),
            "all three events precede the new parent measurement"
        );
        let fixture = handle.lock().unwrap().clone().unwrap();
        let before_open = fixture.before_open.lock().unwrap().unwrap();
        let query = fixture.value.read().query().text().to_owned();
        let map = fixture.stack.hit_regions().published().unwrap();
        let popup = fixture.stack.snapshot().top_id();
        let result = BurstObservation {
            first: FrameGrid::from_canvas(raw.lock().unwrap().get(start).unwrap()).unwrap(),
            settled: driver.frames.last().unwrap().clone(),
            scroll: fixture.scroll.position(),
            before_open,
            parent_field_focused: SurfaceContext {
                stack: fixture.stack,
                surface: fixture.parent,
            }
            .is_focused(&field()),
            query,
            anchor_cells: map
                .targets
                .iter()
                .find(|region| region.surface == fixture.parent && region.target == field())
                .map_or_else(Vec::new, |region| region.cells.clone()),
            popup_cells: map
                .surfaces
                .iter()
                .find(|region| region.surface == popup)
                .map_or_else(Vec::new, |region| region.cells.clone()),
        };
        assert_eq!(
            result.before_open,
            ScrollPosition::default(),
            "the opening control has no parent measurements"
        );
        assert_eq!(
            result.scroll.natural_height, 5,
            "same native body extent as S6b's Fork oracle"
        );
        assert_eq!(
            result.scroll.viewport_height, 3,
            "same native body viewport as S6b's Fork oracle"
        );
        assert!(
            !result.parent_field_focused,
            "the dropdown covers its parent field"
        );
        assert_eq!(result.query, QUERY);
        eprintln!(
            "BURST FIRST:\n{}\nBURST SETTLED:\n{}",
            result.first.round_frame_text(),
            result.settled.round_frame_text()
        );
        result
    })
    .await
    .unwrap()
}
fn contains_choice(frame: &FrameGrid) -> bool {
    (0..frame.height).any(|row| frame.row_text(row).contains("Filter: zz-route-absent"))
        && (0..frame.height).any(|row| frame.row_text(row).contains("No match"))
}
#[tokio::test]
async fn an_unmeasured_reveal_survives_until_the_covered_field_is_measured() {
    let observed = run_burst().await;
    assert!(
        observed.scroll.offset > 0,
        "the pre-measurement reveal must not be lost: {}",
        observed.settled.round_frame_text()
    );
    assert!(
        contains_choice(&observed.settled),
        "the covered field must reveal and anchor: {}",
        observed.settled.round_frame_text()
    );
    assert!(
        !observed.anchor_cells.is_empty(),
        "Destination has actual painted cells after the burst"
    );
    assert!(
        !observed.popup_cells.is_empty(),
        "the dropdown has an actual painted surface"
    );
    let field_top = observed
        .anchor_cells
        .iter()
        .map(|cell| cell.1)
        .min()
        .unwrap();
    let field_bottom = observed
        .anchor_cells
        .iter()
        .map(|cell| cell.1)
        .max()
        .unwrap();
    let popup_top = observed
        .popup_cells
        .iter()
        .map(|cell| cell.1)
        .min()
        .unwrap();
    let popup_bottom = observed
        .popup_cells
        .iter()
        .map(|cell| cell.1)
        .max()
        .unwrap();
    assert!(
        popup_bottom + 1 == field_top || popup_top == field_bottom + 1,
        "the visible dropdown is adjacent to its revealed field"
    );
}
