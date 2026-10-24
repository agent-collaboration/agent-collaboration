SELECT item_id AS "item_id!", recipient_service_id AS "recipient_service_id!",
       recipient_endpoint_id AS "recipient_endpoint_id!", recipient_session_id AS "recipient_session_id!",
       membership_id AS "membership_id!", received_at AS "received_at!"
FROM pending_poll_mail
WHERE recipient_service_id=? AND recipient_endpoint_id=? AND recipient_session_id=?
ORDER BY received_at, item_id
