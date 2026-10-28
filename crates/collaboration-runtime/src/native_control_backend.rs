//! The Codex native control backend the session operations call: the endpoint it serves and
//! the generation gate that admits each call. No provider policy or native process ownership.
use collaboration_protocol::EndpointRef;

#[derive(Clone)]
pub struct NativeControlBackend {
    pub endpoint: EndpointRef,
    pub gate: crate::NativeGenerationGate,
    pub codex_home: std::path::PathBuf,
}
