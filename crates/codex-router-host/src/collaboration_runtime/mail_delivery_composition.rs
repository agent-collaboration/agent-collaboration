//! Select the immutable mail quota at the Host's lifetime boundary.
use collaboration_runtime::MailDeliverySession;
use collaboration_service::{MailDeliveryRuntime, MailPushDeliveryProps};
use fleet_protocol::PeerId;
use std::{io, path::Path};

pub(super) fn construct(
    peer: PeerId,
    props: MailPushDeliveryProps,
    session: Option<MailDeliverySession>,
    _directory: &Path,
) -> io::Result<MailDeliveryRuntime> {
    #[cfg(all(debug_assertions, feature = "runtime-test-support"))]
    if let Some(limit) = crate::debug_inbox_limit::load(_directory)? {
        return MailDeliveryRuntime::runtime_test_new_with_inbox_limit(peer, props, session, limit)
            .map_err(io::Error::other);
    }
    MailDeliveryRuntime::new(peer, props, session).map_err(io::Error::other)
}
