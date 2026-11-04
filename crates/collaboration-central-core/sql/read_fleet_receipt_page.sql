SELECT CAST(receipt_id AS TEXT) AS receipt_id,
typeof(receipt_id) AS receipt_id_class,
CAST(request_id AS TEXT) AS request_id,
typeof(request_id) AS request_id_class,
CAST(ordinal AS TEXT) AS ordinal,
typeof(ordinal) AS ordinal_class,
CAST(record_kind AS TEXT) AS record_kind,
typeof(record_kind) AS record_kind_class,
CAST(record_id AS TEXT) AS record_id,
typeof(record_id) AS record_id_class,
CAST(fleet_position AS TEXT) AS fleet_position,
typeof(fleet_position) AS fleet_position_class,
CAST(origin_installation AS TEXT) AS origin_installation,
typeof(origin_installation) AS origin_installation_class,
CAST(origin_machine AS TEXT) AS origin_machine,
typeof(origin_machine) AS origin_machine_class,
CAST(created_at AS TEXT) AS created_at,
typeof(created_at) AS created_at_class,
CAST(updated_at AS TEXT) AS updated_at,
typeof(updated_at) AS updated_at_class
FROM fleet_decision_receipts
WHERE request_id = ? AND ordinal > CAST(? AS INTEGER)
ORDER BY fleet_decision_receipts.ordinal LIMIT 256;
