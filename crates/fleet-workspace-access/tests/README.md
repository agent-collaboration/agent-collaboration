# Workspace access tests

The native and Worker host integration suites exercise workspace admission, actor
identities, notice cadence, and stored workspace decoding through the shared
runtime. The native link suite forces revocation while a live request waits for
its applying turn and checks the owning stores for request effects.

This crate has no standalone test target. The live connection validation tests
stay with the retained implementation in `collaboration-central-core`.
