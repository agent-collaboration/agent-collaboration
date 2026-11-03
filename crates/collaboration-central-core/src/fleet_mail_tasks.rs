//! Task identities shared by Fleet mail admission and its owning dispatcher.
use board_sync_protocol::BoardDeliveryTypes;

pub(crate) type FleetStatusTask<Store, Driver> =
    fleet_sync_delivery::PublishStatus<Store, Driver, BoardDeliveryTypes>;
pub(crate) type FleetPushTask<Store, Driver> = fleet_sync_delivery::PushItem<
    Store,
    Driver,
    BoardDeliveryTypes,
    FleetStatusTask<Store, Driver>,
>;
pub(crate) type FleetMailRemoval<Store, Driver> =
    fleet_sync_delivery::RemoveDeliveryRecord<Store, Driver>;
