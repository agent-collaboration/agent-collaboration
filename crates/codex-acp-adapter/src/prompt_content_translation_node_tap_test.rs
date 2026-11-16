//! N8 node taps for ACP prompt content: text and resource links become Codex
//! text; other content and over-limit prompts are refused.
//!
//! Program Design, content conversions row: N8 `prompt_content_translation.rs`
//! turns ACP text and resource links into Codex text, keeps the resource
//! description, refuses other content, and limits a prompt to 256 blocks and
//! 64 MiB.
//!
//! Recording layers: the translator is handed the `session/prompt` params without their
//! envelope, so they are a `Body` checked as that request's params; its `{sessionId, input}`
//! output is a typed outcome, not a `turn/start` (the real frame is seen at N8's fake Codex).
use super::translate_prompt_content;
use crate::AcpSchemaCatalog;
use codex_router_test_support::node_taps::{
    BodyRoot, CatalogRowExpectation, EdgeParty, MessageFamily, NodeStateSnapshot, ProtocolNode,
    RecordLayer, TapCase,
};
use serde_json::{Value, json};

const CONTENT_ROW: &str = "Content conversions: N8 prompt_content_translation.rs";
const BYTE_LIMIT_ROW: &str = "Content conversions: N8 64 MiB limit";

/// The prompt's byte limit: the translated text of all blocks together (64 MiB).
const PROMPT_BYTE_LIMIT: usize = 64 * 1024 * 1024;

fn translate_and_record(case_name: &str, prompt: Vec<Value>) -> (TapCase, Vec<Value>) {
    translate_and_record_on(CONTENT_ROW, case_name, prompt)
}

fn translate_and_record_on(
    row: &str,
    case_name: &str,
    prompt: Vec<Value>,
) -> (TapCase, Vec<Value>) {
    let mut catalog = AcpSchemaCatalog::load().unwrap_or_else(|e| panic!("catalog: {e}"));
    let mut case = TapCase::new(ProtocolNode::N8CodexAcpAdapter, case_name, row);
    case.state_before(NodeStateSnapshot::new().field("blocks_in", prompt.len()));
    // Table gap: no family names ACP prompt content consumed by N8.
    let params = json!({"sessionId":"thread-a","prompt":prompt});
    case.consumed_raw(
        RecordLayer::Body {
            body: BodyRoot::AcpRequestParams {
                method: "session/prompt".into(),
            },
        },
        MessageFamily::AcpRequestToCodexAdapter,
        EdgeParty::McpCaller,
        "session/prompt",
        params.clone(),
        &[],
    );
    match translate_prompt_content(&mut catalog, &params) {
        Ok(translated) => {
            // Table gap: translated Codex input rides inside `turn/start`; the
            // closest row is a Codex request to Codex. The record is the whole translated prompt
            // (its session id and every input item), a Rust value, never a frame.
            case.produced_with(
                RecordLayer::TypedOutcome,
                MessageFamily::CodexRequestToCodex,
                EdgeParty::Node(ProtocolNode::N10CodexClient),
                "turn/start input",
                json!({"sessionId": translated.session_id(), "input": translated.input()}),
            );
            case.state_after(NodeStateSnapshot::new().field("result", "accepted"));
            (case, translated.input().to_vec())
        }
        Err(error) => {
            // The whole refusal variant, not a label chosen here.
            case.state_after(NodeStateSnapshot::new().field("result", format!("{error:?}")));
            (case, Vec::new())
        }
    }
}

#[test]
fn node_tap_text_and_resource_link_become_codex_text() {
    let (case, _input) = translate_and_record(
        "prompt_text_and_resource_link_accepted",
        vec![
            json!({"type":"text","text":"hello"}),
            json!({"type":"resource_link","uri":"file:///work/a.rs","name":"a.rs","description":"the file"}),
        ],
    );
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::CodexRequestToCodex,
            EdgeParty::Node(ProtocolNode::N10CodexClient),
            "turn/start input",
            json!({"sessionId":"thread-a","input":[
                {"type":"text","text":"hello","text_elements":[]},
                {"type":"text","text":"Resource: a.rs\nURI: file:///work/a.rs\nthe file","text_elements":[]}
            ]}),
        )
        .state_after(NodeStateSnapshot::new().field("result", "accepted"));
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_image_content_is_refused() {
    let (case, _) = translate_and_record(
        "prompt_image_content_refused",
        vec![json!({"type":"image","data":"AAAA","mimeType":"image/png"})],
    );
    let row = CatalogRowExpectation::new()
        .state_after(NodeStateSnapshot::new().field("result", "UnsupportedContent"));
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_prompt_over_256_blocks_is_refused() {
    let blocks = (0..257)
        .map(|_| json!({"type":"text","text":"x"}))
        .collect::<Vec<_>>();
    let (case, _) = translate_and_record("prompt_over_256_blocks_refused", blocks);
    let row = CatalogRowExpectation::new()
        .state_after(NodeStateSnapshot::new().field("result", "Capacity"));
    case.finish(&row).assert_settled();
}

/// Two text blocks whose translated text together is `total_bytes` long.
fn text_blocks_totalling(total_bytes: usize) -> Vec<Value> {
    let first = total_bytes / 2;
    vec![
        json!({"type":"text","text":"a".repeat(first)}),
        json!({"type":"text","text":"b".repeat(total_bytes - first)}),
    ]
}

#[test]
fn node_tap_prompt_of_exactly_64_mib_is_accepted() {
    let blocks = text_blocks_totalling(PROMPT_BYTE_LIMIT);
    let expected_input: Vec<Value> = blocks
        .iter()
        .map(|block| json!({"type":"text","text":block["text"],"text_elements":[]}))
        .collect();
    let (case, _) = translate_and_record_on(BYTE_LIMIT_ROW, "prompt_at_64_mib_accepted", blocks);
    let row = CatalogRowExpectation::new()
        .produces_with(
            MessageFamily::CodexRequestToCodex,
            EdgeParty::Node(ProtocolNode::N10CodexClient),
            "turn/start input",
            json!({"sessionId":"thread-a","input":expected_input}),
        )
        .state_after(NodeStateSnapshot::new().field("result", "accepted"));
    case.finish(&row).assert_settled();
}

#[test]
fn node_tap_prompt_one_byte_over_64_mib_is_refused() {
    let (case, _) = translate_and_record_on(
        BYTE_LIMIT_ROW,
        "prompt_over_64_mib_refused",
        text_blocks_totalling(PROMPT_BYTE_LIMIT + 1),
    );
    let row = CatalogRowExpectation::new()
        .state_after(NodeStateSnapshot::new().field("result", "Capacity"));
    case.finish(&row).assert_settled();
}
