//! Explicit deployment identity for existing legacy-Access conformance fixtures.
#[expect(
    clippy::expect_used,
    reason = "A fixed UUIDv7 fixture literal must fail loudly if an edit makes it invalid; it is not deployment configuration."
)]
pub fn legacy_organization() -> fleet_identity_protocol::OrganizationId {
    "019f0000-0000-7000-8000-00000000ac01"
        .to_owned()
        .try_into()
        .expect("fixed UUIDv7 organization")
}
pub fn legacy_fleet_address() -> collaboration_central_core::ObjectAddress {
    collaboration_central_core::ObjectAddress::Fleet {
        organization: legacy_organization(),
    }
}
