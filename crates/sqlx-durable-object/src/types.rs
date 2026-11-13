//! `Encode`, `Decode` and `Type` for the Rust types Durable Object storage
//! can store.
//!
//! Durable Object storage is SQLite behind a JavaScript API, like D1, and maps
//! types exactly as `sqlx-cloudflare-d1` does -- the two drivers share one
//! definition. That mapping follows sqlx-sqlite wherever JavaScript does not
//! force a difference, so code moving from `Sqlite` to `Do` behaves the same.
//! Where it does force one, it fails loudly:
//!
//! | Rust | Stored as | Notes |
//! |---|---|---|
//! | `bool` | `INTEGER` 0/1 | any non-zero integer decodes as `true` |
//! | `i8`, `i16`, `i32`, `u8`, `u16`, `u32` | `INTEGER` | decoding a value that does not fit is an error |
//! | `i64` | `INTEGER` | only within ±(2^53 − 1), in both directions |
//! | `f32`, `f64` | `REAL` | also decode from `INTEGER` |
//! | `&str`, `String`, `Box<str>`, `Arc<str>`, `Cow<str>` | `TEXT` | |
//! | `&[u8]`, `Vec<u8>`, `Box<[u8]>`, `Arc<[u8]>` | `BLOB` | also decode from `TEXT`, as its bytes |
//!
//! `u64` is deliberately missing: there is nothing it could be stored as
//! without losing most of its range.
//!
//! # Integers and JavaScript numbers
//!
//! `sql.exec` takes and returns JavaScript numbers, never `BigInt`, and a
//! number is exact only up to 2^53 − 1. So binding an `i64` outside
//! ±(2^53 − 1) is an error, not a value rounded on the way in, and a stored
//! integer beyond that range comes back as a `REAL` that will not decode as
//! `i64`.
//!
//! For the same reason, a whole number stored as `REAL` -- `3.0` -- arrives
//! indistinguishable from the integer `3`, and is typed `INTEGER`. That is why
//! the float types decode from `INTEGER` too.

sqlx_durable_object_core::impl_types!(crate::Do);

// Through `Row::try_get`, as a consumer decodes: that runs `Type::compatible`
// before `Decode`, and the two have to agree.
#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
