//! Real-input tour of the existing component fixture; external ports stay synthetic.
use super::{FONT_FAMILY, TestResult, ensure};

pub(super) const NORMAL_CHECKPOINTS: [&str; 42] = [
    "component-quota-browse",
    "component-quota-browse-credit-window",
    "component-quota-browse-pace-heading",
    "component-quota-browse-pace-value",
    "component-quota-browse-page-returned",
    "component-quota-next-account",
    "component-quota-restored-account",
    "component-quota-details-top",
    "component-quota-details-bottom",
    "component-quota-help-top",
    "component-quota-help-bottom",
    "component-quota-floor-original",
    "component-quota-floor-draft",
    "component-quota-floor-cancelled",
    "component-quota-floor-saved",
    "component-quota-options-identity",
    "component-quota-reset-inventory",
    "component-quota-reset-last-page",
    "component-quota-credits-identity",
    "component-quota-credits-balance",
    "component-quota-policy-original",
    "component-quota-policy-choice",
    "component-quota-policy-no-match",
    "component-quota-policy-choice-returned",
    "component-quota-policy-draft",
    "component-quota-policy-cancelled",
    "component-quota-policy-saved",
    "component-quota-refresh-result",
    "component-quota-credits-refreshed-balance",
    "component-quota-credits-refreshed-freshness",
    "component-quota-credits-refreshed-observation",
    "component-quota-credits-policy-boundary",
    "component-quota-reset-review",
    "component-quota-confirm-no",
    "component-quota-confirm-no-returned",
    "component-quota-confirm-yes",
    "component-quota-confirm-help-top",
    "component-quota-confirm-help-identity",
    "component-quota-confirm-help-returned",
    "component-quota-reset-result",
    "component-quota-reset-result-bottom",
    "component-quota-returned",
];

// Forced bottom scrolling can hide the preceding [voucher-7] row. The actual
// final page still exposes this complete fixture title, including its ordinal.
const LAST_RESET_VOUCHER_READY: &str =
    r"Options · Resets.*Synthetic[\s│┃]+reset[\s│┃]+voucher[\s│┃]+7\b.*Review";

// Actual32 body wraps this success sentence, including VHS's padded frame edge.
// Preserve every word, its order and punctuation; never wildcard intervening text.
const CREDIT_REFRESH_READY: &str = r"Options · Credits.*alpha-tag.*[\s│┃]Credit[\s│┃]{1,1000}balance[\s│┃]{1,1000}refreshed\.(?:[\s│┃]|$)";
// Same physical modal row gap for both field labels: the next screen row may
// retain background text before its modal border. Image-field proof stays strict.
const MODAL_FIELD_ROW_GAP: &str =
    r"(?:[ \t│┃]{1,1000}|[ \t│┃]{0,1000}\r?\n[^\r\n]{0,1000}[│┃][ \t]*)";

fn credit_field_readiness(label: &str, value: &str) -> String {
    format!(r"Options · Credits.*alpha-tag.*[\s│┃]{label}{MODAL_FIELD_ROW_GAP}{value}(?:[\s│┃]|$)")
}

const RESET_SUCCESS_READY: &str = r"Reset result.*alpha-tag.*[\s│┃]Success[\s│┃]{1,1000}—[\s│┃]{1,1000}reset[\s│┃]{1,1000}completed(?:[\s│┃]|$)";

pub(super) fn validate_normal_checkpoints(checkpoints: &[&str]) -> TestResult<()> {
    ensure(
        checkpoints == NORMAL_CHECKPOINTS,
        "component tour must preserve ordered identity, draft, cancel, save and reset evidence",
    )
}

// Readiness follows the selected name column across wrapped terminal rows.
// Original-image review, not this wait, proves the rendered identity and style.
fn selected_account_readiness_expression(words: &[&str]) -> String {
    // Only a real row boundary can split letters. Its remainder starts with a
    // column gap, so name characters cannot be skipped as unrelated row content.
    // Compact rows have exactly the outer border plus two gutter cells. Retain
    // the observed wide prefixes, excluding the farther-right Status/Quota.
    // VHS Wait may pad a short physical row with hundreds of blank cells.
    const ROW_CONTINUATION: &str = r"(?:[ \t]+[^\r\n]{0,220})?[ \t]*\r?\n(?:│ {2}|[│ ]{4,12})";
    let letter_gap = format!("(?:{ROW_CONTINUATION})?");
    let word_gap = format!(r"(?:[ \t]{{1,8}}|{ROW_CONTINUATION})");
    let fragments = words
        .iter()
        .map(|word| {
            word.chars()
                .map(|character| character.to_string())
                .collect::<Vec<_>>()
                .join(&letter_gap)
        })
        .collect::<Vec<_>>();
    format!(
        r"❯[ \t]{{1,4}}{}(?:[ \t│┃\r\n]|$)",
        fragments.join(&word_gap)
    )
}

