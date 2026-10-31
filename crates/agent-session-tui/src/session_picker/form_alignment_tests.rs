use super::*;

async fn dialog_frame(dialog: usize) -> String {
    let mut request = crate::session_picker::test_support::picker_request();
    request.new_session_args_display = "--model fixture-model".to_owned();
    let event = match dialog {
        0 => tests::ctrl_key('n'),
        1 => tests::control_shift_enter_key(),
        2 => TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(2))),
        _ => TerminalEvent::Key(KeyEvent::new(KeyEventKind::Press, KeyCode::F(4))),
    };
    tests::render_picker_capture_at(request, 120, 36, vec![event, tests::ctrl_key('d')]).await
}

fn assert_field_origins(frame: &str, pairs: &[(&str, &str)]) {
    let widest = pairs
        .iter()
        .map(|(label, _)| unicode_width::UnicodeWidthStr::width(*label))
        .max()
        .unwrap();
    for (label, value) in pairs {
        let line = frame
            .lines()
            .find(|line| line.contains(label) && line.contains(value))
            .unwrap_or_else(|| panic!("{label} / {value}: {frame}"));
        let label_origin = unicode_width::UnicodeWidthStr::width(line.split_once(label).unwrap().0);
        let value_origin = unicode_width::UnicodeWidthStr::width(line.split_once(value).unwrap().0);
        assert_eq!(
            value_origin,
            label_origin + widest + 4,
            "shared label width, gap, and reserved two-cell gutter: {frame}"
        );
    }
}

#[tokio::test]
async fn luna_422_new_session_fields_share_value_origin() {
    let frame = dialog_frame(0).await;
    assert_field_origins(
        &frame,
        &[
            ("Client", "Codex ▾"),
            ("Machine", "This machine ▾"),
            ("Directory", "/repo/project-a"),
            ("Arguments", "--model fixture-model"),
        ],
    );
}

#[tokio::test]
async fn luna_422_fork_fields_share_value_origin() {
    let frame = dialog_frame(1).await;
    assert_field_origins(
        &frame,
        &[
            ("Source", "This machine"),
            ("Directory", "/repo/project-a"),
            ("Destination", "This machine ▾"),
        ],
    );
}

#[tokio::test]
async fn luna_422_machine_field_reserves_shared_value_gutter() {
    let frame = dialog_frame(2).await;
    // Owner R14: inline choices become closed fields with a dropdown.
    assert_field_origins(&frame, &[("Machine", "This machine (current/default) ▾")]);
}

#[tokio::test]
async fn luna_422_session_details_share_value_origin() {
    let frame = dialog_frame(3).await;
    assert_field_origins(
        &frame,
        &[
            ("Session ID:", "thread-a"),
            ("Working directory:", "/repo/project-a"),
        ],
    );
}
