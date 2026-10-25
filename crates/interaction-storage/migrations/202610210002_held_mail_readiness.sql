CREATE INDEX router_pushes_held_mail_readiness ON router_pushes (
    target_service_id, target_endpoint_id, target_session_id,
    retention, delivery_state, created_at, push_id
);
