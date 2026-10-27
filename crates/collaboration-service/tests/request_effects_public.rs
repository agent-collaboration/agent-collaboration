#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
#![allow(clippy::indexing_slicing)] // Strict JSON assertions exercise the real public carrier.
#[allow(dead_code)] // This scenario uses only the Fleet portion of the shared real fixture.
#[path = "support/board_fixture.rs"]
mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use collaboration_client::board::{
    BoardId, DecisionEffectsHeader, RequestDecision, RequestEffectsRead, RequestEffectsRefusal,
    RequestEffectsRequest, RequestEffectsScope, RequestId, WorkspaceId,
};
use serde_json::json;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the public contract; Result propagates fixture failures.
async fn a_large_fleet_write_returns_a_signed_public_continuation() -> Result<(), TestError> {
    // Arrange real Fleet history through the same tools as the caller.
    let fixture = BoardFixture::start().await?;
    for index in 0..257 {
        let mut create = fixture.header();
        create["workspaceId"] = json!(fixture.workspace);
        create["boardId"] = json!(BoardId::generate());
        create["name"] = json!(format!("receipt board {index}"));
        create["description"] = json!("");
        create["kind"] = json!({"kind":"area"});
        accepted(&fixture.api.call("board_create", create).await?);
    }
    let request = RequestId::generate();
    let mut archive = fixture.header();
    archive["requestId"] = json!(request);
    archive["workspaceId"] = json!(fixture.workspace);
    // Act through the actual served MCP mutation path.
    let reply = fixture.api.call("workspace_archive", archive).await?;
    accepted(&reply);
    let page = reply
        .pointer("/result/result/decision/effects")
        .ok_or("missing receipt page")?;
    // Assert the public contract. One original board plus 257 new boards and the workspace.
    assert_eq!(page["totalCount"], json!(259));
    assert_eq!(page["entries"].as_array().ok_or("entries")?.len(), 256);
    assert!(
        page["next"]
            .as_str()
            .is_some_and(|next| next.starts_with("board2.")),
        "public write leaked a raw receipt keyset: {}",
        page["next"]
    );
    let next = page["next"].as_str().ok_or("continuation")?.to_owned();
    let mut all =
        serde_json::from_value::<Vec<message_board::DecisionEffect>>(page["entries"].clone())?;
    let header_digest = page["digest"].as_str().ok_or("digest")?.to_owned();
    let query = RequestEffectsRequest {
        scope: RequestEffectsScope::Fleet {
            workspace_id: fixture.workspace.clone(),
        },
        request: request.clone(),
        after: Some(next.clone()),
    };
    let client = fixture.api.client("request-effects-proof").await?;
    let RequestEffectsRead::Decision {
        decision: RequestDecision::Accepted { effects, .. },
    } = client.request_effects(query.clone()).await?
    else {
        return Err("SDK continuation did not return its accepted decision".into());
    };
    assert_eq!(effects.entries().len(), 3);
    assert_eq!(effects.total_count(), 259);
    assert_eq!(effects.digest().as_str(), header_digest);
    assert_eq!(effects.next(), None);
    all.extend_from_slice(effects.entries());
    assert_eq!(
        DecisionEffectsHeader::from_full(&all).digest.as_str(),
        header_digest
    );

    // Re-sign only test-owned payloads to check current commitment binding, beyond MAC integrity.
    let keys = message_board_storage::CursorStore::open(
        &fixture._directory.path().join("board-cursors.sqlite"),
    )
    .await?;
    let key = keys.key().await?.ok_or("stored signer")?;
    let payload: serde_json::Value = message_board_storage::decode_board_cursor(&key, &next)?;
    for field in ["count", "digest"] {
        let mut changed = payload.clone();
        changed[field] = if field == "count" {
            json!(260)
        } else {
            json!("0".repeat(64))
        };
        let mut invalid = query.clone();
        invalid.after = Some(message_board_storage::encode_board_cursor(&key, &changed)?);
        assert_eq!(
            client.request_effects(invalid).await?,
            RequestEffectsRead::Refused {
                reason: RequestEffectsRefusal::CursorInvalid
            }
        );
    }
    keys.close().await;
    let mut invalid = query.clone();
    invalid.after = Some(format!("{next}x"));
    assert_eq!(
        client.request_effects(invalid).await?,
        RequestEffectsRead::Refused {
            reason: RequestEffectsRefusal::CursorInvalid
        }
    );
    let mut wrong_scope = query.clone();
    wrong_scope.scope = RequestEffectsScope::Fleet {
        workspace_id: WorkspaceId::generate(),
    };
    assert_eq!(
        client.request_effects(wrong_scope).await?,
        RequestEffectsRead::Refused {
            reason: RequestEffectsRefusal::CursorInvalid
        }
    );
    let mut wrong_request = query.clone();
    wrong_request.request = RequestId::generate();
    assert_eq!(
        client.request_effects(wrong_request).await?,
        RequestEffectsRead::Refused {
            reason: RequestEffectsRefusal::CursorInvalid
        }
    );
    let unknown = fixture
        .api
        .call(
            "request_effects",
            json!({
                "scope":query.scope,"request":RequestId::generate(),"after":null,
            }),
        )
        .await?;
    assert_eq!(unknown.pointer("/result/kind"), Some(&json!("refused")));
    assert!(unknown.pointer("/result/result").is_none(), "{unknown}");
    assert_eq!(
        unknown.pointer("/result/reason/kind"),
        Some(&json!("unknownRequest")),
        "actual served-MCP unknown-request answer: {unknown}"
    );
    drop(client);

    // Explicit fixture pruning exercises retained receipts, not the production timer.
    let (deleted, remaining) = fixture._fleet_runtime.prune_fleet_feed().await?;
    assert!(deleted > 259);
    assert_eq!(remaining, 0);
    let fixture = fixture.restart_read_fixture().await?;
    let client = fixture.api.client("request-effects-reopen").await?;
    let RequestEffectsRead::Decision {
        decision: RequestDecision::Accepted { effects, .. },
    } = client.request_effects(query).await?
    else {
        return Err("continuation failed after feed pruning and owning-store reopen".into());
    };
    assert_eq!(effects.entries(), &all[256..]);
    assert_eq!(effects.digest().as_str(), header_digest);
    drop(client);
    fixture.stop().await?;
    Ok(())
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the public contract; Result propagates fixture failures.
async fn one_creating_request_has_distinct_project_and_fleet_receipts() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let project = message_board::ProjectId::generate();
    let creating = RequestId::generate();
    let mut request = fixture.header();
    request["requestId"] = json!(creating);
    request["boardId"] = json!(fixture.board);
    request["projectId"] = json!(project);
    request["title"] = json!("Receipt scopes");
    fixture
        .prepare_registered_creation(&serde_json::from_value(request.clone())?)
        .await?;
    accepted(&fixture.api.call("project_create", request).await?);
    let client = fixture.api.client("creating-request-receipts").await?;
    let project_query = RequestEffectsRequest {
        scope: RequestEffectsScope::Project {
            project_id: project.clone(),
        },
        request: creating.clone(),
        after: None,
    };
    let fleet_query = RequestEffectsRequest {
        scope: RequestEffectsScope::Fleet {
            workspace_id: fixture.workspace.clone(),
        },
        request: creating,
        after: None,
    };
    let RequestEffectsRead::Decision {
        decision:
            RequestDecision::Accepted {
                effects: project_effects,
                ..
            },
    } = client.request_effects(project_query.clone()).await?
    else {
        return Err("creating request lost its Project receipt".into());
    };
    let RequestEffectsRead::Decision {
        decision:
            RequestDecision::Accepted {
                effects: fleet_effects,
                ..
            },
    } = client.request_effects(fleet_query.clone()).await?
    else {
        return Err("creating request lost its Fleet directory receipt".into());
    };
    assert!(!project_effects.entries().is_empty());
    assert!(!fleet_effects.entries().is_empty());
    assert!(
        project_effects
            .entries()
            .iter()
            .all(|(_, place)| matches!(place, message_board::RecordPlacement::Project(_)))
    );
    assert!(
        fleet_effects
            .entries()
            .iter()
            .all(|(_, place)| matches!(place, message_board::RecordPlacement::Fleet(_)))
    );
    assert_ne!(project_effects.digest(), fleet_effects.digest());
    // Creation has small receipts, so these are fixture-signed, valid in-scope
    // keysets. Actual server-issued continuation paging is covered by the archive case.
    let keys = message_board_storage::CursorStore::open(
        &fixture._directory.path().join("board-cursors.sqlite"),
    )
    .await?;
    let key = keys
        .get_or_install_key(&vec![23_u8; 32].try_into()?)
        .await?;
    let sign_fixture_cursor =
        |query: &RequestEffectsRequest,
         effects: &collaboration_client::board::DecisionEffectsPage,
         cursor: message_board::RequestEffectsCursor| {
            message_board_storage::encode_board_cursor(
                &key,
                &json!({
                    "scope":query.scope,"request":query.request,"count":effects.total_count(),
                    "digest":effects.digest(),"cursor":cursor,
                }),
            )
        };
    let project_cursor = message_board::RequestEffectsCursor::Project {
        position: project_effects
            .entries()
            .last()
            .ok_or("last Project effect")?
            .1
            .project_cursor()
            .ok_or("Project cursor")?
            .position,
    };
    let fleet_cursor = message_board::RequestEffectsCursor::Fleet {
        ordinal: fleet_effects
            .total_count()
            .checked_sub(1)
            .ok_or("last Fleet effect")?,
    };
    let project_token = sign_fixture_cursor(&project_query, &project_effects, project_cursor)?;
    let fleet_token = sign_fixture_cursor(&fleet_query, &fleet_effects, fleet_cursor)?;
    for (query, token) in [
        (&project_query, &project_token),
        (&fleet_query, &fleet_token),
    ] {
        let mut own_scope = query.clone();
        own_scope.after = Some(token.clone());
        let reply = client.request_effects(own_scope).await?;
        assert!(matches!(
            reply,
            RequestEffectsRead::Decision {
                decision: RequestDecision::Accepted { .. }
            }
        ));
    }
    for (query, token) in [(&project_query, fleet_token), (&fleet_query, project_token)] {
        let mut wrong_scope = query.clone();
        wrong_scope.after = Some(token);
        assert_eq!(
            client.request_effects(wrong_scope).await?,
            RequestEffectsRead::Refused {
                reason: RequestEffectsRefusal::CursorInvalid
            }
        );
    }
    keys.close().await;
    let unknown_project = message_board::ProjectId::generate();
    let unknown_path = fixture
        ._directory
        .path()
        .join("projects")
        .join(unknown_project.as_str());
    assert!(!unknown_path.exists());
    assert_eq!(
        client
            .request_effects(RequestEffectsRequest {
                scope: RequestEffectsScope::Project {
                    project_id: unknown_project
                },
                request: RequestId::generate(),
                after: None,
            })
            .await?,
        RequestEffectsRead::Refused {
            reason: RequestEffectsRefusal::UnknownRequest
        }
    );
    assert!(
        !unknown_path.exists(),
        "a receipt read created project storage"
    );
    drop(client);
    fixture.stop().await?;
    Ok(())
}
