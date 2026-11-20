use vendor_protocol::codex::{
    CodexRequest, ThreadReadParams, ThreadReadResponse, ThreadResumeParams, ThreadResumeResponse,
};

fn read<Request: CodexRequest<Response = ThreadReadResponse>>(_: Option<Request>) {}
fn resume<Request: CodexRequest<Response = ThreadResumeResponse>>(_: Option<Request>) {}

fn main() {
    read(None::<ThreadResumeParams>);
    resume(None::<ThreadReadParams>);
    let _: Option<<ThreadResumeParams as CodexRequest>::Response> = None::<ThreadReadResponse>;
    let _: Option<<ThreadReadParams as CodexRequest>::Response> = None::<ThreadResumeResponse>;
}
