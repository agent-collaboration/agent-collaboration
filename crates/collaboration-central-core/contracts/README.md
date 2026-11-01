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

K14 publishes `FindCentral`, `CentralLookup` and `CentralAddresses` from
`fleet_directory_contracts`. `CentralLookup::Found` carries only public
addresses; `CentralRecord` also keeps the private `directoryCalls` used for
initialization and device approval. These are separate types.

The directory's `HttpsUrl` and `ResourceUri` preserve bytes and enforce one
bounded lexical pattern, shared with their schemas. HTTP is permitted only for
loopback hosts; queries, fragments and user information are refused. Percent
escapes are retained, with exactly two hexadecimal digits after each `%`.
Ports may contain leading zeroes; their numeric value is 1–65535.

`VerifiedEmail` deliberately publishes its accepted input language rather than
only its canonical output. Its pattern includes the verbatim email expression
from the sign-in service's pinned Zod 4.4.3, with explicit ASCII and whole-input
guards, and its byte bound is checked before normalization. Rust accepts
uppercase ASCII and serializes lowercase; its `.reads.json` pins that behavior.
Consumers must not generalize either this rule or the whitespace-normalizing
types' canonical-only schema policy to the other. Verification provenance comes
from the provider or operator, and cannot be established by email syntax.

Reserved and approved enrollments remain internal to the authority. F1
defines no Rust `Principal` union. The Switchboard actor-union parsing and
cross-repository real-output checks remain with their consuming tickets.
