# UW-5: worker0.8.7 narrows database bytes on wasm32

**Status:** in place, 2026-10-16
**SDKs:** worker0.8.7 and worker-sys0.8.7

## Problem and evidence

`worker0.8.7/src/sql.rs:193` implements `SqlStorage::database_size()` by
casting the JavaScript getter's `f64` to `usize`. Our Worker target is
`wasm32-unknown-unknown`, where `usize` is 32 bits. A count of 4,294,967,296
bytes therefore becomes 4,294,967,295. The Health response must retain the
actual byte count.

The pinned sources expose a safe route to the same getter without that
narrowing: `Storage::as_raw()` in `worker/src/durable.rs:670`, followed by
`DurableObjectStorage::sql()` and the raw `SqlStorage::database_size()` in
`worker-sys/src/types/durable_object/sql_storage.rs:12`. The last method
returns `f64` directly from JavaScript's `databaseSize` property.

## Workaround

[storage_measurement.rs](../../crates/collaboration-central-cloudflare/src/storage_measurement.rs)
uses that existing raw getter. It validates a finite, non-negative integer
no greater than JavaScript's maximum safe integer, then converts to `u64`.
It adds no dependency, unsafe code, core port, caller-supplied measurement
or SQL sentinel. The target copy shell supplies the measurement before
the core Health step.

Native Health separately measures its own main database file plus WAL,
excluding SHM. That physical accounting is not claimed equivalent to
Cloudflare's `databaseSize` metric.

## Recreate and verify

From the repository root on Apple Silicon:

```sh
cargo test --locked \
  --manifest-path crates/collaboration-central-cloudflare/Cargo.toml \
  --target aarch64-apple-darwin --lib storage_measurement
```

The permanent decoder cases cover zero, both sides of the 32-bit boundary,
5 GiB and the maximum safe integer. They reject negative, fractional,
non-finite and unsafe-integer values. Deliberately changing the positive
conversion to narrow through `u32` makes the boundary assertion fail:
4,294,967,295 instead of 4,294,967,296. Restore the original conversion
before subsequent checks.

These are pure conversion checks. They do not allocate a multi-gigabyte
Durable Object or constitute deployed Health proof. The compiled Worker
Health journey must separately check that the actual target getter reaches
the response.

## Removal check

On an SDK upgrade, inspect the public getter's return type and conversion
on wasm32. Keep this workaround while it narrows through `usize` or
otherwise loses safe integer byte counts. Once the SDK offers a lossless
public getter, use it and retain the boundary/refusal cases, wasm checks
and real compiled Worker Health journey before removing the raw access.