fn checkpoint(tape: &mut String, name: &str, expression: &str) {
    tape.push_str(&format!(
        "Wait+Screen /(?s){expression}/\nSleep 250ms\nScreenshot {name}.png\nSleep 250ms\n"
    ));
}

fn component_tape_settings(columns: u16, rows: u16) -> String {
    format!(
        "Set Shell bash\nSet Columns {columns}\nSet Rows {rows}\nSet FontFamily {FONT_FAMILY}\nSet FontSize 16\nSet Padding 0\nSet CursorBlink false\nSet WaitTimeout 8s\nOutput screens.txt\nOutput diagnostic/frames/\n"
    )
}

// VHS exports scheduled screenshots only after the entire tape ends successfully.
// This diagnostic stops before navigation; it cannot stand in for the normal tour.
pub(super) fn build_first_frame_diagnostic_tape() -> String {
    let mut tape = component_tape_settings(48, 12);
    checkpoint(&mut tape, "component-quota-browse", "Quota status.*OpenAI");
    tape.push_str("Ctrl+C\nWait+Line /^QUOTA_STORYBOARD_CHILD_EXIT=0$/\n");
    tape
}

pub(super) fn build_failure_retention_control_tape() -> String {
    let mut tape = component_tape_settings(32, 24);
    checkpoint(&mut tape, "component-quota-browse", "Quota status.*OpenAI");
    // A fixed natural command error exercises VHS's deferred raw-layer move.
    // Never accept an arbitrary failing command or substitute for a normal tour.
    tape.push_str(&format!("Wait+Screen /{}/\n", super::IMPOSSIBLE_SCREEN));
    tape
}

