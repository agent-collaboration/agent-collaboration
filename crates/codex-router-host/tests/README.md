# Host tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p codex-router-host --features keychain-test-support,runtime-test-support
```

`integration/main.rs` gathers the external roots, including native model and skill bridge cases. Each root keeps its
module namespace. The cases cover Host startup and configuration, singleton
ownership, child process groups, update and restart lifecycles, enrollment,
collaboration composition, provider routes, wakes and native inventory.
`shared_fleet_store` starts the real runtime on a fresh root and checks it keeps
one Fleet database (`fleet.sqlite`, no older store beside it), that a second
process asking to write it gets the store's lock error, and that a restart keeps
the board cursor key the composition installed.
`runtime_shutdown_stalled_provider_load` starts an enrolled Host whose Cursor
provider never answers `session/load`, lets a Wake follow batch reach that load,
and requires Host shutdown to return anyway. After a provider-less restart, the
reader's subscription view must show that cut-off delivery as outcome-unknown,
written either by shutdown or by durable mail recovery (the test does not tell
which): neither lost nor recorded delivered, so reconcile has it to settle.
Runtime replacement cases hold a real status probe across an app-server schema
change and require one Unavailable terminal for each cancelled operator request.
Library unit tests remain colocated with their implementations.

The D1 send roots drive a real Host through the real client and API with ACP
fixtures whose receipts record every frame the agent read:
`provider_send_behaviour` (one send behaviour: cannot steer, an explicit queue
or steer, and mail and the CLI for each session state),
`provider_send_races` (forced idle-read, steer and recheck races; its
supervisor-level rig is in `direct_supervisor`),
`provider_send_approvers` (who approves a steered or queued message, and
unknown earlier outcomes refusing a send), `provider_queued_api_lifecycle` (the
line's bound, refusals, close interrupting a turn, cancel (by the queue and,
in `conversation_cancel_door`, by conversation cancel), provider stop,
retention, and in
`restart_stages` the clean-shutdown and hard-stop restart stages, the latter
through a relaunched child), `provider_acp_busy_prompt` (the Router's ACP socket
steering, queueing and not-sent answers), `provider_cache_warm` (Claude's
idle cache warm on the Host's injected clock), `provider_full_access_refusal`
(full-access modes refused at start and on settings set) and
`provider_work_roots` (work roots reaching Claude and Cursor sessions started
in auto) and `provider_resumed_work_roots` (a session loaded after a Host
restart gets back the work-root layer and requested mode its create
recorded). Each is bounded by the runner
(`.config/nextest.toml`), never by a clock in test code.

The aggregate declares shared helper implementations once. Other directories
retain case expansions and fixture data. Externally consumed service helpers
keep their existing source owner; Host cases import the shared nested modules.
No empty e2e target is declared: this crate has no separately selected CI e2e
category.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.

Relaunched test names derive from `module_path!()`. Normally completed children
with captured output must report one passed test. Intentional exits, kills,
exec replacements and managed children with discarded output retain actual
readiness, effect, exit-code or reaping checks that reject zero matches.

The provider scenarios that hold the Host at a named point (design note
§11.8) use the crate's `test-support` feature, which exposes only
`ExternalProviderSupervisor::pause_terminal_write_of`,
`ExternalProviderSupervisor::pause_explicit_mode_record_of`,
`ExternalProviderSupervisor::notice_next_explicit_mode_wait` and
`QUEUE_RETRY_DELAY`:
`provider_queue_settlement` (a queued message commits only after the running
prompt's settlement is written, on a ManualClock), `provider_persistence_overlay`
(a settlement that fails to save answers its outcome and durable stage, through
collaboration-runtime's StoreFault) and `provider_router_close` (Router-side
release cases through the public runtime). The two explicit-mode points hold
one session's mode change between the agent applying it and its record write,
and tell a test when a later change waits for that session's turn
(`external_provider_supervisor::start_mode_tests`, Lead V6b). Cases that prove a private invariant
stay unit tests next to their code.
