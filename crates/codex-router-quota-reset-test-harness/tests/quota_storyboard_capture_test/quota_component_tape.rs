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

// The 48x12 Normal body has 38 rows and a four-row viewport. Saturate
// its existing paging owner, acknowledge the bottom, then reverse to the
// complete Alpha group witnessed by the permanent mounted test.
fn append_short_normal_group_pose(tape: &mut String, reverse_pages: usize) {
    tape.push_str(
        "PageDown@1ms 256\nWait+Screen /(?s)Quota status.*Extra.*Credits unknown.*2 resets/\n",
    );
    tape.push_str(&"PageUp\n".repeat(reverse_pages));
}

fn credit_field_readiness(label: &str, value: &str) -> String {
    format!(r"Options · Credits.*alpha-tag.*[\s│┃]{label}{MODAL_FIELD_ROW_GAP}{value}(?:[\s│┃]|$)")
}

// Actual76 Help wraps the selected name onto the next modal row after background
// browse meter glyphs: every word stays in order. The Help image ROI stays strict.
fn help_identity_readiness() -> String {
    let words = [
        "Research",
        "Account",
        "Alpha",
        "Long",
        "Name",
        r"\[alpha-tag\]",
    ];
    format!("Quota Help.*{}", words.join(MODAL_FIELD_ROW_GAP))
}

const RESET_SUCCESS_READY: &str = r"Reset result.*alpha-tag.*[\s│┃]Success[\s│┃]{1,1000}—[\s│┃]{1,1000}reset[\s│┃]{1,1000}completed(?:[\s│┃]|$)";

// Narrow Details clips the account name before its reserved tag (hard 32x24
// "Claude Resea… [beta-tag]"); the captured tag, notice and Close stay exact.
const HARD_DETAILS_READY: &str =
    r"Account details.*Claude Resea(?:rch|…).*\[beta-tag\].*Credentials locked.*Close";

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

// The Beta windows pose waits on two expressions over one settled screen. The
// first binds the selection marker to Beta's whole wrapped name; the second
// requires Beta's window facts after that marker. At 120x36 the name wraps
// after "Account" with the week window on the marker row itself, so the facts
// can precede the end of the name.
fn hard_beta_selected_name_readiness() -> String {
    format!(
        "Quota status.*{}",
        selected_account_readiness_expression(&[
            "Claude", "Research", "Account", "Beta", "Full", "Name",
        ])
    )
}

