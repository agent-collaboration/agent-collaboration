use super::*;

#[test]
fn grouping_cycle_and_stored_values_cover_only_the_declared_facets() {
    let declared = [
        (
            QuotaGroupFacet::None,
            "None",
            "none",
            QuotaGroupFacet::Provider,
        ),
        (
            QuotaGroupFacet::Provider,
            "Provider",
            "provider",
            QuotaGroupFacet::Status,
        ),
        (
            QuotaGroupFacet::Status,
            "Status",
            "status",
            QuotaGroupFacet::None,
        ),
    ];

    for (facet, label, stored, next) in declared {
        assert_eq!(facet.label(), label);
        assert_eq!(facet.stored_value(), stored);
        assert_eq!(QuotaGroupFacet::from_stored_value(Some(stored)), facet);
        assert_eq!(facet.next_facet(), next);
    }
}

#[test]
fn unknown_stored_grouping_opens_as_none() {
    for stored in [
        None,
        Some(""),
        Some("removed-facet"),
        Some("Provider"),
        Some(" provider"),
        Some("provider\0"),
    ] {
        assert_eq!(
            QuotaGroupFacet::from_stored_value(stored),
            QuotaGroupFacet::None
        );
    }
}
