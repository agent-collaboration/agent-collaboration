//! Real reset-choice Buttons share keyboard, pointer, checked and reason paint.
use super::*;

pub(super) fn button_contract(viewport: (u16, u16)) -> ControlKindContract {
    let began = ["begin-inspection:test-alpha:1"];
    let selected = ["begin-inspection:test-alpha:1", "select-no"];
    let clicked = ["begin-inspection:test-alpha:1", "select-no", "select-no"];
    contract(
        ControlKind::Button,
        viewport,
        vec![
            step(
                modified(KeyCode::Char('r'), KeyModifiers::CONTROL),
                true,
                button_state(true),
                &began,
                vec![
                    contains("Confirm reset"),
                    contains("Cancel"),
                    styled_in(
                        target("reset-advance"),
                        "Confirm",
                        Color::AnsiValue(15),
                        base(),
                        false,
                        false,
                    ),
                    styled_in(
                        target("reset-no"),
                        "No",
                        Color::AnsiValue(11),
                        base(),
                        true,
                        false,
                    ),
                ],
            ),
            step(
                key(KeyCode::Tab),
                true,
                button_state(true),
                &began,
                vec![
                    contains("● No"),
                    styled_in(
                        target("reset-no"),
                        "No",
                        Color::AnsiValue(11),
                        base(),
                        true,
                        true,
                    ),
                    styled_in(target("reset-yes"), "Yes", disabled(), base(), false, false),
                    contains("Reset is"),
                    contains("unavailable"),
                    // All words must remain visible inside the disabled target,
                    // including the narrow word-wrapped reason.
                    styled_in(
                        target("reset-yes"),
                        "Reset is",
                        disabled(),
                        base(),
                        false,
                        false,
                    ),
                    styled_in(
                        target("reset-yes"),
                        "unavailable",
                        disabled(),
                        base(),
                        false,
                        false,
                    ),
                ],
            ),
            step(
                key(KeyCode::Enter),
                false,
                button_state(true),
                &selected,
                vec![contains("● No")],
            ),
            step(
                hover(target("domain-close")),
                true,
                button_state(true),
                &selected,
                vec![styled_in(
                    target("domain-close"),
                    "Cancel",
                    primary(),
                    raised(),
                    false,
                    false,
                )],
            ),
            step(
                hover(target("reset-yes")),
                true,
                button_state(true),
                &selected,
                vec![styled_in(
                    target("reset-yes"),
                    "Yes",
                    disabled(),
                    base(),
                    false,
                    false,
                )],
            ),
            step(
                click(target("reset-yes")),
                false,
                button_state(true),
                &selected,
                vec![
                    contains("● No"),
                    styled_in(
                        target("reset-yes"),
                        "Reset is",
                        disabled(),
                        base(),
                        false,
                        false,
                    ),
                    styled_in(
                        target("reset-yes"),
                        "unavailable",
                        disabled(),
                        base(),
                        false,
                        false,
                    ),
                ],
            ),
            step(
                hover(target("reset-no")),
                true,
                button_state(true),
                &selected,
                vec![contains("● No")],
            ),
            step(
                click(target("reset-no")),
                false,
                button_state(true),
                &clicked,
                vec![contains("● No")],
            ),
            step(
                key(KeyCode::Tab),
                true,
                button_state(true),
                &clicked,
                vec![
                    contains("Confirm"),
                    styled_in(
                        target("reset-advance"),
                        "Confirm",
                        Color::AnsiValue(11),
                        base(),
                        true,
                        true,
                    ),
                ],
            ),
            step(
                key(KeyCode::Tab),
                true,
                button_state(true),
                &clicked,
                vec![
                    styled_in(
                        target("domain-close"),
                        "Cancel",
                        Color::AnsiValue(11),
                        base(),
                        true,
                        true,
                    ),
                    contains("● No"),
                    styled_in(
                        target("reset-no"),
                        "No",
                        Color::AnsiValue(11),
                        raised(),
                        true,
                        false,
                    ),
                ],
            ),
        ],
    )
}