const HARD_BETA_WINDOW_FACTS_READY: &str =
    "Quota status.*❯.*75% · week · —.*week stale.*No data · 5h · —";

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
    // Forward poses start at the top; short complete groups reverse from the
    // acknowledged bottom so their heading and literal value remain together.
    let short_groups = (columns, rows) == (48, 12);
    let (credit_pages, heading_pages, value_pages) = match (columns, rows) {
        (48, 12) => (0, 2, 0),
        (48, 24) | (32, 24) => (1, 0, 0),
        _ => (0, 0, 0),
    };
    if short_groups {
        append_short_normal_group_pose(&mut tape, 5);
    } else {
        tape.push_str(&"PageDown\n".repeat(credit_pages));
    }
    // 2026-10-23 C6: browse credit text is quantity-first with its unit.
    checkpoint(
        &mut tape,
        "component-quota-browse-credit-window",
        r"Quota status.*62\.5k credits",
    );
    if short_groups || credit_pages > 0 {
        tape.push_str("PageUp@1ms 256\n");
    }
    tape.push_str(&"PageDown\n".repeat(heading_pages));
    // C1/C2: wide puts "Weekly pace" in the header row above the window line;
    // folded repeats it as the group heading after the Quota group.
    checkpoint(
        &mut tape,
        "component-quota-browse-pace-heading",
        r"Quota status.*(?:99% · 5h.*Weekly[\s│┃]{1,1000}pace|Weekly[\s│┃]{1,1000}pace.*99% · 5h)",
    );
    if heading_pages > 0 {
        tape.push_str("PageUp@1ms 256\n");
    }
    if short_groups {
        append_short_normal_group_pose(&mut tape, 6);
    } else {
        tape.push_str(&"PageDown\n".repeat(value_pages));
    }
    // The interactive Weekly pace group renders "burn unavailable" at every
    // width (quota_account_groups.rs); the old 32-column "Unknown" is retired.
    checkpoint(
        &mut tape,
        "component-quota-browse-pace-value",
        r"Quota status.*burn[\s│┃]{1,1000}u[\s│┃]{0,1000}n[\s│┃]{0,1000}a[\s│┃]{0,1000}v[\s│┃]{0,1000}a[\s│┃]{0,1000}i[\s│┃]{0,1000}l[\s│┃]{0,1000}a[\s│┃]{0,1000}b[\s│┃]{0,1000}l[\s│┃]{0,1000}e",
    );
    if short_groups || value_pages > 0 {
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
        &help_identity_readiness(),
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

// 2026-10-23 C2/C3/C6 hard cases on the separate `visual-hard-cases` fixture
// (quota_storyboard_fixture_test.rs fixture_scenario_model): Alpha has known
// windows and resets, Beta a stale weekly window with unknown reset, no 5h data
// and stale credits; every row and the header carry "Credentials locked".
// 120x36 only: the whole Pool body fits there.
pub(super) const HARD_CASE_CHECKPOINTS: [&str; 9] = [
    "component-quota-hard-browse",
    "component-quota-hard-beta-windows",
    "component-quota-hard-beta-extra",
    "component-quota-hard-pool-details",
    "component-quota-hard-pool-details-complete",
    "component-quota-hard-pool-details-middle",
    "component-quota-hard-pool-details-bottom",
    "component-quota-hard-pool-closed",
    "component-quota-hard-details",
];

// 76x30 and the 24-row grids: top, middle and bottom Pool poses (no complete).
pub(super) const HARD_CASE_PARTIAL_CHECKPOINTS: [&str; 8] = [
    "component-quota-hard-browse",
    "component-quota-hard-beta-windows",
    "component-quota-hard-beta-extra",
    "component-quota-hard-pool-details",
    "component-quota-hard-pool-details-middle",
    "component-quota-hard-pool-details-bottom",
    "component-quota-hard-pool-closed",
    "component-quota-hard-details",
];

// 48x12 shows one body row per page at the top: the selected name's first row,
// then its continuation one page down in the same terminal.
// Identity is the composite of that prefix, the same-child continuation and the
// captured Details tag; no single 48x12 frame holds the whole name. Alpha's
// week line at PageDown 2, 5h at 3; Beta's week 2, stale/no-data 3, credits 6.
pub(super) const HARD_CASE_SHORT_CHECKPOINTS: [&str; 14] = [
    "component-quota-hard-browse-top",
    "component-quota-hard-alpha-name-continued",
    "component-quota-hard-alpha-week",
    "component-quota-hard-alpha-short",
    "component-quota-hard-beta-selected",
    "component-quota-hard-beta-name-continued",
    "component-quota-hard-beta-week",
    "component-quota-hard-beta-short",
    "component-quota-hard-beta-extra",
    "component-quota-hard-pool-details",
    "component-quota-hard-pool-details-middle",
    "component-quota-hard-pool-details-bottom",
    "component-quota-hard-pool-closed-top",
    "component-quota-hard-details",
];

pub(super) fn hard_case_checkpoints(
    columns: u16,
    rows: u16,
) -> TestResult<&'static [&'static str]> {
    match (columns, rows) {
        (48, 12) => Ok(&HARD_CASE_SHORT_CHECKPOINTS),
        (120, 36) => Ok(&HARD_CASE_CHECKPOINTS),
        (76, 30) | (48, 24) | (40, 24) | (32, 24) => Ok(&HARD_CASE_PARTIAL_CHECKPOINTS),
        _ => Err("hard-case viewport is not declared".into()),
    }
}

pub(super) fn validate_hard_case_checkpoints(
    columns: u16,
    rows: u16,
    checkpoints: &[&str],
) -> TestResult<()> {
    ensure(
        checkpoints == hard_case_checkpoints(columns, rows)?,
        "hard-case tour must keep every window, census and return scene in order",
    )
}

/// Body PageDowns from Beta's selected window lines to Beta's Extra group on
/// the tall grids, derived from the folded block height against each body and
/// confirmed by a declared calibration run.
fn hard_case_extra_page_downs(columns: u16, rows: u16) -> TestResult<usize> {
    match (columns, rows) {
        (120, 36) | (76, 30) => Ok(0),
        (48, 24) | (40, 24) | (32, 24) => Ok(1),
        _ => Err("hard-case tall viewport is not declared".into()),
    }
}

fn beta_selected() -> String {
    selected_account_readiness_expression(&[
        "Claude", "Research", "Account", "Beta", "Full", "Name",
    ])
}

fn beta_selected_prefix() -> String {
    selected_account_readiness_expression(&["Claude", "Research", "Account", "Beta", "Full"])
}

