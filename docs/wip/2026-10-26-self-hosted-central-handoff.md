# Handoff: run Fleet without Cloudflare (self-hosted central) and migrate a machine's boards

Status: design handoff, 2026-10-26. Nothing in this document has been built.

## The goal

A machine (for example a work laptop) runs codex-router 0.1.102+ with a **central that isn't on Cloudflare**, and migrates its existing local boards into that central using the Rust importer (`codex-router-board-upgrade`).

## Is it easy? Not plug-and-play, but most of the pieces exist

| Piece | Exists today | What's missing |
|---|---|---|
| Central on Tokio (same core as the Cloudflare Worker, SQLite on disk) | yes: `collaboration-central-tokio`'s `TokioServer::start_on(settings, listen)` (`crates/collaboration-central-tokio/src/native_server.rs`) | no shipped binary: tests and the verification helper (`scripts/verification/board_upgrade_native/src/central.rs`) are the only callers |
| Caller authentication at central | central verifies Cloudflare Access-style signed tokens (RS256 against a JWKS) for owner admin routes and machine sessions (`collaboration-central-core/src/access_rules.rs`) | the JWKS URL is configurable (`TokioSettings.jwks_url`), but `AccessPolicy::validate` requires the issuer to be `*.cloudflareaccess.com` (`access_contract.rs`). Something other than Cloudflare has to mint the tokens |
| Router joins central | `codex-router fleet enroll`: an owner admin token plus a Cloudflare Access Service Auth client id and secret per machine | Without the Cloudflare edge, nothing turns Service Auth into the signed assertion central expects. The Router needs a way to present a machine assertion directly |
| Import of existing boards | `codex-router-board-upgrade` (Rust) uploads to central's admin import with the owner's token from `cloudflared access token` | It needs a non-Cloudflare way to get the owner token. A file-based assertion exists only in debug builds (`CODEX_ROUTER_DEBUG_CENTRAL_ACCESS_ASSERTION_FILE`) |

## Options

**A. No central on that machine (zero work, no cross-machine).** Keep the machine on 0.1.101, the last release before Fleet. Its boards stay local, and there's nothing to migrate yet. When B exists, upgrade and import then. Cost: no shared boards with other machines; that machine has to pin the older Homebrew version.

**B. A self-hosted central (recommended direction).**
1. **A central server binary**: a thin `main` around `TokioServer::start_on` that takes a root directory (SQLite stores), a listen address, the access policy, the JWKS source and the registry/delivery settings. It stays local-first: localhost or a private network such as Tailscale.
2. **A token issuer that replaces Cloudflare Access**:
   - a small signer holding an RSA key, publishing its public JWKS;
   - minting short-lived owner (admin audience) and machine (session audience) assertions with the claims central already checks.
   - In central: generalize `AccessPolicy` from "Cloudflare team domain" to "issuer + JWKS URL", with Cloudflare as one configuration. That's a small, contained change.
3. **The Router presents its machine assertion directly.** It enrolls with a machine credential from the issuer, held in the Router's Keychain-protected secret store, in place of Access Service Auth headers.
4. **The importer gets the owner assertion from the issuer** in release builds (today that's debug-only), then runs the same dry run followed by the real import.

This overlaps with the planned agent-identity design (Better Auth minting tokens, Fleet holding identity truth). The issuer in B2 should be that design's token service rather than a one-off, so the work isn't done twice.

**C. A second Cloudflare account just for work.** No code changes, but it's still Cloudflare, so it doesn't meet the goal. Listed only for completeness.

## Decision needed (owner)

- Choose A now and B later, or start B now.
- For B: who issues the tokens? A dedicated small signer now, or wait for the agent-identity / Better Auth token service. This is a **security boundary**, so it's the owner's call.
- Where does that central run: on the work laptop itself, or on a work server reachable over a private network?

## Rough size of B

| Part | Size |
|---|---|
| Central binary | small: wrap the existing server; config plus signal handling |
| Policy generalization | small: issuer and JWKS URL; Cloudflare stays a configuration |
| Issuer | medium: key handling, JWKS, minting, rotation (or reuse the identity design's service) |
| Router enroll and the importer's owner token without Cloudflare | medium: a new credential path into the secret store and import auth in release builds |
| Proof | the existing conformance suite on Tokio; an isolated enroll → import → claim → write journey (the same journeys used for the Cloudflare release) |

## Migration steps once B exists (same shape as the Cloudflare switch-over)

1. Back up the old binaries and the whole Router root, with the Router stopped.
2. Upgrade the Router.
3. Import dry run, then the real import, with the Router stopped (the Host lock must be free).
4. `fleet enroll` against the self-hosted central.
5. Start the Router.
6. Claim the imported projects (the holder is unset after import), and make a first write.
7. Check that every project landed on the right board and workspace (compare the listing with the import report).

Lessons from the 2026-10-25 Cloudflare switch-over that apply here:
- **Rehearse the real import against the real target first, with a copy of the real data.** Local dry runs didn't catch the importer's 30 s request timeout (fixed in #166, 600 s).
- **Plan for the importer's final local step taking a while.** The message copy and the old-table drop took about 16 minutes of CPU on a 250 MB automation store.
- **Re-claim the projects after the import**, or nothing can write to them.
