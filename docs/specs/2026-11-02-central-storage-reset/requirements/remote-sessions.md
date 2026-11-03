# Requirements: remote-sessions

An enrolled machine creates a session on another enrolled machine.

**Today.** A session can be created only on the caller's own machine (`crates/collaboration-service/src/collaboration_application/conversation_operations.rs:99`, local actors required at :244). Remote-start request types exist but nothing uses them (`crates/fleet-identity-protocol/src/remote_start.rs`, referenced only by its own contract test).

| ID | Need | Owner's words | Authority |
| --- | --- | --- | --- |
| <a id="u10"></a>U10 | From one machine, create a session on another machine. For now any machine enrolled in this Fleet may do so; this is temporary, and code comments say so. Best effort: the caller hears that the request arrived, then the new session or a refusal, or “not sure” if the target cannot tell whether it started one | “we need to be able to create a session in another computer”; “you cannot create sessions in another machine so broken too”; “for now if a machine is registered in workspace it can create”; “put note in comments so other agents don't think this is forever”; “best effort” | authorized, 2026-11-02 |

**Not asked for:** workspace membership or permission rules (“out of scope for you”); an approval step before creating (“chief of staff is my abstraction not in the system”); any change to the new session's own tool approvals.
