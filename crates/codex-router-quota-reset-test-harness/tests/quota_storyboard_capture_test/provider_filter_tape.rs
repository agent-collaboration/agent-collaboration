//! VHS inputs and literal compact scene contract; owned lifecycle stays separate.
use super::{CapturedTuiSurface, FONT_FAMILY, TestResult, ensure};
#[path = "provider_filter_details.rs"]
mod provider_filter_details;
use provider_filter_details::{compact_session_details_poses, expected_session_checkpoints};

// Collapsed FormField values can occupy the next terminal row. Only padding
// and the painted frame may separate a label from its exact value; the bound
// also accommodates VHS's retained 2400-cell right-padded screen text.
const COLLAPSED_FILTER_VALUE_GAP: &str = r"[ \t\r\n│┃]{1,4096}";

fn quota_openai_filter_ready() -> String {
    format!(r"Account filters.*Account provider{COLLAPSED_FILTER_VALUE_GAP}OpenAI\b")
}

pub(super) fn validate_compact_checkpoints(
    surface: CapturedTuiSurface,
    columns: u16,
    rows: u16,
    checkpoints: &[&str],
) -> TestResult<()> {
    let complete = match surface {
        CapturedTuiSurface::Sessions => {
            checkpoints == expected_session_checkpoints(columns, rows).as_slice()
        }
        CapturedTuiSurface::Quota => checkpoints == compact_quota_checkpoints(rows).as_slice(),
    };
    ensure(
        complete,
        "compact capture must contain every declared menu checkpoint",
    )
}

// 2026-10-23 C6: F5 Pool details holds the census. Top, middle and bottom
// poses cover all ten rows at every grid; the whole body fits only at 36 rows.
pub(super) fn compact_quota_checkpoints(rows: u16) -> Vec<&'static str> {
    let mut checkpoints = vec!["compact-quota-all", "compact-quota-pool-details"];
    if rows >= 36 {
        checkpoints.push("compact-quota-pool-details-complete");
    }
    checkpoints.extend([
        "compact-quota-pool-details-middle",
        "compact-quota-pool-details-bottom",
        "compact-quota-pool-closed",
        "compact-quota-filters-block-pool",
        "compact-quota-provider-no-match",
        "compact-quota-chooser",
        "compact-quota-help",
        "compact-quota-disabled-choice",
        "compact-quota-hidden-choice",
        "compact-quota-claude",
        "compact-quota-cancelled",
        "compact-quota-details",
    ]);
    checkpoints
}

