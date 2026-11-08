SELECT CAST(id AS TEXT) AS id,
    typeof(id) AS id_class,
    CAST(minimum_revision AS TEXT) AS minimum_revision,
    typeof(minimum_revision) AS minimum_revision_class,
    CAST(state AS TEXT) AS state,
    typeof(state) AS state_class,
    CAST(permission_policy_digest AS TEXT) AS permission_policy_digest,
    typeof(permission_policy_digest) AS permission_policy_digest_class,
    CAST(request_reject_through_ms AS TEXT) AS request_reject_through_ms,
    typeof(request_reject_through_ms) AS request_reject_through_ms_class
FROM organization_authorization_floors WHERE id = ?;
