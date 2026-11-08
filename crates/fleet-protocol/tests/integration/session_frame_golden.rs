#![allow(clippy::unwrap_used)]
use fleet_protocol::{ClientFrame, ServerFrame};

#[test]
fn released_single_frames_keep_their_exact_bytes() {
    for wire in [
        include_bytes!("../fixtures/session-wire/hello.json").as_slice(),
        include_bytes!("../fixtures/session-wire/client-request.json").as_slice(),
        include_bytes!("../fixtures/session-wire/client-answer.json").as_slice(),
    ] {
        let frame: ClientFrame = serde_json::from_slice(wire).unwrap();
        assert_eq!(serde_json::to_vec(&frame).unwrap(), wire);
    }
    for wire in [
        include_bytes!("../fixtures/session-wire/welcome.json").as_slice(),
        include_bytes!("../fixtures/session-wire/server-request.json").as_slice(),
        include_bytes!("../fixtures/session-wire/server-answer.json").as_slice(),
        include_bytes!("../fixtures/session-wire/notice.json").as_slice(),
        include_bytes!("../fixtures/session-wire/closed.json").as_slice(),
    ] {
        let frame: ServerFrame = serde_json::from_slice(wire).unwrap();
        assert_eq!(serde_json::to_vec(&frame).unwrap(), wire);
    }
}
