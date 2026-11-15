# Central link client policy

The Host creates one client with its machine id and injected RuntimeClock, then
owns socket I/O. `poll_time` emits a connection attempt or heartbeat action;
`next_deadline` tells that owner when to poll again using the same clock.
The client spawns no tasks or timers. Welcome makes an attempt connected, and
requests before Welcome or while disconnected/refused fail immediately.
On a `Reconnect` action, close the current socket and settle the returned calls;
the client has already scheduled its backoff. Open another socket only when a
later `Connect` action supplies a new attempt identity.

Pass the connection-attempt identity back with received frames, close events
and refusal events. Obsolete callbacks cannot affect a newer connection.
`begin_request` mints transport correlation, preserving the payload's durable
write id. Mark a request as potentially sent before handing its bytes to a
socket write. A failed write is known-unsent only when the transport explicitly
guarantees that receipt; otherwise an unanswered write is OutcomeUnknown.
There is no offline queue or automatic replay.

The socket owner must close after a protocol error and report `closed` so
in-flight callers settle. Literal pongs reset heartbeat observation; two missed
20-second answers cause reconnect with 1–30-second capped backoff. Credential
refusal remains until re-enrollment. See `tests/README.md` for the protocol-stub
proof and the real-socket behavior this slice does not establish.
