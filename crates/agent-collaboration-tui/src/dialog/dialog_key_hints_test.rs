//! Real dialog/footer geometry keeps shortcut tokens whole across the four grids.
use crate::test_support::{
    dialog_scene::DialogScene, event_driver::EventDriver,
    recording_backend::recording_terminal_render_loop,
};
use futures_util::StreamExt;
use iocraft::{ComponentDrawer, Hook, prelude::*};
use std::sync::{Arc, Mutex};

struct HintDrawLimit(crate::test_support::event_driver::EventJournal);
impl Hook for HintDrawLimit {
    fn pre_component_draw(&mut self, _: &mut ComponentDrawer) {
        assert!(
            self.0.draw_count() < 64,
            "non-converging hint layout after a live title update"
        );
    }
}
#[derive(Props)]
struct HintTitleSceneProps {
    #[iocraft(required)]
    journal: crate::test_support::event_driver::EventJournal,
    surface_stack: Arc<Mutex<Option<crate::surfaces::surface_handle::SurfaceStackHandle>>>,
    width: u16,
    height: u16,
}
#[component]
fn HintTitleScene(props: &HintTitleSceneProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    hooks.use_hook(|| HintDrawLimit(props.journal.clone())).0 = props.journal.clone();
    element!(DialogScene(journal:props.journal.clone(),form:true,width:props.width,height:props.height,surface_stack:props.surface_stack.clone()))
}

#[tokio::test]
async fn dialog_hint_groups_wrap_whole_and_stay_inside_the_measured_footer_at_all_grids() {
    for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
        let serialized = Arc::new(Mutex::new(Vec::new()));
        let captured = serialized.clone();
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(DialogScene(journal,form:true,width,height));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config).inspect(
            move |canvas| {
                let mut bytes = Vec::new();
                canvas.write_ansi(&mut bytes).unwrap();
                *captured.lock().unwrap() = bytes;
            },
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            if width == 120 {
                let bytes = serialized.lock().unwrap();
                let baseline = include_bytes!("test_fixtures/dialog_key_hints_wide_120x36.ansi");
                assert_eq!(&*bytes, baseline, "wide hints remain byte-identical");
            }
            let grid = driver.frames.last().unwrap();
            let find = |needle: &str| {
                (0..grid.height)
                    .find(|row| grid.row_text(*row).contains(needle))
                    .unwrap_or_else(|| {
                        panic!("whole hint group {needle:?} missing at {width}x{height}: {grid:?}")
                    })
            };
            let start = find("Ctrl+Enter Start");
            let close = find("Esc Close");
            let bottom = find("╰");
            assert_eq!(
                close,
                start + usize::from(width == 32),
                "hints wrap only between whole groups"
            );
            assert!(close < bottom, "content height includes every hint row");
            if width == 32 {
                assert!(!grid.row_text(start).contains('·'));
                assert!(!grid.row_text(close).contains('·'));
            } else {
                assert!(
                    grid.row_text(start)
                        .contains("Ctrl+Enter Start · Esc Close")
                );
            }
        })
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn an_overwide_hint_truncates_only_its_title_and_keeps_its_key_whole_at_all_grids() {
    use crate::{
        actions::ActionRegistry, surfaces::surface_stack::SurfaceId,
        test_support::dialog_scene::action,
    };
    const TITLE: &str = "Start a session with a deliberately long but readable descriptive title";
    for (width, height) in crate::test_support::grids::STANDARD_GRIDS {
        let observed = Arc::new(Mutex::new(None));
        let (mut driver, config, journal) = EventDriver::new();
        let mut ui = element!(HintTitleScene(journal,width,height,surface_stack:observed.clone()));
        let mut frames = Box::pin(recording_terminal_render_loop(&mut ui, config));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            driver.initial(&mut frames).await;
            let stack = observed.lock().unwrap().unwrap();
            let SurfaceId::Overlay(id) = stack.snapshot().top_id() else {
                panic!("real dialog")
            };
            assert!(stack.update_overlay_if_open(id, |model| {
                let scope = model.actions.scope().clone();
                let mut definitions = model.actions.definitions().to_vec();
                let start = definitions
                    .iter_mut()
                    .find(|definition| definition.id == action("start"))
                    .unwrap();
                start.title = TITLE.into();
                start.toolbar_label = Some("Start".into());
                model.actions = ActionRegistry::new(scope, definitions).unwrap();
            }));
            driver.initial(&mut frames).await;
            let grid = driver.frames.last().unwrap();
            let row = (0..grid.height)
                .find(|row| grid.row_text(*row).contains("Ctrl+Enter Start"))
                .expect("whole key survives title fitting");
            let hint = grid.row_text(row);
            assert!(
                hint.contains("Ctrl+Enter Start"),
                "title remains attributable: {hint}"
            );
            if width < 120 {
                assert!(
                    hint.contains('…'),
                    "an overwide single group ellipsizes its title: {hint}"
                );
            }
            assert!(
                (0..grid.height).any(|row| grid.row_text(row).contains("Esc Close")),
                "the next hint is still a whole group"
            );
        })
        .await
        .unwrap();
    }
}
