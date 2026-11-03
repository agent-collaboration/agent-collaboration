//! Query clicks and edits use the actual ComboBox overlays on quota.
use super::*;

fn query_contract(viewport: (u16, u16), field: &'static str) -> ControlKindContract {
    let credit = field == "credit-field";
    let draft = (!credit).then_some("All");
    let phase = credit.then_some("Editing");
    let closed = || combo_state("all", draft, None, phase);
    let queried = |query, highlight, cursor| {
        combo_state("all", draft, Some((field, query, highlight, cursor)), phase)
    };
    let mut steps = if credit {
        vec![
            step(
                modified(KeyCode::Char('r'), KeyModifiers::CONTROL),
                true,
                combo_state("all", None, None, None),
                &[],
                vec![contains("Resets")],
            ),
            step(
                modified(KeyCode::Tab, KeyModifiers::SHIFT),
                true,
                combo_state("all", None, None, None),
                &[],
                vec![contains("Credits")],
            ),
            step(
                key(KeyCode::Enter),
                true,
                combo_state("all", None, None, None),
                &[],
                vec![contains("Credit balance")],
            ),
            step(
                click(target("edit-credit")),
                true,
                closed(),
                &[],
                vec![contains("Disallow")],
            ),
        ]
    } else {
        let mut steps = vec![step(
            key(KeyCode::F(3)),
            true,
            closed(),
            &[],
            vec![contains("Account filters")],
        )];
        let tabs = match field {
            "provider-field" => 0,
            "disabled-field" => 1,
            "group-field" => 2,
            _ => panic!("not an actual quota option field"),
        };
        for _ in 0..tabs {
            steps.push(step(
                key(KeyCode::Tab),
                true,
                closed(),
                &[],
                vec![contains("Account filters")],
            ));
        }
        steps
    };
    let highlighted = match field {
        "provider-field" => "all",
        "disabled-field" => "shown",
        "group-field" => "none",
        "credit-field" => "disallow",
        _ => panic!("not an actual quota option field"),
    };
    let query_target = target(field).child("filter");
    steps.extend([
        step(
            click(target(field)),
            true,
            queried("", Some(highlighted), 0),
            &[],
            vec![contains("Filter:")],
        ),
        // The query owns editable input, with no activation action. A mouse down
        // must keep the overlay open and preserve the actual selected value.
        step(
            click(query_target.clone()),
            false,
            queried("", Some(highlighted), 0),
            &[],
            vec![contains("Filter:")],
        ),
        step(
            ControlInput::Event(TerminalEvent::BracketedPaste("zzz".to_owned())),
            true,
            queried("zzz", None, 3),
            &[],
            vec![
                contains("No match"),
                styled_in(
                    query_target.clone(),
                    "zzz",
                    Color::AnsiValue(15),
                    raised(),
                    false,
                    false,
                ),
            ],
        ),
        step(
            click(query_target),
            false,
            queried("zzz", None, 3),
            &[],
            vec![contains("No match")],
        ),
        step(
            key(KeyCode::Backspace),
            true,
            queried("zz", None, 2),
            &[],
            vec![contains("No match")],
        ),
        step(
            key(KeyCode::Esc),
            true,
            closed(),
            &[],
            vec![contains(if credit {
                "Use credits"
            } else {
                "Account filters"
            })],
        ),
    ]);
    contract(ControlKind::ComboBox, viewport, steps)
}

async fn run_query(field: &'static str) {
    for viewport in STANDARD_GRIDS {
        assert_control_kind_contract(query_contract(viewport, field), |journal, observed| {
            fixture::build(
                crate::quota_test_support::control_kind_contract::ControlInstanceRequest {
                    screen: "quota",
                    kind: ControlKind::ComboBox,
                    viewport,
                },
                journal,
                observed,
            )
        })
        .await;
    }
}
#[tokio::test]
async fn quota_provider_query_click_focuses_without_activation_and_accepts_edits() {
    run_query("provider-field").await;
}
#[tokio::test]
async fn quota_disabled_query_click_focuses_without_activation_and_accepts_edits() {
    run_query("disabled-field").await;
}
#[tokio::test]
async fn quota_group_query_click_focuses_without_activation_and_accepts_edits() {
    run_query("group-field").await;
}
#[tokio::test]
async fn quota_credit_query_click_focuses_without_activation_and_accepts_edits() {
    run_query("credit-field").await;
}
