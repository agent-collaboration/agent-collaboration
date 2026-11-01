# Private test subscribers and parallel untraced producers

The Fleet login trace test registers a live `NoSubscriber` dispatch alongside
its private formatter. This describes the other, untraced tests to tracing's
global callsite-interest registry without installing a global subscriber.

In tracing-core 0.1.36, `Dispatchers::rebuilder` selects `Rebuilder::JustOne`
when it knows only one dispatch. That branch asks the registering thread's
current default for its interest. A parallel untraced producer can therefore
register a callsite with `Never` while the private subscriber needs it. Two
registered dispatches use the registry instead; combined interest is checked
against the current dispatch for each event. See the pinned
[callsite implementation](https://docs.rs/tracing-core/0.1.36/src/tracing_core/callsite.rs.html).

The observed failure was in the default parallel run:

```sh
cargo test --locked -p codex-router-host --lib fleet_machine_login -- --nocapture
```

At `abc08694`, 43 tests passed and the trace test failed its public-field
assertion. The same compiled trace test passed when selected alone. Static
maximum-level features were absent from the compiled tracing fingerprints.
The correction keeps the real owner tasks, formatter, public-field assertions
and credential-canary assertions. It changes no production logging or test
thread setting.

Remove this fixture registration when the pinned tracing implementation handles
parallel untraced producers without the shortcut, and the default parallel
focused suite passes with the registration removed. The full suite remains
the required proof; an isolated pass does not establish the correction.
