//! N3 tap support: renders the connection's receipt state for node-tap cases.
use super::NativeUserReceipts;

/// Renders the receipts as `outstanding=[client@item,...] bound_next_item=<client|none>`
/// for one thread. Receipts are listed in registration order; `item` is `-` until
/// a user message item has been stamped with the client id.
pub(crate) fn render_receipt_state(receipts: &NativeUserReceipts, thread_id: &str) -> String {
    let outstanding = receipts
        .outstanding
        .iter()
        .map(|receipt| {
            format!(
                "{}@{}",
                receipt.client_id,
                receipt.item_id.as_deref().unwrap_or("-")
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let next_item = receipts
        .accepted
        .get(thread_id)
        .and_then(|input_id| {
            receipts
                .outstanding
                .iter()
                .find(|receipt| &receipt.input_id == input_id)
        })
        .map_or("none", |receipt| receipt.client_id.as_str());
    format!("outstanding=[{outstanding}] accepted_for_next_item={next_item}")
}
