# Adopted S6 fixture outcome readback for U4

**Adopted for the Lead's S6 brief** by push
`01a18ce0-eea3-7eb3-b6bb-18b1df4f05d5`; no producer or product change is made here.
The Implementer owns the producer, feature-gated test support only and absent
from normal/release builds. This remains unobserved until its real bytes reach
the real reader with the same-ID controls below.
The current terminal marker prints only `selection.session_id()`. That discards
which `SessionPickerIdentity` variant and endpoint owns the opaque ID. The fixture
already gives Codex Alpha, Claude Gamma and Cursor Echo the same session ID.
A wrong-source selection can therefore print the expected UUID.

## Smallest readback

Have the existing test-only entry
`crates/agent-session-tui/src/session_picker/picker_entrypoint.rs::run_sessions_picker_test_harness`
write one durable `picker-outcome.json` in the already owned harness receipt
location, **after** the real render loop returns. Emit from the returned
`SessionsPickerOutcome`, never from the current browse row or label. Keep the
human marker derived from that same outcome; the U4 checker never falls back to it.

The fixture-local DTO has one action tag and the actual typed identity:

```ts
type PickerOutcomeReadback =
  | { kind: 'cancelled' }
  | { kind: 'resume'; identity: PickerIdentityReadback }
  | { kind: 'fork'; identity: PickerIdentityReadback };

type PickerIdentityReadback =
  | { kind: 'localCodex'; sessionId: string }
  | { kind: 'hostedCodex'; target: SessionRef }
  | { kind: 'hostedProvider'; target: SessionRef }
  | { kind: 'nativeClaude'; target: SessionRef };
```

`SessionRef` is the existing serialized protocol type, not a second handwritten
schema: `{endpoint:{serviceId,endpointId},sessionId}`. Its service identity is a
UUID; endpoint and session IDs are their existing validated opaque types. Reuse
its Serde implementation. Session IDs are nonempty, not necessarily UUIDs.

The producer DTO belongs adjacent to the feature-gated entry. Match the existing
`SessionPickerIdentity::{LocalCodex,HostedCodex,HostedProvider,NativeClaude}`
exhaustively, copying from `SessionActionSelection.identity`. Map ResumeSession
and ForkSession to their corresponding tags; `None` is cancelled. Other picker
outcome variants must be explicitly reported as outside this U4 fixture case,
never coerced into Resume/Fork or a fabricated target.

The future reader's home is `scripts/verification/tui/source-qualified-outcome.ts`,
with a strict schema and whole-value comparison to an independently declared
fixture target. This pre-stage reserves that reader home; implementation and
binding await S6's actual producer. A wrong kind, missing target, unknown field,
different service/endpoint/identity variant or same-UUID foreign target rejects.

No title, display machine, provider label, model, credential reference, address or
full RouterConnectionProfile is needed for this U4 identity claim. Configured
machine dispatch policy and native/provider job execution remain separate F
proof. If S6 also needs route-context proof, add only the validated connection
name as a separately agreed projection of `PickerSourceContext`; never serialize
the full profile to get it accidentally.

## Proof the reader must earn

- Select Alpha, dispatch Resume, require LocalCodex plus its exact opaque ID.
- Select Gamma/Echo with the same ID, require HostedProvider and the full exact
  target; a wrong family/endpoint with that same ID must fail.
- Retain wrong-service, wrong-endpoint, wrong-kind, missing-target and extra-field
  controls. Also check Fork separately and ordinary Enter → Details → Cancelled.
- The real producer's serialized DTO must pass the real reader. A hand-built JSON
  fixture qualifies only reader sensitivity, not picker wiring or effect identity.
- Keep source/build/byte bindings outside this DTO in the existing capture receipt.
  Keep New's accepted `{callCount,request}` preparation receipt separate: it proves
  dispatch cardinality/arguments; this DTO proves the returned selected identity.

Source anchors: `agent-session-inventory/src/session_commands/session_action_selection.rs`
(`identity` is frozen from the captured record; `session_id()` drops qualification),
`session_catalog_records.rs::SessionPickerIdentity`, and
`collaboration-protocol/src/endpoint_identity.rs::SessionRef`.
