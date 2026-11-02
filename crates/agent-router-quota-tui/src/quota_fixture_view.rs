//! Debug fixture composition keeps the real component and CLI restart owner.
use crate::quota_fixture_controller::{LiveQuotaFixture, QuotaFixtureController};
use crate::test_support::{QuotaFixtureControl, QuotaFixtureOptions};
use crate::{QuotaOutcome, QuotaStatusViewOptions, quota_status_component::QuotaStatusComponent};
use agent_collaboration_tui::{
    input::input_dispatch_receipt::InputDispatchReceiptSink,
    surfaces::surface_handle::SurfaceStackHandle,
};
use codex_router_quota_service::quota_display_model::QuotaStatusViewModel;
use iocraft::{ComponentDrawer, Hook, prelude::*};
use std::{
    future::Future,
    io,
    sync::{Arc, Mutex},
};

#[derive(Clone, Default)]
pub struct QuotaFixtureObservers {
    pub surface_stack: Option<Handler<SurfaceStackHandle>>,
    pub dispatch_receipt: Option<InputDispatchReceiptSink>,
}

#[derive(Clone, Default)]
pub struct QuotaFixtureView {
    pub controller: QuotaFixtureController,
    pub observers: QuotaFixtureObservers,
    pub outcomes: Arc<Mutex<Vec<QuotaOutcome>>>,
}

pub struct ControlledQuotaFixture {
    pub element: AnyElement<'static>,
    pub outcome: Arc<Mutex<Option<QuotaOutcome>>>,
}

pub fn controlled_quota_fixture(
    inputs: QuotaFixtureOptions,
    fixture: QuotaFixtureView,
) -> ControlledQuotaFixture {
    let outcome = Arc::new(Mutex::new(None));
    let options = inputs.ports;
    let (reset_intent_sender, reset_snapshot_receiver) = options
        .reset_session_ports
        .map(|ports| (Some(ports.intent_sender), Some(ports.snapshot_receiver)))
        .unwrap_or((None, None));
    let element = element!(QuotaStatusComponent(
        view_model:inputs.view_model,width:usize::from(inputs.width),height:usize::from(inputs.height),
        fixture_control:Some(inputs.control),fixture_controller:Some(fixture.controller),
        surface_stack_observer:fixture.observers.surface_stack,dispatch_receipt:fixture.observers.dispatch_receipt,
        outcome_sink:Some(outcome.clone()),reload_view_model:options.reload_view_model,
        reset_intent_sender,reset_snapshot_receiver,weekly_floor_saver:options.weekly_floor_saver,
        credit_policy_saver:options.credit_policy_saver,credit_usage_refresher:options.credit_usage_refresher,
        quota_refresher:options.quota_refresher,account_commands:options.account_commands,
        initial_focused_account_id:options.focused_account_id,notice:options.notice,
        initial_group_facet:options.group_preferences.as_ref().map_or(crate::QuotaGroupFacet::None, crate::QuotaGroupPreferences::current_facet),
        group_preferences:options.group_preferences,
        reload_interval:crate::quota_view_configuration::LIVE_QUOTA_STATUS_RELOAD_INTERVAL,
        spinner_interval:crate::quota_view_configuration::LIVE_QUOTA_STATUS_SPINNER_INTERVAL,
    ));
    ControlledQuotaFixture {
        element: element.into_any(),
        outcome,
    }
}

tokio::task_local! {
    static FIXTURE_VIEW: QuotaFixtureView;
}

/// Scope the existing CLI loop, including each real login restart, to one controller.
pub async fn with_quota_fixture_view<T>(
    fixture: QuotaFixtureView,
    future: impl Future<Output = T>,
) -> T {
    FIXTURE_VIEW.scope(fixture, future).await
}

pub(crate) fn current() -> Option<QuotaFixtureView> {
    FIXTURE_VIEW.try_with(Clone::clone).ok()
}

pub(crate) async fn run(
    view_model: QuotaStatusViewModel,
    options: QuotaStatusViewOptions,
    fixture: QuotaFixtureView,
) -> io::Result<QuotaOutcome> {
    let (width, height) = crossterm::terminal::size()?;
    let outcomes = fixture.outcomes.clone();
    let dispatch = fixture.observers.dispatch_receipt.clone();
    let mut mounted = controlled_quota_fixture(
        QuotaFixtureOptions {
            view_model,
            ports: options,
            width,
            height,
            control: QuotaFixtureControl::Browse,
        },
        fixture,
    );
    let render = mounted.element.render_loop().fullscreen().ignore_ctrl_c();
    match dispatch {
        Some(sink) => render.stdout(sink.frame_writer(std::io::stdout())?).await?,
        None => render.await?,
    }
    let result = mounted
        .outcome
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let result =
        result.ok_or_else(|| io::Error::other("quota exited without a terminal outcome"))?;
    outcomes
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(result.clone());
    Ok(result)
}

struct FixturePublicationHook {
    binding: Option<(QuotaFixtureController, u64)>,
}
impl Hook for FixturePublicationHook {
    fn post_component_draw(&mut self, _drawer: &mut ComponentDrawer) {
        if let Some((controller, mount)) = &self.binding {
            controller.published(*mount);
        }
    }
}
impl Drop for FixturePublicationHook {
    fn drop(&mut self) {
        if let Some((controller, mount)) = &self.binding {
            controller.unmount(*mount);
        }
    }
}
pub(crate) fn use_fixture_controller(
    hooks: &mut Hooks<'_, '_>,
    controller: Option<QuotaFixtureController>,
    live: LiveQuotaFixture,
) {
    hooks.use_hook(|| FixturePublicationHook {
        binding: controller.and_then(|controller| {
            let context = live.context.clone();
            match controller.mount(live) {
                Ok(mount) => Some((controller, mount)),
                Err(error) => {
                    context.report(error);
                    None
                }
            }
        }),
    });
}