pub(super) fn build_filter_tape(
    surface: CapturedTuiSurface,
    columns: u16,
    rows: u16,
    empty: bool,
) -> (String, Vec<&'static str>) {
    let mut tape = format!(
        "Set Shell bash\nSet Columns {columns}\nSet Rows {rows}\nSet FontFamily {FONT_FAMILY}\nSet FontSize 16\nSet Padding 0\nSet CursorBlink false\nSet WaitTimeout 8s\nOutput screens.txt\n"
    );
    let mut checkpoints = Vec::new();
    match surface {
        CapturedTuiSurface::Sessions => {
            checkpoint(&mut tape, &mut checkpoints, "sessions-all", "6/6");
            tape.push_str("Ctrl+F\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-chooser",
                "Draft: All providers",
            );
            tape.push_str("Down\nEnter\n");
            checkpoint(&mut tape, &mut checkpoints, "sessions-codex", "2/6.*Codex");
            tape.push_str("F4\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-details",
                "Session details.*Codex Alpha",
            );
            tape.push_str("Escape\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-details-closed",
                "2/6.*Codex",
            );
            tape.push_str("Down\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-codex-bottom",
                "Codex Beta",
            );
            tape.push_str("Ctrl+F\nDown\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-claude",
                "2/6.*Claude Code",
            );
            tape.push_str("Down\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-claude-bottom",
                "Claude Delta",
            );
            tape.push_str("Ctrl+F\nDown\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-cursor",
                "1/6.*Cursor",
            );
            tape.push_str("Ctrl+F\nUp\nUp\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-draft",
                "Draft: Codex",
            );
            tape.push_str("Escape\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-cancelled",
                "1/6.*Cursor",
            );
            tape.push_str("Up\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-new-choice",
                "New Codex session",
            );
            tape.push_str("Type \"zznomatchzz\"\n");
            checkpoint(&mut tape, &mut checkpoints, "sessions-empty", "0/6");
            tape.push_str("Escape\nCtrl+F\nUp\nUp\nUp\nEnter\n");
            checkpoint(&mut tape, &mut checkpoints, "sessions-restored", "6/6");
            // Complementary final page demonstrates rows below the first viewport.
            tape.push_str("Down\nDown\nDown\nDown\nDown\nDown\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "sessions-all-bottom",
                "Claude-lookalike Zeta",
            );
            tape.push_str("Ctrl+D\n");
        }
        CapturedTuiSurface::Quota => {
            if empty {
                tape.push_str(
                    "Wait+Screen /4\\/4 accounts/\nCtrl+F\nDown\nDown\nTab\nSpace\nEnter\n",
                );
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-empty",
                    "No accounts match these filters",
                );
            } else {
                checkpoint(&mut tape, &mut checkpoints, "quota-all", "4/4 accounts");
                tape.push_str("Up\nUp\nUp\n");
                checkpoint(&mut tape, &mut checkpoints, "quota-all-top", "Claude Delta");
                tape.push_str("Down\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-all-next",
                    "Claude Gamma",
                );
                tape.push_str("Ctrl+F\n");
                checkpoint(&mut tape, &mut checkpoints, "quota-chooser", "Draft: All");
                tape.push_str("Down\nEnter\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-openai",
                    "OpenAI.*2/4 accounts",
                );
                tape.push_str("Down\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-openai-bottom",
                    "OpenAI Beta",
                );
                tape.push_str("Ctrl+F\nDown\nEnter\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-claude",
                    "Claude.*2/4 accounts",
                );
                tape.push_str("Down\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-claude-bottom",
                    "Claude Gamma",
                );
                tape.push_str("Ctrl+F\nTab\nSpace\nEnter\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-hidden",
                    "Claude.*Disabled: Hidden.*1/4 accounts",
                );
                tape.push_str("F4\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-details",
                    "Account details",
                );
                tape.push_str("Escape\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-details-closed",
                    "Claude.*Disabled: Hidden.*1/4 accounts",
                );
                tape.push_str("Ctrl+F\nUp\n");
                checkpoint(&mut tape, &mut checkpoints, "quota-draft", "Draft: OpenAI");
                tape.push_str("Escape\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-cancelled",
                    "Claude.*Disabled: Hidden.*1/4 accounts",
                );
                tape.push_str("Ctrl+F\nUp\nUp\nTab\nSpace\nEnter\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-restored",
                    "All.*Disabled: Shown.*4/4 accounts",
                );
                tape.push_str("Down\nDown\nDown\n");
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "quota-all-bottom",
                    "OpenAI Beta",
                );
            }
            tape.push_str("Type \"q\"\n");
        }
    }
    tape.push_str("Wait+Screen /QUOTA_STORYBOARD_CHILD_EXIT=0/\n");
    (tape, checkpoints)
}

fn checkpoint(
    tape: &mut String,
    checkpoints: &mut Vec<&'static str>,
    name: &'static str,
    expected: &str,
) {
    let expression = expected.replace('/', "\\/");
    tape.push_str(&format!(
        "Wait+Screen /(?s){expression}/\nSleep 250ms\nScreenshot {name}.png\nSleep 250ms\n"
    ));
    checkpoints.push(name);
}

