# Codex CLI with Claude and Cursor ACP

Human entry: [Design documents](index.html).

| Question | Owning source | HTML |
| --- | --- | --- |
| What do you need? | [Requirements](requirements.md) | [Requirements](requirements.html) |
| What must happen? | [Specification](specification.md) | [Specification](specification.html) |
| How do components and provider traits fit? | [Program Design](program-design.md) | [Program Design](program-design.html) |
| How will each outcome be proved? | [Verification Design](verification-design.md) | [Verification Design](verification-design.html) |
| What changes, in what order, with which proof gates? | [Published plan source companion](plans/2026-10-30-app-server-acp-shared-verifier.source.md) | [Implementation Plan](implementation-plan.html) |

The plan source companion is generated from the authoring artifact named in its canonical record. Keep it byte-identical when refreshing the reader. It preserves the planning-time record; planning readiness is separate from implementation proof and PR readiness.

## Through-line: skills

1. [U1/U2](requirements.md#what-you-need)
2. [R1 listing](specification.md#r1--list-the-right-providers-skills-c1-v1), [R2 invocation](specification.md#r2--run-the-selected-skill-c2-v2)
3. [Provider metadata](program-design.md#the-metadata-gap-handled-at-the-provider-boundary), [invocation](program-design.md#invocation-and-commands)
4. [J1 and coverage](verification-design.md#target-and-lasting-journeys)

## Through-line: commands

1. [U3/U4](requirements.md#what-you-need)
2. [R3 operations](specification.md#r3--map-existing-operations-separately-c3-v3), [R4 preservation](specification.md#r4--preserve-conversations-c4-v4)
3. [Native/provider operations](program-design.md#invocation-and-commands), [state and failure](program-design.md#state-failure-and-trust)
4. [J2/J3 and coverage](verification-design.md#target-and-lasting-journeys)

## Reading path: provider injection

1. [U5](requirements.md#what-you-need)
2. [R5](specification.md#r5--inject-the-differences-keep-one-bridge-c5-v5)
3. [Provider routing and sockets](program-design.md#provider-routing-and-separate-sockets), [crates and trait](program-design.md#crates-and-injection), [constructor path](program-design.md#where-the-dependency-is-constructed-and-passed), [rationale](program-design.md#why-this-much-and-no-more)
4. [U5 proof](verification-design.md#u5--constructor-di-supplies-the-right-implementation)

## Reading path: implement the design

1. [Crates and injection](program-design.md#crates-and-injection)
2. [Canonical planning record](plans/2026-10-30-app-server-acp-shared-verifier.source.md#canonical-planning-result)
3. [Slices and gates](plans/2026-10-30-app-server-acp-shared-verifier.source.md#order-and-integration-gates)
4. [Obligation-to-proof map](plans/2026-10-30-app-server-acp-shared-verifier.source.md#obligation-to-proof-map)
