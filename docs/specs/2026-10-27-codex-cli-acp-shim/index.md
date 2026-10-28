# Codex CLI through ACP: design and plan

Human entries: [Design](index.html), [Implementation plan](plan.html).

- [Requirements](requirements.md): authorized needs and limits.
- [Specification](specification.md): observable behavior and provider capabilities.
- [Program Design](program-design.md): owners, contracts and provider injection.
- [Verification Design](verification-design.md): planned target observations and their limits.
- [Implementation plan](plans/2026-10-28-codex-cli-acp-shim.md): execution order and proof-bearing slices; plan-only.

## Through-line: skills

1. [Developer needs](requirements.md#authorized-outcome-and-boundary)
2. [Conversation binding](specification.md#c1-which-conversation-answers-a-cwd-only-skill-request)
3. [Selected input](specification.md#c2-exact-selected-input-semantics)
4. [Native listing and refresh](program-design.md#connection-binding-first-list-and-refresh)
5. [Claude integration](program-design.md#claude-provider-integration-using-public-apis)
6. [Planned observations](verification-design.md#declared-target-and-shared-observations)

## Through-line: commands

1. [Developer needs](requirements.md#authorized-outcome-and-boundary)
2. [Existing command operations](specification.md#c3-existing-command-operations)
3. [Native mappings](program-design.md#native-contracts-mapped-separately)
4. [Calls and failures](program-design.md#calls-state-and-failure)
5. [Planned observations](verification-design.md#declared-target-and-shared-observations)

## Through-line: provider injection

1. [Maintainer needs](requirements.md#authorized-outcome-and-boundary)
2. [Obligations](specification.md#contracts-and-obligations)
3. [Crate responsibilities](program-design.md#composition-and-crate-hygiene)
4. [Consumer-owned interface](program-design.md#small-consumer-owned-interface)
5. [Planned observations](verification-design.md#declared-target-and-shared-observations)

## Reading path: implement the reviewed design

1. [Component ownership](program-design.md#composition-and-crate-hygiene)
2. [Plan scope and authority](plans/2026-10-28-codex-cli-acp-shim.md#scope-and-authority)
3. [Slices](plans/2026-10-28-codex-cli-acp-shim.md#execution-slices)
4. [Proof mapping](plans/2026-10-28-codex-cli-acp-shim.md#obligation-to-proof-map)
