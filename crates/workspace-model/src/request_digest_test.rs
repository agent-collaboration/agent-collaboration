use super::*;
use serde_json::json;

#[test]
fn the_digest_ignores_key_order_and_changes_with_any_content() {
    let post = json!({"kind": "threadPost", "threadId": "t", "body": "hello"});
    let reordered = json!({"body": "hello", "threadId": "t", "kind": "threadPost"});
    let edited = json!({"kind": "threadPost", "threadId": "t", "body": "hello!"});

    let digest = operation_content_digest(&post).unwrap();

    assert_eq!(digest, operation_content_digest(&reordered).unwrap());
    assert_ne!(digest, operation_content_digest(&edited).unwrap());
    assert_eq!(digest.len(), 64);
    assert!(!digest.contains("hello"));
}
