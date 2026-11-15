# Central link protocol

This driver-free crate owns Router–central text frames. Heartbeats are exactly
the unquoted text `ping` toward central and `pong` toward the Router, so the
Cloudflare adapter can configure a platform auto-response from the same codec.
All other frames are strict JSON with a `kind`: `welcome`, `request`, `reply`,
`deliver` or `ack`. The codec checks direction as well as decoding.

A Request's `request` object holds `correlationId`, `caller` and `payload`.
The payload selects an existing `workspace-model` workspace or mail request.
A Reply echoes `correlationId` and carries the matching typed `result` family.
Transport correlation is a fresh UUIDv7; a write's existing durable `requestId`
is independent. Reads carry no durable request id. The request constructor and
decoder enforce this policy together.

Caller stamping, token verification, workspace authorization, socket admission,
delivery acknowledgement and object hibernation have other owners. Successful
decoding establishes none of that authority. See `tests/README.md` for proof scope.
