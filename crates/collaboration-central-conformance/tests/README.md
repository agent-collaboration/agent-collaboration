# collaboration-central-conformance test contracts

This package declares no integration or e2e test target. Its existing unit tests
remain colocated with their implementation and retain their Cargo/nextest identity.
Shared fixtures belong to their named test-support owners; this folder adds no
empty aggregate or executable journey. Package and platform-specific checks remain
in the workspace CI selection and the owning source modules.

The non-default `signin-test-support` feature provides shared real signers, JWKS
publication and signed person fixtures to both hosts' dev dependencies. Business
time comes from each caller's injected clock. This fixture module is excluded on
WASM and is not enabled by production host dependencies.
