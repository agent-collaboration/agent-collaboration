use central_storage::{CorruptReason, StoreFailure};
pub fn corrupt(column: &str) -> StoreFailure {
    StoreFailure::Corrupt {
        reason: CorruptReason::BadValue {
            column: column.to_owned(),
        },
    }
}
