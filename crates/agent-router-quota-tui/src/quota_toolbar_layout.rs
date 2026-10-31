//! The toolbar reserves its fixed menu and visible account-count suffix once.
use super::quota_control_context::QuotaControlContext;
use agent_collaboration_tui::{actions::ActionRegistry, controls::toolbar_actions};
use unicode_width::UnicodeWidthStr;

// MenuButton: glyph + title + F10 + two internal gaps + padding, then one gap.
const MENU_CELLS: usize = 13;

#[derive(Default)]
pub(crate) struct QuotaToolbarLayout {
    pub(crate) caption: String,
    pub(crate) action_width: u16,
}

pub(crate) fn toolbar_layout(
    context: &QuotaControlContext,
    registry: &ActionRegistry,
) -> QuotaToolbarLayout {
    let model = context.view_model.read();
    let visible = context
        .filters
        .read()
        .committed
        .visible_rows(&model.rows)
        .len();
    let width = context.width.get();
    let total = model.rows.len();
    let compact = format!("Quota · {visible}/{total}");
    let accounts = format!(
        "{compact} {}",
        if total == 1 { "account" } else { "accounts" }
    );
    let clients = model
        .header
        .observed_clients
        .map_or_else(String::new, |clients| {
            format!(
                " · {clients} {}",
                if clients == 1 { "client" } else { "clients" }
            )
        });
    let full = format!("{accounts}{clients}");
    let counted = format!("{compact}{clients}");
    // Preserve the crate's compact-caption button prefix before expanding the caption.
    // Its own width projection remains the owner of both button paint and focus eligibility.
    let buttons = toolbar_actions(registry, toolbar_action_width(width, &compact)).len();
    let available = toolbar_content_width(width).saturating_sub(MENU_CELLS + 1);
    let caption = [full, counted]
        .into_iter()
        .find(|caption| {
            caption.as_str().width() <= available
                && toolbar_actions(registry, toolbar_action_width(width, caption)).len() == buttons
        })
        .unwrap_or(compact);
    QuotaToolbarLayout {
        action_width: toolbar_action_width(width, &caption),
        caption,
    }
}

fn toolbar_action_width(width: usize, suffix: &str) -> u16 {
    let suffix_cells = suffix.width() + usize::from(!suffix.is_empty());
    u16::try_from(toolbar_content_width(width).saturating_sub(MENU_CELLS + suffix_cells))
        .unwrap_or(u16::MAX)
}

fn toolbar_content_width(width: usize) -> usize {
    width.saturating_sub(if width < 48 { 2 } else { 4 })
}

#[cfg(test)]
mod tests {
    use iocraft::prelude::*;

    #[tokio::test]
    async fn caption_shrinks_before_toolbar_actions_drop() {
        for width in [160u16, 120] {
            let mut model = crate::quota_browse_presentation_test::quota_two_account_view_model();
            model.header.observed_clients = Some(1);
            let mut fixture = crate::test_support::quota_control_fixture(
                crate::test_support::QuotaFixtureOptions {
                    view_model: model,
                    ports: crate::QuotaStatusViewOptions {
                        weekly_floor_saver: Some(std::sync::Arc::new(|_, _| {
                            panic!("caption cannot save")
                        })),
                        ..Default::default()
                    },
                    width,
                    height: 36,
                    control: crate::test_support::QuotaFixtureControl::Browse,
                },
            );
            let frames =
                fixture
                    .element
                    .mock_terminal_render_loop(MockTerminalConfig::with_events(
                        futures_util::stream::pending::<TerminalEvent>(),
                    ));
            tokio::pin!(frames);
            let frame = crate::quota_frame_observation::next_matching_frame(
                frames.as_mut(),
                std::time::Duration::from_secs(2),
                |canvas| canvas.to_string().contains("Menu F10"),
            )
            .await
            .unwrap()
            .to_string();
            let toolbar = frame
                .lines()
                .find(|line| line.contains("Menu F10"))
                .unwrap();
            for label in ["Details", "Filters:", "Pool", "Options", "Floor"] {
                assert!(
                    toolbar.contains(label),
                    "{width}-cell caption must preserve {label}: {frame}"
                );
            }
            let caption = if width == 160 {
                "Quota · 2/2 accounts · 1 client"
            } else {
                "Quota · 2/2 · 1 client"
            };
            assert!(
                toolbar.contains(caption),
                "{width}-cell caption uses the remaining toolbar space: {frame}"
            );
        }
    }
}
