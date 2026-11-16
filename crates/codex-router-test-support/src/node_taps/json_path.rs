//! Reading a JSON value at a path of object keys without indexing (the workspace denies
//! `clippy::indexing_slicing`): a missing key, or a step into a non-object, reads as `null`,
//! exactly as `serde_json`'s `Index` does for reads.

use serde_json::Value;

static NULL: Value = Value::Null;

/// The value at `path` inside `value`, or `null` when any step is missing.
#[must_use]
pub fn at<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    path.iter()
        .try_fold(value, |current, key| current.get(*key))
        .unwrap_or(&NULL)
}