fn hard_case_pool_and_return(
    tape: &mut String,
    complete: bool,
    middle_downs: usize,
    closed: (&str, String),
) {
    tape.push_str("F5\nUp@1ms 256\n");
    checkpoint(
        tape,
        "component-quota-hard-pool-details",
        "Pool details.*Accounts: 2.*Usable: 1.*Reserve: 0.*Close",
    );
    if complete {
        checkpoint(
            tape,
            "component-quota-hard-pool-details-complete",
            "Pool details.*Accounts: 2.*Usable: 1.*Reserve: 0.*Blocked: 1.*Pool unknown: 0.*Excluded: 0.*Fresh: 0.*Stale: 1.*Data unknown: 1.*Clients: 1.*Credentials locked.*Close",
        );
    }
    // Middle rows: offset 0 on tall grids, four Downs in the 48x12 body.
    tape.push_str(&"Down\n".repeat(middle_downs));
    checkpoint(
        tape,
        "component-quota-hard-pool-details-middle",
        "Pool details.*Blocked: 1.*Pool unknown: 0.*Excluded: 0.*Fresh: 0.*Close",
    );
    tape.push_str("Down@1ms 256\n");
    checkpoint(
        tape,
        "component-quota-hard-pool-details-bottom",
        "Pool details.*Stale: 1.*Data unknown: 1.*Clients: 1.*Credentials locked.*Close",
    );
    tape.push_str("Escape\n");
    checkpoint(tape, closed.0, &format!("Quota status.*{}", closed.1));
    tape.push_str("F4\nUp@1ms 256\n");
    checkpoint(tape, "component-quota-hard-details", HARD_DETAILS_READY);
    tape.push_str("Escape\nWait+Screen /Quota status/\nType \"q\"\nWait+Line /^QUOTA_STORYBOARD_CHILD_EXIT=0$/\n");
}

fn build_short_hard_case_tape() -> String {
    let mut tape = component_tape_settings(48, 12);
    checkpoint(
        &mut tape,
        "component-quota-hard-browse-top",
        &format!(
            "Quota status.*Credentials locked.*{}",
            selected_account_readiness_expression(&[
                "東京", "OpenAI", "Research", "Account", "Alpha",
            ])
        ),
    );
    tape.push_str("PageDown\n");
    checkpoint(
        &mut tape,
        "component-quota-hard-alpha-name-continued",
        r"Quota status.*Long Name.*OpenAI · 1 client",
    );
    tape.push_str("PageDown\n");
    checkpoint(
        &mut tape,
        "component-quota-hard-alpha-week",
        "Quota status.*92% · week · 6d 16h",
    );
    tape.push_str("PageDown\n");
    checkpoint(
        &mut tape,
        "component-quota-hard-alpha-short",
        "Quota status.*83% · 5h · 2h 10m",
    );
    tape.push_str("PageUp\nPageUp\nPageUp\nDown\n");
    checkpoint(
        &mut tape,
        "component-quota-hard-beta-selected",
        &format!("Quota status.*{}", beta_selected_prefix()),
    );
    tape.push_str("PageDown\n");
    checkpoint(
        &mut tape,
        "component-quota-hard-beta-name-continued",
        r"Quota status.*Name.*Claude · 0 clients",
    );
    tape.push_str("PageDown\n");
    checkpoint(
        &mut tape,
        "component-quota-hard-beta-week",
        "Quota status.*75% · week · —",
    );
    tape.push_str("PageDown\n");
    checkpoint(
        &mut tape,
        "component-quota-hard-beta-short",
        "Quota status.*week stale.*No data · 5h · —",
    );
    tape.push_str("PageDown\nPageDown\nPageDown\n");
    checkpoint(
        &mut tape,
        "component-quota-hard-beta-extra",
        r"Quota status.*125\.5 credits.*Credits stale",
    );
    // Back to the selected-Beta pose before Pool details and its return.
    tape.push_str(&"PageUp\n".repeat(6));
    hard_case_pool_and_return(
        &mut tape,
        false,
        4,
        (
            "component-quota-hard-pool-closed-top",
            beta_selected_prefix(),
        ),
    );
    tape
}

pub(super) fn build_hard_case_component_tape(columns: u16, rows: u16) -> TestResult<String> {
    if (columns, rows) == (48, 12) {
        return Ok(build_short_hard_case_tape());
    }
    let extra_page_downs = hard_case_extra_page_downs(columns, rows)?;
    let mut tape = component_tape_settings(columns, rows);
    // Exact rendered window lines (quota_account_groups.rs render_window).
    checkpoint(
        &mut tape,
        "component-quota-hard-browse",
        r"Quota status.*Credentials locked.*92% · week · 6d 16h.*83% · 5h · 2h 10m",
    );
    tape.push_str("Down\n");
    tape.push_str(&format!(
        "Wait+Screen /(?s){}/\n",
        hard_beta_selected_name_readiness()
    ));
    checkpoint(
        &mut tape,
        "component-quota-hard-beta-windows",
        HARD_BETA_WINDOW_FACTS_READY,
    );
    // Same child, same selection; only Beta has these credit facts.
    tape.push_str(&"PageDown\n".repeat(extra_page_downs));
    checkpoint(
        &mut tape,
        "component-quota-hard-beta-extra",
        r"Quota status.*125\.5 credits.*Credits stale",
    );
    // Back to the windows pose, where Beta's selected name is visible again.
    tape.push_str(&"PageUp\n".repeat(extra_page_downs));
    hard_case_pool_and_return(
        &mut tape,
        (columns, rows) == (120, 36),
        0,
        ("component-quota-hard-pool-closed", beta_selected()),
    );
    Ok(tape)
}

#[cfg(test)]
#[path = "quota_component_tape_test.rs"]
mod tests;

#[cfg(test)]
#[path = "quota_component_observation_wait_test.rs"]
mod observation_wait_tests;
