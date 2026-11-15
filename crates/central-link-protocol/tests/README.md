# Central link protocol proof

Unit cases are colocated in `src/link_codec_test.rs` and `src/link_request_test.rs`.
They exercise the actual text codec, strict direction and semantic decoding,
UUIDv7 correlation and distinct write-id policy. No integration or e2e target is
needed for this driver-free vocabulary. Socket admission and hibernation belong
to the later host slice.
