//! Verify guarded reload through upstream's native protocol, without writing auth.
use std::path::Path;

use codex_native_integration::NativeProtocolConnection;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

pub(super) enum ReloadObservation {
    SignedOut,
    Chatgpt {
        account_id: Option<String>,
        access_digest: [u8; 32],
    },
}

pub(super) async fn reload(socket: &Path) -> Result<ReloadObservation, &'static str> {
    let mut client = NativeProtocolConnection::connect(socket)
        .await
        .map_err(|_| "failed: native app-server connection unavailable")?;
    let result = client
        .request("account/read", json!({"refreshToken": true}))
        .await
        .map_err(|_| "failed: guarded account/read did not return a result")?;
    let account: AccountRead =
        serde_json::from_value(result).map_err(|_| "failed: invalid account/read result")?;
    match account.account {
        None => return Ok(ReloadObservation::SignedOut),
        Some(NativeAccount::Other) => {
            return Err("failed: app-server is not using managed ChatGPT authentication");
        }
        Some(NativeAccount::Chatgpt {}) => {}
    }
    // account/read swallows refresh errors upstream: RPC success alone is not adoption.
    let access_digest = cached_access_digest(&mut client)
        .await?
        .ok_or("failed: app-server has no cached ChatGPT access token")?;
    Ok(ReloadObservation::Chatgpt {
        account_id: account
            .workspace_routing
            .map(|routing| routing.chatgpt_account_id),
        access_digest,
    })
}

async fn cached_access_digest(
    client: &mut NativeProtocolConnection,
) -> Result<Option<[u8; 32]>, &'static str> {
    let response = client
        .request(
            "getAuthStatus",
            json!({"includeToken": true, "refreshToken": false}),
        )
        .await
        .map_err(|_| "failed: cached authentication observation unavailable")?;
    let status: CachedAuthStatus = serde_json::from_value(response)
        .map_err(|_| "failed: invalid cached authentication result")?;
    Ok(status
        .auth_token
        .map(|token| Sha256::digest(token.as_bytes()).into()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CachedAuthStatus {
    auth_token: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountRead {
    account: Option<NativeAccount>,
    workspace_routing: Option<WorkspaceRouting>,
}

#[derive(Deserialize)]
#[serde(tag = "type")]
enum NativeAccount {
    #[serde(rename = "chatgpt")]
    Chatgpt {},
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkspaceRouting {
    chatgpt_account_id: String,
}
