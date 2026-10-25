# ADR 0002: A lease registry with fencing, and explicit holder changes

**Status:** accepted, 2026-10-11. **Decided by:** the owner (2026-10-08 and 2026-10-09). **Design:** requirements U37–U39; spec 2 WA-4, WA-5, WA-11; spec 3 FL-3, FL-9, FL-10.

## Context
A project has one writer at a time. A lease says which machine that is. A machine that goes away must be stoppable without risking two writers.

## Decision
- **The registry and the fence.** A lease registry, separate from project data, is the only issuer of leases and epochs. Each project's central copy keeps a fence and refuses anything from an older epoch.
- **Lease timing.** A lease lasts **4 hours** and is renewed every **15 minutes**, off the write path.
- **After expiry,** with no holder change, the holder refuses new writes (`leaseExpired`) and pauses that project's wakes and schedules. Reads keep working, and everything resumes on renewal. There's no draft state.
- **Holder changes.** Every change of holder is an explicit request through a **separate admin endpoint with its own credentials**. Machine and agent credentials can't make one. A move drains the holder first, so nothing is lost. A takeover fences first and grants second. It's allowed at any time, voids the unpushed tail, and shows a preview of what will be lost.

## Consequences
- Agents can't work without internet, and a 4-hour Cloudflare outage is unlikely. So expiry only happens when something is really wrong, and writes keep flowing through short outages.
- The 4-hour lease also bounds how large an unpushed backlog can grow.
- Moving off a dead machine waits for an admin request.
- A takeover of a machine that is still alive voids its recent work. That is the admin's explicit choice.

## Evidence
- The Advisor's lease memo (2026-10-08).
- The owner's lease decisions (2026-10-09).

## Revisit when
Fleet size makes one registry a bottleneck, or the owner wants automatic failover.
