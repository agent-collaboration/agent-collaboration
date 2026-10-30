use crate::dialog_test_frame::StaticDialogFrame as DialogFrame;
use crate::*;
use futures_util::{FutureExt, StreamExt};
use iocraft::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

#[derive(Props)]
struct AsyncDialogProps {
    #[iocraft(required)]
    receiver: tokio::sync::watch::Receiver<bool>,
    #[iocraft(required)]
    starts: Arc<AtomicUsize>,
}

#[component]
fn AsyncDialog(props: &AsyncDialogProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let mut loaded = hooks.use_state(|| false);
    let clicks = hooks.use_state(|| 0u32);
    let mut system = hooks.use_context_mut::<SystemContext>();
    hooks.use_future({
        let mut receiver = props.receiver.clone();
        let starts = props.starts.clone();
        async move {
            starts.fetch_add(1, Ordering::SeqCst);
            while !*receiver.borrow_and_update() {
                if receiver.changed().await.is_err() {
                    return;
                }
            }
            loaded.set(true);
        }
    });
    use crate::{
        actions::{ActionDef, ActionId, ActionRegistry, AvailabilityFn, Placement, ScopeId},
        controls::{ScrollHandle, button_target},
        focus_model::{FocusModel, TargetId},
        input_owner::InputOwner,
        overlay_host::OverlayHost,
        surface_handle::use_surface_stack,
        surface_stack::{OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceModel},
    };
    let stack = use_surface_stack(&mut hooks, |_| SurfaceModel {
        focus: FocusModel::new(Vec::new()).unwrap(),
        actions: ActionRegistry::new(ScopeId::new("screen").unwrap(), Vec::new()).unwrap(),
        primary_action: None,
    });
    let mut opened = hooks.use_ref(|| false);
    if !opened.get() {
        let apply = ActionDef {
            toolbar_label: None,
            id: ActionId::new("apply").unwrap(),
            title: "Apply".into(),
            keys: Vec::new(),
            availability: AvailabilityFn::default(),
            choice: None,
            placement: Placement::Toolbar,
            run: Handler::from(move |()| {
                let mut clicks = clicks;
                clicks.set(clicks.get() + 1);
            }),
        };
        let scroll = ScrollHandle::new(stack);
        stack
            .open_overlay(OverlaySpec {
                kind: OverlayKind::Dialog,
                model: SurfaceModel {
                    focus: FocusModel::new(vec![button_target(
                        TargetId::for_action(&apply.id),
                        &apply,
                    )])
                    .unwrap(),
                    actions: ActionRegistry::new(
                        ScopeId::new("async-dialog").unwrap(),
                        vec![apply],
                    )
                    .unwrap(),
                    primary_action: None,
                },
                view: OverlayView::new(move || {
                    element!(AsyncDialogBody(stack,loaded,clicks,scroll:scroll.clone())).into_any()
                }),
                layout: OverlayLayout::Dialog {
                    presentation: DialogPresentation::cells(50, 14),
                    margins: DialogMargins {
                        top: 1,
                        right: 2,
                        bottom: 1,
                        left: 2,
                    },
                },
            })
            .unwrap();
        opened.set(true);
    }
    // The loader belongs to this mounted root; viewport changes never recreate it.
    let width = stack.viewport().0;
    let layout = OverlayLayout::Dialog {
        presentation: if width != 0 && width < 60 {
            DialogPresentation::default()
        } else {
            DialogPresentation::cells(50, 14)
        },
        margins: DialogMargins {
            top: 1,
            right: 2,
            bottom: 1,
            left: 2,
        },
    };
    if let Some(overlay) = stack.snapshot().overlays().last()
        && overlay.layout != layout
    {
        let id = overlay.id;
        stack.update(|model| model.set_overlay_layout_for_test(id, layout));
    }
    if stack.snapshot().overlays().is_empty() {
        system.exit();
    }
    element!(InputOwner(stack,width:80u16,height:24u16,quit_action:ActionId::new("quit").unwrap()) {OverlayHost})
}

#[derive(Props)]
struct AsyncDialogBodyProps {
    #[iocraft(required)]
    stack: crate::surface_handle::SurfaceStackHandle,
    #[iocraft(required)]
    loaded: State<bool>,
    #[iocraft(required)]
    clicks: State<u32>,
    #[iocraft(required)]
    scroll: crate::controls::ScrollHandle,
}
#[component]
fn AsyncDialogBody(props: &AsyncDialogBodyProps) -> impl Into<AnyElement<'static>> {
    use crate::{
        actions::ActionId,
        focus_model::TargetId,
        hit_regions::{HitRegion, HitRegionKind},
    };
    let (width, height) = props.stack.viewport();
    let status = if props.loaded.get() {
        "Loaded"
    } else {
        "Loading"
    };
    element!(crate::controls::DialogFrame(title:format!("{width}x{height} {status}"),scroll:props.scroll.clone(),scroll_target:TargetId::new("body").unwrap(),context:vec![element!(Text(content:"Account: alpha")).into_any()],actions:vec![element!(HitRegion(target:TargetId::for_action(&ActionId::new("apply").unwrap()),kind:HitRegionKind::Target) {Text(content:format!("Apply {}",props.clicks.get()))}).into_any()]) {Text(content:"Loading runs independently of dialog input")})
}