pub(super) fn build_normal_component_tape(columns: u16, rows: u16) -> TestResult<String> {
    // Current mounted normal-fixture preflight, including the saved1% notice.
    // These are body offsets from Up256, not a relaxed readiness predicate.
    let (last_id_down, last_title_down, balance_down) = match (columns, rows) {
        (32, 24) => (3, 5, 0),
        (48, 12) => (7, 8, 1),
        (40, 24) => (3, 4, 0),
        (48, 24) => (2, 3, 0),
        (76, 30) => (3, 4, 0),
        (120, 36) => (0, 0, 0),
        _ => return Err("component viewport is not declared".into()),
    };
    let mut tape = component_tape_settings(columns, rows);
    checkpoint(&mut tape, "component-quota-browse", "Quota status.*OpenAI");
    if (columns, rows) == (48, 12) {
        tape.push_str("PageDown\n");
    }
    checkpoint(
        &mut tape,
        "component-quota-browse-credit-window",
        r"Quota status.*62\.5k",
    );
    if (columns, rows) == (48, 12) {
        tape.push_str("PageDown\n");
    }
    checkpoint(
        &mut tape,
        "component-quota-browse-pace-heading",
        r"Quota status.*99%.*5h.*Weekly[\s│┃]{1,1000}pace",
    );
    if (columns, rows) == (48, 12) {
        tape.push_str("PageDown@1ms 256\nPageUp\nPageUp\nPageUp\n");
    }
    checkpoint(
        &mut tape,
        "component-quota-browse-pace-value",
        if columns == 32 {
            r"Quota status.*Weekly[\s│┃]{1,1000}pace[\s│┃]{1,1000}Unknown(?:[\s│┃]|$)"
        } else {
            r"Quota status.*burn[\s│┃]{1,1000}u[\s│┃]{0,1000}n[\s│┃]{0,1000}a[\s│┃]{0,1000}v[\s│┃]{0,1000}a[\s│┃]{0,1000}i[\s│┃]{0,1000}l[\s│┃]{0,1000}a[\s│┃]{0,1000}b[\s│┃]{0,1000}l[\s│┃]{0,1000}e"
        },
    );
    if (columns, rows) == (48, 12) {
        tape.push_str("PageUp@1ms 256\n");
    }
    checkpoint(
        &mut tape,
        "component-quota-browse-page-returned",
        &selected_account_readiness_expression(&[
            "東京", "OpenAI", "Research", "Account", "Alpha", "Long", "Name",
        ]),
    );
    tape.push_str("Down\n");
    checkpoint(
        &mut tape,
        "component-quota-next-account",
        &selected_account_readiness_expression(&[
            "Claude", "Research", "Account", "Beta", "Full", "Name",
        ]),
    );
    tape.push_str("Up\n");
    checkpoint(
        &mut tape,
        "component-quota-restored-account",
        &selected_account_readiness_expression(&[
            "東京", "OpenAI", "Research", "Account", "Alpha", "Long", "Name",
        ]),
    );
    tape.push_str("F4\nUp@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-details-top",
        "Account details.*Status.*Reason",
    );
    tape.push_str("Down@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-details-bottom",
        "Account details.*Close",
    );
    tape.push_str("Escape\nF1\nUp@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-help-top",
        "Quota Help.*Current quota.*Close",
    );
    tape.push_str("Down@1ms 256\n");
    if (columns, rows) == (48, 12) {
        tape.push_str("Up\n");
    }
    checkpoint(
        &mut tape,
        "component-quota-help-bottom",
        "Quota Help.*Close",
    );
    tape.push_str("Escape\nCtrl+E\nDown@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-floor-original",
        "Weekly floor.*Floor.*0%",
    );
    tape.push_str("Right\n");
    checkpoint(
        &mut tape,
        "component-quota-floor-draft",
        "Weekly floor.*Floor.*1%",
    );
    tape.push_str("Escape\nCtrl+E\nDown@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-floor-cancelled",
        "Weekly floor.*Floor.*0%",
    );
    tape.push_str("Right\nEnter\nWait+Screen /Quota status/\nCtrl+E\nUp@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-floor-saved",
        "Weekly floor.*Current.*1%",
    );
    tape.push_str("Escape\nCtrl+R\n");
    checkpoint(
        &mut tape,
        "component-quota-options-identity",
        r"Options · Resets.*alpha-tag.*(?:Checking[\s│┃]{1,1000}reset|Live[\s│┃]{1,1000}eligibility|\[voucher-[0-9]+\])",
    );
    tape.push_str("Down@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-reset-inventory",
        "Options · Resets.*Review",
    );
    // Seven fixture vouchers with one-to-four visible entries per page.
    // Six NextPage actions reach the clamped last page at every declared size.
    tape.push_str("PageDown\nPageDown\nPageDown\nPageDown\nPageDown\nPageDown\nUp@1ms 256\n");
    tape.push_str(&"Down\n".repeat(last_id_down));
    tape.push_str("Wait+Screen /(?s)Options · Resets.*alpha-tag.*\\[voucher-7\\]/\n");
    tape.push_str(&"Down\n".repeat(last_title_down - last_id_down));
    checkpoint(
        &mut tape,
        "component-quota-reset-last-page",
        LAST_RESET_VOUCHER_READY,
    );
    tape.push_str("Tab\nWait+Screen /Options · Credits/\n");
    checkpoint(
        &mut tape,
        "component-quota-credits-identity",
        r"Options · Credits.*alpha-tag.*(?:Use[\s│┃]{1,1000}credits|Credit[\s│┃]{1,1000}balance)",
    );
    // Original identity and balance are separate same-process body views on short grids.
    tape.push_str("Up@1ms 256\n");
    tape.push_str(&"Down\n".repeat(balance_down));
    checkpoint(
        &mut tape,
        "component-quota-credits-balance",
        "Options · Credits.*alpha-tag.*62,500",
    );
    tape.push_str("Down@1ms 256\nEnter\nEscape\n");
    checkpoint(
        &mut tape,
        "component-quota-policy-original",
        "Credit usage.*Disallow.*Save",
    );
    tape.push_str("Right\n");
    checkpoint(
        &mut tape,
        "component-quota-policy-choice",
        "Credit usage.*alpha-tag.*❯[ \\t]+Allow.*Cancel.*Select",
    );
    tape.push_str("Type \"zznomatchzz\"\n");
    checkpoint(
        &mut tape,
        "component-quota-policy-no-match",
        "Credit usage.*No matching[\\s│┃┌┐]{1,128}choices",
    );
    tape.push_str("Enter\nEscape\n");
    checkpoint(
        &mut tape,
        "component-quota-policy-choice-returned",
        "Credit usage.*Use credits.*Disallow.*Save",
    );
    tape.push_str("Right\n");
    tape.push_str("Enter\n");
    checkpoint(
        &mut tape,
        "component-quota-policy-draft",
        "Credit usage.*Allow.*Save",
    );
    tape.push_str("Escape\nEnter\nEscape\n");
    checkpoint(
        &mut tape,
        "component-quota-policy-cancelled",
        "Credit usage.*Disallow.*Save",
    );
    tape.push_str("Right\nEnter\nEnter\nWait+Screen /Options · Credits/\nEnter\nEscape\n");
    checkpoint(
        &mut tape,
        "component-quota-policy-saved",
        "Credit usage.*Allow.*Save",
    );
    tape.push_str("Escape\nType \"r\"\n");
    checkpoint(
        &mut tape,
        "component-quota-refresh-result",
        CREDIT_REFRESH_READY,
    );
    // Explicit current-response contrast. Final per-grid positions are checked
    // against Main's current normal-fixture procedure before this is released.
    tape.push_str("Up@1ms 256\n");
    tape.push_str(&"Down\n".repeat(balance_down));
    checkpoint(
        &mut tape,
        "component-quota-credits-refreshed-balance",
        "Options · Credits.*alpha-tag.*63,000",
    );
    let (freshness_down, observation_down, policy_down) = match (columns, rows) {
        (32, 24) => (0, 1, 3),
        (76, 30) => (0, 1, 3),
        (48, 12) => (3, 5, 7),
        (40 | 48, 24) => (0, 0, 2),
        (120, 36) => (0, 0, 0),
        _ => return Err("component viewport is not declared".into()),
    };
    tape.push_str(&"Down\n".repeat(freshness_down - balance_down));
    checkpoint(
        &mut tape,
        "component-quota-credits-refreshed-freshness",
        &credit_field_readiness("Freshness", "Fresh"),
    );
    tape.push_str(&"Down\n".repeat(observation_down - freshness_down));
    checkpoint(
        &mut tape,
        "component-quota-credits-refreshed-observation",
        &credit_field_readiness("Observation", r"checked[\s│┃]{1,1000}0s[\s│┃]{1,1000}ago"),
    );
    tape.push_str(&"Down\n".repeat(policy_down - observation_down));
    checkpoint(
        &mut tape,
        "component-quota-credits-policy-boundary",
        "Options · Credits.*alpha-tag.*Credit usage",
    );
    tape.push_str("Tab\nWait+Screen /(?s)Options · Resets.*Review/\nDown@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-reset-review",
        "Options · Resets.*Review",
    );
    tape.push_str("Enter\nDown@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-confirm-no",
        "Confirm reset.*1 credit.*Cannot undo.*Reset quota\\?.*No.*Yes.*Cancel.*Confirm",
    );
    tape.push_str("Enter\nDown@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-confirm-no-returned",
        &format!(
            r"Options · Resets.*alpha-tag.*[\s│┃]Eligibility{MODAL_FIELD_ROW_GAP}Not[\s│┃]{{1,1000}}checked(?:[\s│┃]|$).*Close.*Inspect"
        ),
    );
    tape.push_str("Enter\nWait+Screen /(?s)Options · Resets.*Review/\nEnter\nDown@1ms 256\nWait+Screen /(?s)Confirm reset.*Reset quota\\?.*No.*Yes.*Cancel.*Confirm/\nRight\nDown@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-confirm-yes",
        "Confirm reset.*1 credit.*Cannot undo.*Reset quota\\?.*No.*Yes.*Cancel.*Confirm",
    );
    tape.push_str("F1\nUp@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-confirm-help-top",
        "Quota Help.*Selected account.*Account",
    );
    tape.push_str("Down\n");
    checkpoint(
        &mut tape,
        "component-quota-confirm-help-identity",
        "Quota Help.*Research[\\s│┃]{1,1000}Account[\\s│┃]{1,1000}Alpha[\\s│┃]{1,1000}Long[\\s│┃]{1,1000}Name[\\s│┃]{1,1000}\\[alpha-tag\\]",
    );
    tape.push_str("Escape\nDown@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-confirm-help-returned",
        "Confirm reset.*1 credit.*Cannot undo.*Reset quota\\?.*No.*Yes.*Cancel.*Confirm",
    );
    tape.push_str("Enter\nUp@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-reset-result",
        RESET_SUCCESS_READY,
    );
    tape.push_str("Down@1ms 256\n");
    checkpoint(
        &mut tape,
        "component-quota-reset-result-bottom",
        "Reset result.*Return to quota status",
    );
    // Keep a real returned browse image before q; the receipt follows actual exit.
    tape.push_str("Escape\nWait+Screen /(?s)Options · Resets.*Close.*Inspect/\nEscape\n");
    checkpoint(
        &mut tape,
        "component-quota-returned",
        &format!(
            "Quota status.*{}",
            selected_account_readiness_expression(&[
                "東京", "OpenAI", "Research", "Account", "Alpha", "Long", "Name",
            ])
        ),
    );
    tape.push_str("Type \"q\"\nWait+Line /^QUOTA_STORYBOARD_CHILD_EXIT=0$/\n");
    Ok(tape)
}

#[cfg(test)]
#[path = "quota_component_tape_test.rs"]
mod tests;

#[cfg(test)]
#[path = "quota_component_observation_wait_test.rs"]
mod observation_wait_tests;
