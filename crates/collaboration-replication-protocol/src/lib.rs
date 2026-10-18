#![forbid(unsafe_code)]

mod forwarded_post;
mod import_snapshot;
pub use import_snapshot::*;
mod piece_digest;
pub use forwarded_post::*;
mod machine_frame_decode;
mod rejected_tail;
mod replication_ids;
mod replication_piece;
mod replication_position;
pub use rejected_tail::{TailDigest, TailReportRow};
mod reserved_namespace;
mod schema_admission;
mod schema_fingerprint;
mod schema_sql;
mod session_frame;
pub use machine_frame_decode::{MachineFrameDecodeFailure, decode_machine_frame};
mod snapshot_page;
mod transaction_part;

pub use piece_digest::{DigestError, PieceDigest};
pub use schema_fingerprint::{
    SchemaEntry, SchemaObjectKind, SchemaObjectKindError, schema_fingerprint,
};
pub use snapshot_page::{
    ProjectCopyRead, SchemaIdentity, SchemaIdentityError, SchemaSupport, SnapshotCursor,
    SnapshotPage,
};
pub use transaction_part::{TransactionKey, TransactionPart, TransactionPartError};

pub use replication_ids::{
    BoardId, CommitId, Epoch, EpochError, ForwardId, FrameId, IdentityError, InstallationId,
    ProjectId, RequestId, SessionId, TransferId,
};
pub use replication_piece::{
    Piece, PieceContents, PieceError, ProjectSummary, ReplicatedTransaction, RowChange,
    RowChangeError, SqlValue, SqlValueError,
};
pub use replication_position::ReplicationPosition;
pub use reserved_namespace::reserved_namespace;
pub use session_frame::{
    ArchiveSeen, CentralFrame, CredentialRejection, FleetPayload, FleetPosition,
    FleetRequestEnvelope, HolderReleaseRefusal, LeaseState, LeaseStatus, MachineFrame,
    MalformedFrameDetail, MigrationState, MoveRefusalReason, MoveResult, Notice, NoticeKind,
    NoticePosition, NoticePositionError, ProjectSessionState, ReadCursor, ReadPage, ReadScope,
    ReleaseOutcome, ReleaseRefusal, SessionAnswer, SessionCloseReason, SessionRefusal,
    SessionRequest, TakeoverPreview, TakeoverRefusalReason, TakeoverResult,
};

#[cfg(test)]
mod test_fixtures;
#[cfg(test)]
mod tests;

pub use schema_admission::{
    SchemaAdmissionError, SchemaAdmissionReason, SchemaStatementForm, admit_schema_statement,
};
mod copy_availability;
pub use copy_availability::{BlockedSubject, BookkeepingKind, CopyUnavailableReason};
