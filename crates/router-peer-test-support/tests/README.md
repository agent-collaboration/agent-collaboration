# router-peer-test-support tests

CI and test infrastructure owns this crate's scripted Router health, native
app-server, Claude executable, signal and peer socket fixtures. It has no
installed-Codex harness or product binary discovery. Each consumer owns its
compiled artifact paths and re-exec selectors.

Run the library's 11 real Unix protocol and artifact-lifetime cases with:

```sh
cargo nextest run --locked -p router-peer-test-support
```

The cases live in `src/owned_peer_fixture_test.rs` under the unchanged
`owned_peer_fixture::tests` namespace. They check authenticated content,
barrier counts, partial-frame cancellation and cleanup that preserves unrelated
files. Nextest runs each case in its own process with the existing defaults.
No integration or e2e target is declared because this crate has only unit tests.
