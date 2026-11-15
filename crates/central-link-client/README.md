# Central link client policy

The Host creates one client with its machine id and injected RuntimeClock, then
owns socket I/O. `poll_time` emits connection, heartbeat or call-expiry actions;
`next_deadline` tells that owner when to poll again using the same clock.
Drain `poll_time` until None before processing socket events, including when
several deadlines coincide. Every accepted call has a 30-second deadline; a
connection attempt has ten seconds to receive Welcome, on this same clock.
The client spawns no tasks or timers. Welcome makes an attempt connected, and
requests before Welcome or while disconnected/refused fail immediately.
On a `Reconnect` action, close the current socket and settle the returned calls;
the client has already scheduled its backoff. Open another socket only when a
later `Connect` action supplies a new attempt identity.

Pass the connection-attempt identity back with received frames, close events
and refusal events. A refusal may arrive before or after `closed` for the latest
attempt; both orders end Refused without completing callers twice. A late close
after a timeout is idempotent. Obsolete callbacks cannot affect a newer attempt,
and re-enrollment invalidates the previous attempt.
`begin_request` mints transport correlation, preserving the payload's durable
write id. It is potentially sent as soon as it is accepted: a forgotten progress
callback cannot downgrade a committed write. Only an explicit
`send_failed(DefinitelyNotSent)` receipt makes an accepted write unavailable;
expiry, ambiguous failure and disconnection produce OutcomeUnknown with its
durable id. Reads fail as unavailable. A matching reply itself proves receipt.
There is no offline queue or automatic replay.

The socket owner closes after a receive protocol error and reports `closed` so
in-flight callers settle. Replies for resolved calls produce a typed LateReply
event, do not complete the caller again, and do not require closing. Resolved
correlations are retained for that attempt and cleared at the next connection;
their memory use grows with completed calls on a long-lived connection.
Literal pongs reset heartbeat observation. Two actual unanswered probes, each
given a 20-second answer interval, cause reconnect with 1–30-second capped
backoff; a late poll never invents a probe. Credential
refusal remains until re-enrollment. See `tests/README.md` for the protocol-stub
proof and the real-socket behavior this slice does not establish.
