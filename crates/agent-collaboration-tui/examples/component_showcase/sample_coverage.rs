//! Ordinary layout fixtures place real UiText beneath each overlay border row.
use super::{
    sample_dialogs::{SampleDialogKind, open_dialog},
    sample_model::{CoverageFixture, CoverageGlyph, CoverageScene, SampleData, action, target_id},
};
use agent_collaboration_tui::{
    actions::{ActionDef, Placement},
    controls::{self, ComboBoxBinding, ComboBoxControl, HelpControl},
    surface_handle::{SurfaceContext, SurfaceStackHandle},
    surface_stack::{OverlayLayout, SurfaceId},
};
use iocraft::prelude::*;
#[expect(
    clippy::expect_used,
    reason = "Fixed coverage fixtures must fail visibly if the declared public overlay cannot be opened."
)]
pub fn coverage_actions(
    stack: SurfaceStackHandle,
    data: &SampleData,
    help: HelpControl,
) -> Vec<ActionDef> {
    let data = *data;
    [
        (
            "coverage-menu-folder",
            "Menu coverage 📂",
            CoverageGlyph::Folder,
            CoverageScene::Menu,
        ),
        (
            "coverage-menu-mail",
            "Menu coverage ✉️",
            CoverageGlyph::Mail,
            CoverageScene::Menu,
        ),
        (
            "coverage-menu-cjk",
            "Menu coverage 界",
            CoverageGlyph::Cjk,
            CoverageScene::Menu,
        ),
        (
            "coverage-form-folder",
            "Dialog coverage 📂",
            CoverageGlyph::Folder,
            CoverageScene::Dialog,
        ),
        (
            "coverage-form-mail",
            "Dialog coverage ✉️",
            CoverageGlyph::Mail,
            CoverageScene::Dialog,
        ),
        (
            "coverage-form-cjk",
            "Dialog coverage 界",
            CoverageGlyph::Cjk,
            CoverageScene::Dialog,
        ),
        (
            "coverage-dropdown-folder",
            "Dropdown coverage 📂",
            CoverageGlyph::Folder,
            CoverageScene::Dropdown,
        ),
        (
            "coverage-dropdown-mail",
            "Dropdown coverage ✉️",
            CoverageGlyph::Mail,
            CoverageScene::Dropdown,
        ),
        (
            "coverage-dropdown-cjk",
            "Dropdown coverage 界",
            CoverageGlyph::Cjk,
            CoverageScene::Dropdown,
        ),
        (
            "coverage-edge-folder",
            "Left edge coverage 📂",
            CoverageGlyph::Folder,
            CoverageScene::LeftEdge,
        ),
        (
            "coverage-edge-mail",
            "Left edge coverage ✉️",
            CoverageGlyph::Mail,
            CoverageScene::LeftEdge,
        ),
        (
            "coverage-edge-cjk",
            "Left edge coverage 界",
            CoverageGlyph::Cjk,
            CoverageScene::LeftEdge,
        ),
    ]
    .into_iter()
    .zip([
        "Ctrl+Alt+1",
        "Ctrl+Alt+2",
        "Ctrl+Alt+3",
        "Ctrl+Alt+4",
        "Ctrl+Alt+5",
        "Ctrl+Alt+6",
        "Ctrl+Alt+7",
        "Ctrl+Alt+8",
        "Ctrl+Alt+9",
        "Ctrl+Alt+0",
        "Ctrl+Alt+-",
        "Ctrl+Alt+=",
    ])
    .map(|((name, title, glyph, scene), key)| {
        let help = help.clone();
        action(
            name,
            title,
            &[key],
            Placement::Menu {
                section: "Wide glyph fixtures",
            },
            move |()| {
                data.update(|model| model.coverage = Some(CoverageFixture { glyph, scene }));
                let viewport = stack.viewport();
                match scene {
                    CoverageScene::Dialog => {
                        open_dialog(stack, &data, SampleDialogKind::Form, help.clone())
                    }
                    CoverageScene::Dropdown => {
                        ComboBoxControl::new(ComboBoxBinding {
                            parent: SurfaceContext {
                                stack,
                                surface: SurfaceId::Screen,
                            },
                            field: target_id("screen-choice"),
                            value: data.choices,
                            on_pick: Handler::from(move |_| data.record("screen-pick")),
                        })
                        .open(OverlayLayout::CenteredCells {
                            width: 40,
                            height: 10,
                        })
                        .expect("coverage dropdown");
                    }
                    CoverageScene::Menu | CoverageScene::LeftEdge => {
                        let layout = if scene == CoverageScene::LeftEdge {
                            OverlayLayout::AtCells {
                                left: 0,
                                top: 2,
                                width: 44,
                                height: viewport.1.saturating_sub(3).max(1),
                            }
                        } else {
                            OverlayLayout::CenteredCells {
                                width: 44,
                                height: viewport.1.saturating_sub(3).max(1),
                            }
                        };
                        let registry = stack.snapshot().top().actions.clone();
                        stack
                            .open_overlay(
                                controls::menu_overlay(stack, &registry, layout)
                                    .expect("coverage menu model"),
                            )
                            .expect("coverage menu identity");
                    }
                }
            },
        )
    })
    .collect()
}
#[derive(Props)]
pub struct CoverageColumnProps {
    pub fixture: Option<CoverageFixture>,
    pub viewport: (u16, u16),
}
#[component]
pub fn CoverageColumn(props: &CoverageColumnProps) -> impl Into<AnyElement<'static>> {
    let Some(fixture) = props.fixture else {
        return element!(View).into_any();
    };
    let left = fixture.column(props.viewport);
    element!(View(position:Position::Absolute,left,top:0,width:2,height:props.viewport.1,flex_direction:FlexDirection::Column,overflow:Overflow::Hidden) {
        #((0..props.viewport.1).map(|row|element!(agent_collaboration_tui::UiText(key:row,content:fixture.glyph.text().to_owned(),role:agent_collaboration_tui::UiTextRole::Body))))
    }).into_any()
}
