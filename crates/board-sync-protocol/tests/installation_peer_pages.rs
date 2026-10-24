#![allow(clippy::unwrap_used)]
//! The directory pages have a fixed bound, typed identities and a progressing peer cursor.
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardReadPage, InstallationPeerPage,
    InstallationPeerPageError, MAX_INSTALLATION_PEERS_PAGE_ROWS,
};
use fleet_sync_protocol as wire;
use message_board::ServiceId;
use serde_json::{Value, json};

const FRAME: &str = "019a1234-5678-7000-8000-000000000001";
const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const PEER: &str = "019a1234-5678-7000-8000-000000000073";

fn peers(count: usize) -> Vec<(ServiceId, wire::PeerId)> {
    (0..count)
        .map(|index| {
            (
                format!("00000000-0000-4000-8000-{index:012x}")
                    .try_into()
                    .unwrap(),
                format!("019a1234-5678-7000-8000-{:012x}", index + 1)
                    .parse()
                    .unwrap(),
            )
        })
        .collect()
}

#[test]
fn paged_directory_request_round_trips_the_peer_cursor_without_a_request_limit() {
    for after in [json!(null), json!(PEER)] {
        let expected = json!({"kind":"request","value":{"frame":FRAME,"body":{"kind":"consumer","value":{
            "kind":"read","value":{"scope":{"kind":"installationPeers","value":{"after":after}},"cursor":null}
        }}}});
        let request: wire::ClientFrame<BoardConsumerProtocol> =
            serde_json::from_value(expected.clone()).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), expected);
    }
    for value in [json!({"after":SERVICE}), json!({"after":null,"limit":1000})] {
        assert!(
            serde_json::from_value::<board_sync_protocol::BoardReadScope>(
                json!({"kind":"installationPeers","value":value})
            )
            .is_err()
        );
    }
}

#[test]
fn final_directory_page_round_trips_distinct_service_and_peer_identities_in_a_real_frame() {
    let expected = json!({"kind":"answer","value":{"frame":FRAME,"body":{"kind":"consumer","value":{
        "kind":"read","value":{"kind":"installationPeers","value":{"peers":[[SERVICE,PEER]],"next":null}}
    }}}});
    let answer: wire::ServerFrame<BoardConsumerProtocol> =
        serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(answer).unwrap(), expected);
}

#[test]
fn full_typed_answer_stays_within_the_measured_byte_bound() {
    // Arrange: distinct canonical identities occupy their full fixed ASCII width.
    let rows = peers(MAX_INSTALLATION_PEERS_PAGE_ROWS);
    let next = rows.last().unwrap().1;
    let page = InstallationPeerPage::new(rows, Some(next)).unwrap();
    // Act: encode the real answered-page carrier, not a lookalike JSON fixture.
    let encoded = serde_json::to_vec(&BoardConsumerAnswer::Read(
        BoardReadPage::InstallationPeers(page),
    ))
    .unwrap();
    // Assert: the independently measured literal is exact and below the owner's bound.
    assert_eq!(encoded.len(), 80_118);
    assert!(encoded.len() <= 80_127);
}

#[test]
fn malformed_pages_refuse_at_typed_construction_and_decoding() {
    let rows = peers(2);
    let first = rows.first().unwrap().clone();
    let second = rows.last().unwrap().clone();
    let reversed = vec![second.clone(), first.clone()];
    assert_eq!(
        InstallationPeerPage::new(reversed, None),
        Err(InstallationPeerPageError::UnorderedPeers)
    );
    let duplicate_service = vec![first.clone(), (first.0.clone(), second.1)];
    assert_eq!(
        InstallationPeerPage::new(duplicate_service, None),
        Err(InstallationPeerPageError::DuplicateServices)
    );
    assert_eq!(
        InstallationPeerPage::new(peers(1001), None),
        Err(InstallationPeerPageError::TooManyPeers)
    );
    assert_eq!(
        InstallationPeerPage::new(rows.clone(), Some(first.1)),
        Err(InstallationPeerPageError::InvalidContinuation)
    );
    let mut invalid: Vec<Value> = vec![
        json!({"peers":[[SERVICE,SERVICE]],"next":null}),
        json!({"peers":[[SERVICE,PEER],[SERVICE,PEER]],"next":null}),
        json!({"peers":rows,"next":null,"unexpected":true}),
        json!({"peers":peers(1001),"next":null}),
    ];
    let full = peers(1000);
    invalid.push(json!({"peers":full,"next":PEER}));
    for page in invalid {
        assert!(serde_json::from_value::<InstallationPeerPage>(page).is_err());
    }
}

#[test]
fn validated_page_must_advance_the_requested_lower_bound() {
    let rows = peers(2);
    let first = rows.first().unwrap().1;
    let last = rows.last().unwrap().1;
    // A maximum is not a requirement to fill every intermediate page; `next` decides completion.
    let continuation = InstallationPeerPage::new(rows.clone(), Some(last)).unwrap();
    assert_eq!(continuation.next(), Some(last));
    let page = InstallationPeerPage::new(rows, None).unwrap();
    assert!(page.validate_after(None).is_ok());
    assert_eq!(
        page.validate_after(Some(first)),
        Err(InstallationPeerPageError::CursorNotAdvanced)
    );
    assert_eq!(page.peers().len(), 2);
    assert_eq!(page.next(), None);
    let terminal = InstallationPeerPage::new(Vec::new(), None).unwrap();
    assert!(terminal.validate_after(Some(first)).is_ok());
}
