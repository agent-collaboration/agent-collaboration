use vendor_protocol::codex::{CodexRequest, ModelListResponse};

fn main() {
    let _method = <ModelListResponse as CodexRequest>::METHOD;
}
