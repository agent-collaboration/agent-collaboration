# Requirements: central-layout

Central is one Fleet object plus one object per project, the same on Tokio and Cloudflare.

**Today.** Central has five kinds of object: Fleet and Registry once each, a Connection and a Mailbox for every machine, and a Copy per project (`crates/collaboration-central-core/src/core_contract.rs:36`). With M machines and P projects that is 2 + 2M + P stores. The owner calls the per-machine split broken.

| ID | Need | Owner's words | Authority |
| --- | --- | --- | --- |
| <a id="u1"></a>U1 | One organization has one Fleet: central is one Fleet object plus one object per project, on both Tokio and Cloudflare central | “we could have just 1 object. with n project objects. unless durable objects were gonna break but i doubt it”; “this is nailed down already one org one fleet”; “cloudflare and tokio works” | authorized, 2026-11-02 |
| <a id="u2"></a>U2 | No central store exists per machine | “database broken for fleet central”; “think about why you would want a database per machine … it's all fucked” (second quote as recorded in the 2026-11-02 handoff) | authorized, 2026-11-02 |
| <a id="u3"></a>U3 | At cutover, staging central starts empty: queued messages may be lost and machines are enrolled again | “machine to machine is not working so i dont care if the messages are dropped. enrollments i can enroll them again with router my computers are behind cloudflare access” | authorized, 2026-11-02 |

**Not asked for:** carrying staging data across, more than one organization per deployment (“one org one fleet”), or any change to project content replication beyond where project objects are addressed from.
