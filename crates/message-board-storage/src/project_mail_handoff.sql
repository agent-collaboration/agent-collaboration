SELECT item_id AS "item_id!: String", handoff_seq AS "handoff_seq!: i64", item AS "item!: String"
FROM mail_handoffs WHERE item_id=?
