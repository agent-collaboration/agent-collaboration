//! Quota window emphasis belongs to the account presentation, not generic controls.
use iocraft::prelude::*;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum QuotaTextRole {
    WeeklyWindow,
    ShortWindow,
    AccountName { focused: bool },
    WeeklyPace(codex_router_quota_service::quota_display_model::ResetPaceState),
    Secondary,
}

impl QuotaTextRole {
    pub(crate) const fn color(self) -> Color {
        match self {
            Self::AccountName { focused: true } => Color::Yellow,
            Self::AccountName { focused: false } => Self::WeeklyWindow.color(),
            Self::WeeklyPace(state) => super::quota_fact_presentation::reset_pace_color(state),
            Self::Secondary => Color::Grey,
            Self::WeeklyWindow => Color::Rgb {
                r: 237,
                g: 240,
                b: 245,
            },
            Self::ShortWindow => Color::Rgb {
                r: 206,
                g: 212,
                b: 221,
            },
        }
    }
}

#[derive(Props)]
pub(crate) struct QuotaTextProps {
    pub content: String,
    #[iocraft(required)]
    pub role: QuotaTextRole,
    pub wrap: Option<TextWrap>,
    pub weight: Option<Weight>,
}

#[component]
pub(crate) fn QuotaText(props: &QuotaTextProps, hooks: Hooks) -> impl Into<AnyElement<'static>> {
    let theme = hooks
        .try_use_context::<agent_collaboration_tui::surface_handle::SurfaceContext>()
        .map_or(
            agent_collaboration_tui::theme::Theme::default(),
            |context| context.theme(),
        );
    element!(Text(content:props.content.clone(),color:theme.foreground_color(props.role.color()),weight:props.weight.unwrap_or(Weight::Normal),
        wrap:props.wrap.unwrap_or(TextWrap::NoWrap)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_collaboration_tui::{
        actions::{ActionRegistry, ScopeId},
        focus_model::FocusModel,
        surface_handle::{SurfaceContext, use_surface_stack},
        surface_stack::{
            OverlayKind, OverlayLayout, OverlaySpec, OverlayView, SurfaceId, SurfaceModel,
        },
    };

    #[derive(Props)]
    struct CoveredQuotaTextProps {
        #[iocraft(required)]
        role: QuotaTextRole,
        covered: bool,
    }

    fn empty_surface() -> SurfaceModel {
        SurfaceModel {
            focus: FocusModel::new(Vec::new()).unwrap(),
            actions: ActionRegistry::new(ScopeId::new("quota-window-text").unwrap(), Vec::new())
                .unwrap(),
            primary_action: None,
        }
    }

    #[component]
    fn CoveredQuotaText(
        props: &CoveredQuotaTextProps,
        mut hooks: Hooks,
    ) -> impl Into<AnyElement<'static>> {
        let stack = use_surface_stack(&mut hooks, |_| empty_surface());
        let mut opened = hooks.use_ref(|| false);
        if props.covered && !opened.get() {
            stack
                .open_overlay(OverlaySpec {
                    kind: OverlayKind::Menu,
                    model: empty_surface(),
                    view: OverlayView::new(|| element!(View).into_any()),
                    layout: OverlayLayout::AtCells {
                        left: 15,
                        top: 0,
                        width: 3,
                        height: 1,
                    },
                })
                .unwrap();
            opened.set(true);
        }
        element!(ContextProvider(value:Context::owned(SurfaceContext {stack,surface:SurfaceId::Screen})) {QuotaText(content:"92% · week",role:props.role)})
    }

    #[test]
    fn window_roles_use_the_real_covered_context_and_keep_uncovered_styles() {
        for role in [QuotaTextRole::WeeklyWindow, QuotaTextRole::ShortWindow] {
            for covered in [false, true] {
                let canvas = element!(CoveredQuotaText(role, covered)).render(None);
                let style = canvas.cell(0, 0).unwrap().text_style().unwrap();
                assert_eq!(
                    style.color,
                    Some(if covered {
                        Color::Rgb {
                            r: 142,
                            g: 150,
                            b: 163,
                        }
                    } else {
                        role.color()
                    }),
                    "{role:?},covered={covered}"
                );
            }
        }
    }

    #[test]
    fn visual_window_roles_follow_the_adopted_palette() {
        for (role, expected) in [
            (
                QuotaTextRole::WeeklyWindow,
                Color::Rgb {
                    r: 237,
                    g: 240,
                    b: 245,
                },
            ),
            (
                QuotaTextRole::ShortWindow,
                Color::Rgb {
                    r: 206,
                    g: 212,
                    b: 221,
                },
            ),
        ] {
            let canvas = element!(QuotaText(content:"92% · week",role)).render(None);
            assert_eq!(
                canvas.cell(0, 0).unwrap().text_style().unwrap().color,
                Some(expected)
            );
        }
    }
}
