# Organization link seam: keeping new call modules out of core's cycle

`collaboration-central-core` has one large module cycle. The module-cycle
ratchet (`architecture/module-cycle-baseline.json`) refuses any new edge
inside or entering it. Older Fleet call modules (`fleet_metadata_calls`,
`fleet_read_calls`, …) entered that cycle the usual way: they `impl` handlers on
`ObjectRuntime` and use the call envelope, caller context, step driver and
`PlatformIo` directly. A new call module written that way fails the ratchet.

## What the seam changes

The dependency direction. The new module decides through a narrow trait it
defines itself, and an adapter in an existing owner implements that trait. The
new module then depends on nothing in the cycle; the only edge between the two
runs from the adapter's owner, which is already in the cycle, to the new module,
which leaves it.

The adapter's placement inside an existing owner does not hide edges. The
ratchet folds a child module into its parent, so the adapter's uses of the
runtime count as the parent's, and those edges already exist. The new module
passes because it no longer reaches into the cycle, not because of where its
adapter sits.

## The shape, per owner

Each module gets its own port, at its own owner. They are not merged into one
Fleet or platform façade, which would couple the owners again.

**Fleet handlers** (`organization_provisioning_calls`, `organization_admin_calls`):
- The decision module holds the `CallTarget` types and answers, and the decisions
  as free functions over a narrow trait named for what they need:
  `OrganizationLinkFleet` (organization, stamp authority, clock and IDs,
  observation, one method per store step) and `OrganizationAdminFleet`
  (organization, stamp authority, stamp standing, the one applying transaction).
- The adapter is a child of `fleet_runtime`
  (`fleet_runtime/organization_link_fleet.rs`,
  `fleet_runtime/organization_admin_fleet.rs`). It checks the envelope, borrows
  the Fleet's parts into a view that implements the trait, runs the store steps
  through `StepDriver`, and encodes the answer.
- The admin request's applying transaction stays one callback (`ApplyAdmin`),
  unchanged: the clock sampled inside it, admission against the floors and the
  replay check, then the decision, the marker, the record and the domain
  effect. It lives in the adapter because it is a step-driver callback.

**Edge façades** (`organization_link`, `organization_admin_link`):
- The routers are built over ports: the existing `OrganizationProvisioningPort`
  and a small `OrganizationAdminPort`.
- The ports' transport over a host's object calls lives with the existing
  edge-to-object transport owner
  (`object_transport/organization_link_transport.rs`), whose recorded edges
  already cover the caller context, call graph, envelope and platform it uses.
  That is also where the public `organization_link_router(platform)` is defined
  now; its signature is unchanged.

## Rules carried over unchanged

- Clocks the applying transaction needs are passed in as `Arc<dyn Clock>` and
  sampled inside the transaction, as before.
- Each store step is still one transaction; no adapter adds or removes one.
- Refusals, answers and observations come from the same code in the same order.
- Conformance hooks (`before_apply`, `take_record_fault`, `take_answer_drop`)
  are trait methods behind `feature = "conformance"`.
