# Application fixture proof

This unpublished library owns the shared real application/API composition, registry,
clock, accepted snapshot and frozen cancellation arrangers. Its existing service and
MCP test-support APIs stay behind their non-default features. Consumers supply their
private temporary resources and retain their original case modules and assertions.

The original fixture sources are case-free. Shared application assembly lives here;
NativeMailOwner's task lifetime stays in Service's mail runtime behind its existing
test-support gate. The Service's private unit factory uses its own builder types;
both factories retain the builder's physical-owner validation and use the same
runtime-task owner. The original Service, MCP, client, Host and agent-collaboration
suites exercise these fixtures through dev-dependencies.
No integration or e2e target is declared here, and no empty aggregate is added.

Use the owning packages’ `tests/README.md` files and the CI workflow for their package
selections and complete fixture setup. Agent E2E retains its separate all-feature
selection, prebuilt Host nomination, private runtime parent and one test thread.
