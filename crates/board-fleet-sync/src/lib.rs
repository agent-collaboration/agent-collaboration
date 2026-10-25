//! Board adapters for the generic fleet synchronization library.

mod board_capture_bookkeeping;
mod board_lease_policy;
mod board_piece_annotation;
mod board_project_schema;
mod board_session_inputs;
mod lease_authority;
mod machine_notice_progress_store;
mod machine_session_store;
mod reqwest_session_transport;
mod session_credentials;

pub use board_capture_bookkeeping::BoardCaptureBookkeeping;
pub use board_lease_policy::BOARD_LEASE_POLICY;
pub use board_piece_annotation::BoardPieceAnnotation;
pub use board_project_schema::{BoardProjectSchema, BoardProjectSchemaError};
pub use board_session_inputs::BoardSessionInputs;
pub use lease_authority::LeaseAuthority;
pub use machine_notice_progress_store::MachineNoticeProgressStore;
pub use machine_session_store::MachineSessionStore;
pub use reqwest_session_transport::{ReqwestSessionConnection, ReqwestSessionTransport};
pub use session_credentials::AccessServiceToken;
