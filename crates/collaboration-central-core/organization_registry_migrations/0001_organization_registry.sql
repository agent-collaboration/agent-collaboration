-- PROPOSAL ONLY. Not installed, included by Rust, generated against, or executed.
-- Governing basis: PD cbda512d / plan 7b1836e1, Organization Registry.
-- Dedicated OrganizationRegistryObject, singleton organization-registry;
-- its own SQLite store on Tokio and Durable Object storage on Cloudflare.
-- Supersedes the unused directory proposal. No address column or routing cache.
-- Existing Registry/Fleet/authority tables and migrations are untouched.

-- Business table 1: the global organization list, owned by this object.
-- Rust decodes state as a closed Creating/Active enum and rejects corruption.
CREATE TABLE organizations (
    organization_id TEXT NOT NULL PRIMARY KEY,
    state TEXT NOT NULL
);

-- Business table 2: one authorized organization per installation AND peer.
-- Both keys are checked again in the claim transaction after authorization.
-- A same-organization repeat changes no row, including claimed_at.
-- claimed_at is injected UTC RFC3339 text, validated on every read.
CREATE TABLE fleet_machines (
    service_id TEXT NOT NULL PRIMARY KEY,
    peer_id TEXT NOT NULL UNIQUE,
    organization_id TEXT NOT NULL REFERENCES organizations(organization_id),
    claimed_at TEXT NOT NULL
);
CREATE INDEX fleet_machines_organization ON fleet_machines (organization_id);

-- Auxiliary immutable audit records, required by PD "Admin commands":
-- CreateOrganization and its record commit in the SAME Registry transaction.
-- This is separate from the two business tables shown in the ERD, like
-- F2a's organization_record. Included explicitly for owner SQL review.
-- payload is a validated typed record (actor, command, organization, change,
-- injected time). It is never a schema-less serde_json::Value domain row.
CREATE TABLE organization_registry_record (
    record_id TEXT NOT NULL PRIMARY KEY,
    organization_id TEXT NOT NULL REFERENCES organizations(organization_id),
    payload TEXT NOT NULL
);
CREATE INDEX organization_registry_record_organization
    ON organization_registry_record (organization_id);

-- The existing durable-steps journal schema owns the creation outbox body,
-- stable command identity and retry progress; it is reused without edits.
-- InitializeOrganization retains the owner email and command ID there.
-- No approval attempt/claim is written on a refused or unknown authorization.
-- Approval recovery uses the same request ID at the authority and sender
-- resends; a lost cancel leaves only an inert reservation until its expiry.
-- No SQL CHECK for enum, range, length or variant rules. Rust owns validation.
