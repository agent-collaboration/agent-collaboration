SELECT c.project_id,c.creating_request,c.carrier_installation,
       CAST(c.created_at AS TEXT) AS created_at,typeof(c.created_at) AS created_class,
       CAST(c.updated_at AS TEXT) AS updated_at,typeof(c.updated_at) AS updated_class
FROM fleet_creation_decisions AS c WHERE c.project_id=?1;
