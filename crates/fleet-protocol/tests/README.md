# Tests

The integration target preserves every upstream public API and wire contract test.
`session_frame_golden` pins released single-frame bytes in `fixtures/session-wire`.
`session_batch_wire` covers new batch/notice wire vocabulary, strict decoding,
operation homogeneity, UUIDv7 correlation, encoded-size and count boundaries.
