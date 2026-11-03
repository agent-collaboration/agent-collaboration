# Producer-owned staged person identity fixtures

Copied byte for byte from Switchboard commit `a75ce8c28d5346e9ef76814355f700eb877bb086`,
`packages/sign-in-core/contract-fixtures/person-identity/`. Authority:
`packages/sign-in-core/src/contracts/person-identity-contracts.ts`.

These are B3 structural schemas and examples, not minted tokens, real
introspection responses or authentication evidence. B3 is still in progress.
B4a owns the live producer cutover; the later of B4a/F2a owns the real-output check.

The Rust consumer validates these artifacts without becoming their schema owner.
Update them from a newly pinned producer commit rather than editing their JSON.

| File | SHA-256 |
| --- | --- |
| K5.schema.json | b976903b852e8b0dfba20d2789bf468301fcbfcb156b9666c1415424d6406db6 |
| K5.fixtures.json | 2bb041130cc739d193734caa0d51909cd9bdb22e1b64b1dc0f9905b46b8d1b50 |
| K6.schema.json | 507f8cd4b83f69977ed8729d7b437dac9684bb82a9ec8811296be4b9313c939a |
| K6.fixtures.json | 941233e5e6bf4821ca85cdf016ae84e2e80be60e80d26e1ca1cfcdf75c0cec64 |
