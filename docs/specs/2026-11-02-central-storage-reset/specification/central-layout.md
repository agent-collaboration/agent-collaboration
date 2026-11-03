# Specification: central-layout

What must be observably true about central's shape. Entities: [E1](entities.md#e1), [E2](entities.md#e2), [E3](entities.md#e3), [E5](entities.md#e5).

| ID | Obligation | Failure expectation | Proof obligation (what decides pass) | Basis |
| --- | --- | --- | --- | --- |
| <a id="r1"></a>R1 | Central holds exactly one Fleet store per deployment, plus one store per Project. No store exists per Machine | n/a | Against a running central with three enrolled Machines and two Projects, the set of central stores is exactly one Fleet store and two Project stores, on Tokio (files) and on Cloudflare (objects) | U1, U2 |
| <a id="r2"></a>R2 | Every Machine keeps one connection to the Fleet, and everything between that Machine and central (Messages, Statuses, Fleet requests, Project replication) travels over it | If the connection drops, the Machine reconnects and resumes; nothing it had handed to its outbox is lost | With two Machines connected, the Fleet holds two connections and no other central store holds a Machine connection; after a forced disconnect of one Machine, its next Message is delivered after it reconnects | U1, U2 |
| <a id="r3"></a>R3 | Project content stays in its own Project store, reached through the Fleet | If a Project store is unavailable, other Projects and all mail keep working | A Machine reads and writes a Project after cutover; a fault injected into one Project store leaves Message delivery and other Projects unaffected | U1 |
| <a id="r4"></a>R4 | Tokio central and Cloudflare central behave the same for every obligation in this design | A behavior seen on one and not the other is a defect | Every proof in this design passes on both backends | U1 |
| <a id="r5"></a>R5 | At cutover, staging central starts empty. Machines enroll again; queued Messages from before may be lost | n/a | After cutover, re-enrolled Machines exchange Messages and use their Projects | U3 |
| <a id="r6"></a>R6 | Every central table belongs to one concern and starts with that concern's prefix | n/a | The schema listing of the Fleet store shows only prefixed tables, one prefix per concern | U11 (also serves) |

**Limit (not an obligation):** whether one Fleet object's request rate, CPU and memory fit the real workload on Cloudflare is not yet measured. Cloudflare's published limits are 10 GB per object, about 1,000 requests a second (soft), 32,768 hibernating WebSockets and one alarm per object. The D1 proof measures it.
