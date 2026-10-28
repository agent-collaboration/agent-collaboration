-- Delivery progress belongs to this Router, independently of replicated membership.
CREATE TABLE subscription_delivery_progress (
    project_id TEXT NOT NULL,
    membership_id TEXT NOT NULL,
    activated_epoch INTEGER NOT NULL,
    activated_position INTEGER NOT NULL,
    payload TEXT NOT NULL,
    PRIMARY KEY(project_id, membership_id, activated_epoch, activated_position)
);
