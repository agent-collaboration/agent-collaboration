//! One InputOwner and public controls; models exist before the first event.
#[cfg(all(feature = "test-support", debug_assertions))]
use super::sample_dialogs;
use super::{
    sample_model::{
        FILTER_OUTER_MINIMUM_WIDTH, FILTER_SETTINGS_GAP, SampleData, SampleModel, ScreenBindings,
        action_id, fixture_rows, initial_choices, initial_list, initialized_bindings,
        settings_switch_width, target_id, visible_row,
    },
    screen_actions::build_screen,
};
use agent_collaboration_tui::{
    SurfaceHeader, UiText, UiTextRole,
    controls::{
        self, FilterField, ListRow, ListSelectionControl, MenuButton, ScrollList, SegmentedSwitch,
        TextFieldState, Toolbar,
    },
    focus_model::TargetId,
    input_owner::InputOwner,
    overlay_host::OverlayHost,
    surface_handle::{SurfaceContext, use_surface_stack},
};
use iocraft::prelude::*;
use std::sync::{Arc, Mutex};
#[derive(Props)]
pub struct ShowcaseProps {
    #[iocraft(required)]
    pub initial_width: u16,
    #[iocraft(required)]
    pub initial_height: u16,
    #[cfg(all(feature = "test-support", debug_assertions))]
    pub receipts: Option<super::sample_receipts::SampleReceipts>,
}
#[component]
pub fn Showcase(props: &ShowcaseProps, mut hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let (width, height) = (props.initial_width, props.initial_height);
    let data = SampleData {
        model: hooks.use_state(SampleModel::default),
        filter: hooks.use_state(TextFieldState::default),
        draft: hooks.use_state(|| TextFieldState::new("/sample/界/project")),
        choices: hooks.use_state(initial_choices),
        rows: hooks.use_state(fixture_rows),
        selected: hooks.use_state(initial_list),
        detail_selected: hooks.use_state(initial_list),
        committed: hooks.use_state(String::new),
        last_overlay: hooks.use_state(|| None),
        main_scroll: hooks.use_state(|| None),
        dialog_scroll: hooks.use_state(|| None),
        detail_control: hooks.use_state(|| None),
    };
    let handle = hooks
        .use_ref(|| Arc::new(Mutex::new(None::<ScreenBindings>)))
        .read()
        .clone();
    let stack = use_surface_stack(&mut hooks, |stack| {
        build_screen(stack, &data, handle.clone())
    });
    let bindings = initialized_bindings(&handle);
    #[cfg(all(feature = "test-support", debug_assertions))]
    {
        let hook = hooks.use_hook(|| SampleDrawReceipt {
            help: bindings.help.clone(),
            receipts: props.receipts.clone(),
            data,
            stack,
        });
        hook.receipts = props.receipts.clone();
        hook.help = bindings.help.clone();
        hook.data = data;
        hook.stack = stack;
    }
    let mut initialized = hooks.use_ref(|| false);
    if !initialized.get() {
        #[cfg(all(feature = "test-support", debug_assertions))]
        if let Some(receipts) = &props.receipts {
            receipts.observe(
                &bindings.help,
                &data,
                stack,
                super::sample_receipts::ObservationCause::Fixture,
            );
        }
        initialized.set(true);
    }
    #[cfg(all(feature = "test-support", debug_assertions, unix))]
    hooks.use_future({
        let receipts = props.receipts.clone();
        let bindings = bindings.clone();
        async move {
            if receipts.is_some() {
                let Ok(mut completion) =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined1())
                else {
                    eprintln!("sample completion signal unavailable");
                    return;
                };
                if let Some(receipts) = &receipts {
                    receipts.mark_completion_ready();
                    receipts.observe(
                        &bindings.help,
                        &data,
                        stack,
                        super::sample_receipts::ObservationCause::Fixture,
                    );
                }
                while completion.recv().await.is_some() {
                    sample_dialogs::complete_fixture(stack, &data, &bindings);
                    if let Some(receipts) = &receipts {
                        receipts.observe(
                            &bindings.help,
                            &data,
                            stack,
                            super::sample_receipts::ObservationCause::Fixture,
                        );
                    }
                }
            }
        }
    });
    let mut system = hooks.use_context_mut::<SystemContext>();
    if data
        .model
        .read()
        .effects
        .iter()
        .any(|record| record.action == "quit")
    {
        system.exit();
    }
    #[cfg(all(feature = "test-support", debug_assertions))]
    let receipts = props.receipts.clone();
    let on_routed = move |_| {
        #[cfg(all(feature = "test-support", debug_assertions))]
        if let Some(receipts) = &receipts {
            receipts.observe(
                &bindings.help,
                &data,
                stack,
                super::sample_receipts::ObservationCause::Input,
            );
        }
    };
    let body = element!(OverlayHost {ScreenBody(data,list:bindings.list)}).into_any();
    #[cfg(all(feature = "test-support", debug_assertions))]
    let owner = element!(InputOwner(stack,width,height,quit_action:action_id("quit"),on_routed,dispatch_receipt:props.receipts.as_ref().map(|receipts|receipts.sink.clone())) {#(Some(body))});
    #[cfg(not(all(feature = "test-support", debug_assertions)))]
    let owner = element!(InputOwner(stack,width,height,quit_action:action_id("quit"),on_routed) {#(Some(body))});
    owner
}
#[derive(Props)]
struct ScreenBodyProps {
    #[iocraft(required)]
    data: SampleData,
    #[iocraft(required)]
    list: ListSelectionControl,
}
#[component]
fn ScreenBody(props: &ScreenBodyProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let context = *hooks.use_context::<SurfaceContext>();
    let data = props.data;
    let list = props.list.clone();
    element!(View(width:100pct,height:100pct,flex_direction:FlexDirection::Column) {
        SurfaceHeader(title:"Component showcase".to_owned()) {UiText(content:format!("{} rows · {}",list.rows().len(),data.model.read().effects.last().map_or("Ready".to_owned(),|effect|format!("{} {}",effect.action,effect.target.as_deref().unwrap_or("none")))),role:UiTextRole::Secondary)}
        Toolbar(available_width:context.stack.viewport().0.saturating_sub(12)) {MenuButton(id:TargetId::for_action(&action_id("menu")),action:action_id("menu"))}
        View(width:100pct,column_gap:FILTER_SETTINGS_GAP,flex_shrink:0.0_f32) {View(flex_grow:1.0_f32,min_width:FILTER_OUTER_MINIMUM_WIDTH) {FilterField(id:target_id("filter"),value:data.filter,placeholder:"Filter rows / 界".to_owned())} SegmentedSwitch(actions:vec![action_id("all"),action_id("local")],available_width:settings_switch_width(context.stack.viewport().0))}
        controls::ComboBox(id:target_id("screen-choice"),value:data.choices)
        ScrollList(id:list.scroll_target(),keyboard_target:Some(target_id("sample-list")),scroll:list.scroll(),grow:true) {
            #(list.rows().into_iter().map(|key|{let row=visible_row(&data,&key);element!(ListRow(key:key.as_str().to_owned(),control:list.clone(),row:key.clone()) {View(column_gap:1,width:100pct) {View(width:13,flex_shrink:0.0_f32) UiText(content:row.glyph.to_owned(),role:UiTextRole::Body) UiText(content:format!("{} · {}",row.title,row.key.as_str()),role:UiTextRole::Body)}}).into_any()}))
            #((list.rows().is_empty()).then(||element!(UiText(content:"No match".to_owned(),role:UiTextRole::Secondary))))
        }
        super::sample_coverage::CoverageColumn(fixture:data.model.read().coverage,viewport:context.stack.viewport())
    })
}

#[cfg(all(feature = "test-support", debug_assertions))]
struct SampleDrawReceipt {
    help: agent_collaboration_tui::controls::HelpControl,
    receipts: Option<super::sample_receipts::SampleReceipts>,
    data: SampleData,
    stack: agent_collaboration_tui::surface_handle::SurfaceStackHandle,
}
#[cfg(all(feature = "test-support", debug_assertions))]
impl iocraft::Hook for SampleDrawReceipt {
    fn post_component_draw(&mut self, _drawer: &mut iocraft::ComponentDrawer) {
        if let Some(receipts) = &self.receipts {
            receipts.observe(
                &self.help,
                &self.data,
                self.stack,
                super::sample_receipts::ObservationCause::Frame,
            );
        }
    }
}
