//! The organization's workspace directory.
#![forbid(unsafe_code)]

mod workspace_directory_rows;

pub use workspace_directory_rows::{DirectoryWorkspace, insert, read_by_id, read_by_name};
