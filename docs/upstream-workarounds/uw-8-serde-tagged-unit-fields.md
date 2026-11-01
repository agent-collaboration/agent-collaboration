# Internally tagged unit variants must reject extra fields

Serde and serde_derive 1.0.228 accept extra fields on an internally tagged unit
variant even when its enum declares `deny_unknown_fields`. For example,
`{"state":"accepted","unexpected":true}` decoded as `FirstInputState::Accepted`,
while its generated schema rejected the same object.

The pinned generator's `src/de/enum_internally.rs` sends a unit variant's remaining
content to `InternallyTaggedUnitVisitor`. A variant-level `deserialize_with`
instead receives that content directly. Our
[strict empty-field decoder](../../crates/collaboration-protocol/src/automation_custody.rs)
reads it as an empty struct with `deny_unknown_fields`. Its attributes cover the
18 unit variants in the six automation custody enums added by this change.
Valid Rust variants, serialized JSON and all six generated schemas stay unchanged.

The [permanent contract test](../../crates/collaboration-protocol/tests/automation_custody_contract.rs)
checks every affected unit variant: valid JSON roundtrips unchanged, and an extra
field fails both schema validation and decoding. Run it with:

```sh
cargo test --locked -p collaboration-protocol --test automation_custody_contract tagged_unit_outcomes_reject_unknown_fields_without_changing_valid_wire
```

To recreate the upstream behavior, remove the `deserialize_with` attributes from
these unit variants and run that same test. It fails on an object with an extra
field. The retained-result decoder matrix in
[machine_work_item_record_tests.rs](../../crates/collaboration-central-core/src/machine_work_item_record_tests.rs)
also rejects this corruption through the real stored-row decoder.

Recheck when Serde is upgraded. Remove the adapter only when the unchanged
contract test passes without its attributes, valid roundtrips and schema snapshots
remain unchanged, and the protocol suite and strict Clippy pass. Pre-existing
tagged enums outside these automation contracts are outside this repair's scope.