pub(super) fn build_compact_filter_tape(
    surface: CapturedTuiSurface,
    columns: u16,
    rows: u16,
) -> (String, Vec<&'static str>) {
    let mut tape = format!(
        "Set Shell bash\nSet Columns {columns}\nSet Rows {rows}\nSet FontFamily {FONT_FAMILY}\nSet FontSize 16\nSet Padding 0\nSet CursorBlink false\nSet WaitTimeout 8s\nOutput screens.txt\n"
    );
    let mut checkpoints = Vec::new();
    match surface {
        CapturedTuiSurface::Sessions => {
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-all",
                "6/6.*Codex Alpha",
            );
            tape.push_str("F4\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-codex-details",
                "Session details.*Codex Alpha",
            );
            // Details starts on Resume; backwards focus reaches Fork. Forward
            // focus wraps to Close and would never reach this next scene.
            tape.push_str("Shift+Tab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-fork",
                "Fork session.*Cancel",
            );
            tape.push_str("Tab\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-fork-choice",
                "Fork session.*Destination[:]?.*Cancel.*Select",
            );
            tape.push_str("Escape\nEscape\nWait+Screen /(?s)6\\/6.*Codex Alpha/\nCtrl+N\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-new",
                "New session.*Cancel.*Start",
            );
            tape.push_str("Shift+Tab\nShift+Tab\nShift+Tab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-new-machine",
                "New session.*Machine[:]?.*Cancel.*Select",
            );
            tape.push_str("Type \"zznomatchzz\"\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-new-no-match",
                r"New session.*Machine[:]?.*Search:[ \t\r\n│┃┌┐]{1,128}zznomatchzz▏.*No matching[ \t\r\n│┃┌┐]{1,128}choices.*Cancel.*Select",
            );
            tape.push_str("Escape\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-new-returned",
                "New session.*Cancel.*Start",
            );
            tape.push_str("Escape\nWait+Screen /(?s)6\\/6.*Codex Alpha/\nCtrl+G\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-machine-sources",
                "Choose machine.*Machine[:]?.*Cancel.*Select",
            );
            tape.push_str("Escape\nCtrl+F\nEnter\nType \"zznomatchzz\"\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-client-no-match",
                r"Session filters.*Client[:]?.*Search:[ \t\r\n│┃┌┐]{1,128}zznomatchzz▏.*No matching[ \t\r\n│┃┌┐]{1,128}choices.*Cancel.*Select",
            );
            tape.push_str("Escape\nEnter\nDown\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-chooser",
                r"Session filters.*Client[ \t\r\n│┃┌┐]{1,128}Codex.*Cancel.*Apply",
            );
            tape.push_str("F1\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-help",
                "Sessions Help.*Move field focus",
            );
            tape.push_str("Escape\nWait+Screen /(?s)Session filters.*Client.*Codex.*Cancel.*Apply/\nEnter\nDown\nEnter\nTab\nTab\nTab\nTab\nTab\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-claude",
                "2/6.*Claude Gamma",
            );
            tape.push_str("Ctrl+F\nEnter\nDown\nEnter\nWait+Screen /(?s)Session filters.*Client.*Cursor.*Cancel.*Apply/\nEscape\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-cancelled",
                "2/6.*Claude Gamma",
            );
            tape.push_str("F4\n");
            let mut previous = 0;
            for &(name, down) in compact_session_details_poses(columns, rows) {
                for _ in previous..down {
                    tape.push_str("Down\n");
                }
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    name,
                    "Session details.*Claude Gamma",
                );
                previous = down;
            }
            tape.push_str("Escape\nWait+Screen /(?s)2\\/6.*Claude Gamma/\nCtrl+F\nTab\nEnter\nType \"Checkout\"\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-folder-choice",
                "Session filters.*Folder[:]?.*Checkout.*Cancel.*Select",
            );
            tape.push_str("Escape\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-model-provider-choice",
                "Session filters.*Model provider[:]?.*Any model provider.*Cancel.*Select",
            );
            tape.push_str("Escape\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-source-choice",
                "Session filters.*Source[:]?.*All sources.*Cancel.*Select",
            );
            tape.push_str("Escape\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-runtime-choice",
                "Session filters.*Runtime[:]?.*Active.*Cancel.*Select",
            );
            tape.push_str("Escape\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-sessions-sort-choice",
                "Session filters.*Sort[:]?.*Updated.*Cancel.*Select",
            );
            tape.push_str("Escape\nEscape\nWait+Screen /(?s)2\\/6.*Claude Gamma/\nCtrl+D\n");
        }
        CapturedTuiSurface::Quota => {
            tape.push_str(
                "Wait+Screen /(?s)All.*Disabled: Shown.*4\\/4( accounts| ▾)/\nUp\nUp\nUp\n",
            );
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-all",
                "All.*Disabled: Shown.*4/4( accounts| ▾).*Claude Delta",
            );
            // Exact rendered census rows (quota_pool_details.rs "Label: N").
            tape.push_str("F5\nUp@1ms 256\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-pool-details",
                "Pool details.*Accounts: 4.*Usable: 0.*Reserve: 0.*Close",
            );
            if rows >= 36 {
                checkpoint(
                    &mut tape,
                    &mut checkpoints,
                    "compact-quota-pool-details-complete",
                    "Pool details.*Accounts: 4.*Usable: 0.*Reserve: 0.*Blocked: 0.*Pool unknown: 1.*Excluded: 3.*Fresh: 0.*Stale: 0.*Data unknown: 4.*Clients: 0.*Close",
                );
            }
            // The middle rows are visible at offset 0 except in the 48x12 body's
            // four rows, where three Downs show rows 3-6 (mounted frames).
            tape.push_str(&"Down\n".repeat(if rows == 12 { 3 } else { 0 }));
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-pool-details-middle",
                "Pool details.*Blocked: 0.*Pool unknown: 1.*Excluded: 3.*Close",
            );
            tape.push_str("Down@1ms 256\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-pool-details-bottom",
                "Pool details.*Fresh: 0.*Stale: 0.*Data unknown: 4.*Clients: 0.*Close",
            );
            tape.push_str("Escape\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-pool-closed",
                "All.*Disabled: Shown.*4/4( accounts| ▾).*Claude Delta",
            );
            // An obscuring modal owns input: F5 must not open Pool details.
            tape.push_str("Ctrl+F\nWait+Screen /(?s)Account filters.*Account provider/\nF5\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-filters-block-pool",
                "Account filters.*Account provider",
            );
            tape.push_str(
                "Escape\nWait+Screen /(?s)All.*Disabled: Shown.*4\\/4( accounts| ▾).*Claude Delta/\n",
            );
            tape.push_str("Ctrl+F\nEnter\nType \"zznomatchzz\"\n");
            // C7: the terminal text carries the exact end caret after the query.
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-provider-no-match",
                r"Account filters.*Account provider[:]?.*Search:[ \t\r\n│┃┌┐]{1,128}zznomatchzz▏.*No matching[ \t\r\n│┃┌┐]{1,128}choices",
            );
            tape.push_str("Escape\nEnter\nDown\nEnter\nDown\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-chooser",
                &format!(
                    r"{}.*Disabled accounts{COLLAPSED_FILTER_VALUE_GAP}Shown\b",
                    quota_openai_filter_ready()
                ),
            );
            tape.push_str("Up\nF1\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-help",
                "Quota Help.*Move field",
            );
            tape.push_str(&format!(
                "Escape\nWait+Screen /(?s){}/\nEnter\nDown\nEnter\nTab\nEnter\n",
                quota_openai_filter_ready()
            ));
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-disabled-choice",
                "Account filters.*Disabled accounts[:]?.*[›❯>] Shown",
            );
            tape.push_str("Down\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-hidden-choice",
                "Account filters.*Shown.*[›❯>] Hidden",
            );
            tape.push_str("Enter\nTab\nEnter\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-claude",
                "Claude.*Disabled: Hidden.*1/4( accounts| ▾).*Claude Gamma",
            );
            tape.push_str(&format!(
                "Ctrl+F\nEnter\nUp\nEnter\nWait+Screen /(?s){}/\nEscape\n",
                quota_openai_filter_ready()
            ));
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-cancelled",
                "Claude.*Disabled: Hidden.*1/4( accounts| ▾).*Claude Gamma",
            );
            tape.push_str("F4\n");
            checkpoint(
                &mut tape,
                &mut checkpoints,
                "compact-quota-details",
                "Account details.*Claude Gamma",
            );
            tape.push_str("Escape\nWait+Screen /(?s)Claude.*Disabled: Hidden.*1\\/4( accounts| ▾).*Claude Gamma/\nType \"q\"\n");
        }
    }
    tape.push_str("Wait+Screen /QUOTA_STORYBOARD_CHILD_EXIT=0/\n");
    (tape, checkpoints)
}

#[cfg(test)]
#[path = "provider_filter_tape_test.rs"]
mod compact_capture_contract_tests;
