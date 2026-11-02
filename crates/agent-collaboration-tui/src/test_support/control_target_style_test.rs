use super::*;

fn scoped_paint(case: &mut ControlKindContract) {
    for paint in std::iter::once(&mut case.initial_paint)
        .chain(case.steps.iter_mut().map(|step| &mut step.paint))
    {
        for assertion in paint {
            if let ControlPaint::Styled { text, style } = assertion {
                *assertion = ControlPaint::StyledInTarget {
                    target: target("run"),
                    text: text.clone(),
                    style: style.clone(),
                };
            }
        }
    }
}
async fn duplicate_label_case(mut case: ControlKindContract, initial_count: usize) {
    scoped_paint(&mut case);
    let (width, height) = case.viewport;
    assert_control_kind_contract(case, move |journal, observed| {
        element!(ContractButton(journal,observed,label:"Start",width,height,duplicate_label:true,initial_count))
    }).await;
}

#[tokio::test]
async fn target_style_ignores_a_differently_styled_label_outside_its_published_cells() {
    for viewport in crate::test_support::grids::STANDARD_GRIDS {
        duplicate_label_case(contract("Start", viewport), 0).await;
    }
}

#[tokio::test]
#[should_panic(expected = "target style for")]
async fn target_style_rejects_a_restyled_real_target_even_with_a_duplicate_label() {
    duplicate_label_case(contract("Start", (76, 30)), 1).await;
}

#[tokio::test]
#[should_panic(expected = "styled target has no visible published cells")]
async fn target_style_rejects_an_absent_target() {
    let mut case = contract("Start", (76, 30));
    case.initial_paint = vec![ControlPaint::StyledInTarget {
        target: target("absent"),
        text: "Start".into(),
        style: style(false, false, false),
    }];
    run(case, "Start").await;
}

#[tokio::test]
#[should_panic(expected = "missing styled text")]
async fn target_style_rejects_text_painted_only_outside_the_target() {
    let mut case = contract("Start", (76, 30));
    case.initial_paint = vec![ControlPaint::StyledInTarget {
        target: target("other"),
        text: "Start".into(),
        style: style(false, false, false),
    }];
    run(case, "Start").await;
}

#[tokio::test]
#[should_panic(expected = "style for")]
async fn whole_frame_style_still_rejects_a_differently_styled_duplicate() {
    let case = contract("Start", (76, 30));
    assert_control_kind_contract(case, |journal,observed| element!(ContractButton(journal,observed,label:"Start",width:76u16,height:30u16,duplicate_label:true))).await;
}
