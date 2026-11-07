# agent-session-inventory test contracts

Unit tests stay colocated with their implementation (`*_test.rs` and the existing
`*_tests.rs` modules) and keep their Cargo/nextest identity. Shared fixtures
belong to their named test-support owners.

`integration/main.rs` is the package's one integration target. It drives the
public readers on real files:

- `machine_list_files`: the session picker's machine list is read from
  `agent-sessions.jsonc` at the Router root; the old `routers.jsonc` is never
  read, and when both exist neither is used and both are named.

Package and platform-specific checks remain in the workspace CI selection and
the owning source modules.