#[tokio::test]
async fn resize_and_apply_remain_responsive_while_tokio_loader_is_pending() {
    let (load_sender, load_receiver) = tokio::sync::watch::channel(false);
    let starts = Arc::new(AtomicUsize::new(0));
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let events = futures_util::stream::unfold(receiver, |mut receiver| async {
        receiver.recv().await.map(|event| (event, receiver))
    });
    let mut ui = element!(AsyncDialog(receiver: load_receiver, starts: starts.clone()));
    let canvases = ui.mock_terminal_render_loop(MockTerminalConfig::with_events(events));
    tokio::pin!(canvases);
    let mut phase = 0;
    let journey = async {
        while let Some(mut canvas) = canvases.next().await {
            while let Some(Some(current)) = canvases.next().now_or_never() {
                canvas = current;
            }
            let frame = canvas.to_string();
            match phase {
                0 if frame.contains("80x24 Loading") => {
                    sender.send(TerminalEvent::Resize(48, 18)).unwrap();
                    phase = 1;
                }
                1 if frame.contains("48x18 Loading") => {
                    assert!(frame.lines().count() <= 18, "{frame}");
                    assert!(
                        frame.contains("Account: alpha") && frame.contains("Apply 0"),
                        "{frame}"
                    );
                    sender.send(TerminalEvent::Resize(32, 24)).unwrap();
                    phase = 2;
                }
                2 if frame.contains("32x24 Loading") => {
                    assert!(
                        frame.contains("Account: al…") && frame.contains("Apply 0"),
                        "{frame}"
                    );
                    assert!(
                        frame.lines().all(|line| line.chars().count() <= 32),
                        "{frame}"
                    );
                    sender.send(TerminalEvent::Resize(40, 12)).unwrap();
                    phase = 3;
                }
                3 if frame.contains("40x12 Loading") => {
                    assert!(
                        frame.contains("Account: alpha") && frame.contains("Apply 0"),
                        "{frame}"
                    );
                    assert!(frame.lines().count() <= 12, "{frame}");
                    sender
                        .send(TerminalEvent::Key(KeyEvent::new(
                            KeyEventKind::Press,
                            KeyCode::Enter,
                        )))
                        .unwrap();
                    phase = 4;
                }
                4 if frame.contains("Apply 1") => {
                    assert!(
                        frame.contains("Loading"),
                        "loader must still be held: {frame}"
                    );
                    assert_eq!(starts.load(Ordering::SeqCst), 1);
                    load_sender.send(true).unwrap();
                    phase = 5;
                }
                5 if frame.contains("40x12 Loaded") => {
                    assert!(frame.contains("Apply 1"));
                    sender.send(TerminalEvent::Resize(160, 40)).unwrap();
                    phase = 6;
                }
                6 if frame.contains("160x40 Loaded") => {
                    assert_eq!(starts.load(Ordering::SeqCst), 1, "resize recreated loader");
                    sender
                        .send(TerminalEvent::Key(KeyEvent::new(
                            KeyEventKind::Press,
                            KeyCode::Esc,
                        )))
                        .unwrap();
                    phase = 7;
                }
                _ => {}
            }
        }
    };
    tokio::time::timeout(std::time::Duration::from_secs(2), journey)
        .await
        .unwrap();
    assert_eq!(phase, 7);
}

#[test]
#[ignore = "explicit performance measurement, not a timing-based correctness gate"]
fn dialog_render_performance_measurements() {
    for (width, height) in [(48_u16, 18_u16), (76, 30), (120, 36), (160, 40)] {
        for rows in [10, 1000] {
            let content = (0..rows)
                .map(|row| format!("ACCOUNT {row}: available quota\n"))
                .collect::<String>();
            for dialog_open in [false, true] {
                let mut samples = Vec::new();
                for _ in 0..50 {
                    let start = std::time::Instant::now();
                    let dialog=dialog_open.then(||element! {
                        DialogFrame(width,height,title:"Filters".to_owned(),presentation:DialogPresentation::cells(72, 20),
                            actions:vec![element!(Text(content:"Apply Cancel")).into_any()]) {
                            Text(content:content.clone())
                        }
                    }.into_any());
                    let output=element! {
                        DialogHost(width,height,background:vec![element! {
                            View(width:u32::from(width),height:u32::from(height),overflow:Overflow::Hidden) {
                                Text(content:"Background account list")
                            }
                        }.into_any()],dialog)
                    }.to_string();
                    std::hint::black_box(output);
                    samples.push(start.elapsed().as_micros());
                }
                samples.sort_unstable();
                println!(
                    "dialog_render width={width} height={height} rows={rows} open={dialog_open} samples=50 p50_us={} p95_us={} max_us={}",
                    samples.get(24).unwrap(),
                    samples.get(47).unwrap(),
                    samples.last().unwrap()
                );
            }
        }
    }
}
