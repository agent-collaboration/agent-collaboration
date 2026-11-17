# collaboration-central-conformance test contracts

This package declares no integration or e2e test target and holds no tests of
its own. It is shared test code: both central hosts import it from their dev
dependencies.

`ConformanceResult` is always available. The non-default `signin-test-support`
feature provides shared real signers, JWKS publication and signed person
fixtures (`signin_fixtures`) to both hosts' dev dependencies. Business time
comes from each caller's injected clock. This fixture module is excluded on
WASM and is not enabled by production host dependencies.
