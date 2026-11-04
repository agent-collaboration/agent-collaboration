use board_sync_protocol::BoardDeliveryTypes;
use collaboration_central_core::central_mailbox_limits;
use fleet_sync_delivery::minimum_settlement_allowance;

#[test]
fn board_mailbox_defaults_reserve_enough_room_to_settle_admitted_items() {
    // PD4's caps must admit a valid configuration for the actual board codec.
    let minimum = minimum_settlement_allowance::<BoardDeliveryTypes>().unwrap();
    let limits = central_mailbox_limits().unwrap();
    assert_eq!(limits.settlement_allowance(), minimum);
    assert_eq!(limits.mailbox_payload_bytes(), 1024 * 1024 * 1024);
    assert_eq!(limits.mailbox_items(), 200_000);
    assert_eq!(limits.outbox_bytes(), 256 * 1024 * 1024);
    assert_eq!(limits.outbox_items(), 50_000);
    assert_eq!(limits.inbox_bytes(), 256 * 1024 * 1024);
}
