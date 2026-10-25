SELECT receipt_id, CAST(ordinal AS TEXT) AS ordinal, typeof(ordinal) AS ordinal_class, effect
FROM fleet_decision_receipts WHERE request_id = ? ORDER BY fleet_decision_receipts.ordinal;
