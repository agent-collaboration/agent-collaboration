# Central identity contracts

These schemas derive from `collaboration_central_core::identity_contract`.
Each file is keyed by type name, including shapes with identical fields such
as `DecideMachineToken` and `DisableMachineClient`.

- `<RustType>.fixtures.json`: canonical serializer output.
- `<RustType>.negatives.json`: rejected reads.
- `<RustType>.reads.json`, where present: accepted `input` and expected
  `canonical` serialization.

The Verifier runs the same committed corpora natively and under WebAssembly.

```sh
UPDATE_FLEET_IDENTITY_CONTRACTS=1 cargo test -p collaboration-central-core
cargo test -p collaboration-central-core
```

Permanent tests cover issuer decisions, observable enrollments, presence,
organization commands and all four admin lookups with their closed results.
Unknown or foreign lookup subjects carry no result payload. Lookups never
answer `secondFactorRequired`; their approval and start times use Unix
milliseconds.

Bodies refuse unknown fields. Machine token extension claims accept the
issuer's registered JWT claims alongside them. Writers omit absent optional
fields, while readers accept omission or null. Every schema validator must
enforce `x-maxUtf8Bytes`; Rust tests register the keyword, and Zod consumers
add a `TextEncoder` byte refinement.
Schemas validate canonical values; Rust additionally accepts and normalizes
surrounding whitespace on read. A padded label may exceed the raw schema bound
while its trimmed value is within Rust's bound; full read equivalence is not claimed.

Reserved and approved enrollments remain internal to the authority. F1
defines no Rust `Principal` union. The Switchboard actor-union parsing and
cross-repository real-output checks remain with their consuming tickets.
