//! Normal enrollment, socket barriers and observations shared by lifecycle cases.

use super::{holder_writer_retry::owner_post, normal_capability_cases::observe};
use collaboration_central_conformance::{
    ConformanceClient, ConformanceResult, enroll_open_peer_with_label,
};
use futures_util::{SinkExt, StreamExt};
use reqwest_websocket::{Message, WebSocket};
use serde_json::{Value, json};
use std::time::Duration;

pub(super) fn bounded_http_client() -> ConformanceResult<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .build()?)
}

pub(super) struct NormalSession {
    pub service: String,
    pub peer: durable_steps::Uuid,
    pub session: String,
    pub connection: String,
    pub socket: WebSocket,
}

impl NormalSession {
    pub async fn open(base: &str, label: &str) -> ConformanceResult<Self> {
        let service = durable_steps::Uuid::now_v7().to_string();
        let proposed_peer = durable_steps::Uuid::now_v7().to_string();
        let session = durable_steps::Uuid::now_v7().to_string();
        let admitted = enroll_open_peer_with_label(
            &ConformanceClient::new(base)?,
            &service,
            &proposed_peer,
            label,
            label,
            &session,
        )
        .await?;
        let connections = observe(
            base,
            json!({"kind":"sessionConnections","value":{"service":service}}),
        )
        .await?;
        let [connection] = connections
            .as_array()
            .ok_or("connections not an array")?
            .as_slice()
        else {
            return Err("normal fixture must own exactly one physical socket".into());
        };
        if connection.get("peer") != Some(&json!(admitted.peer))
            || connection.get("registration") != Some(&json!({"kind":"opening"}))
        {
            return Err(format!("normal socket was not Opening: {connection}").into());
        }
        Ok(Self {
            connection: connection
                .get("connection")
                .and_then(Value::as_str)
                .ok_or("connection missing")?
                .to_owned(),
            service,
            peer: admitted.peer,
            session,
            socket: admitted.socket,
        })
    }

    pub async fn hello(&mut self) -> ConformanceResult<()> {
        self.socket
            .send(Message::Text(
                json!({"kind":"hello","value":{
                    "sessionId":self.session,"schemaSupport":{"maxVersion":1}
                }})
                .to_string(),
            ))
            .await?;
        Ok(())
    }

    pub async fn welcome(&mut self) -> ConformanceResult<()> {
        let actual = data(&mut self.socket).await?;
        if actual != json!({"kind":"welcome","value":{"sessionId":self.session,"databases":[]}}) {
            return Err(format!("normal session Welcome changed: {actual}").into());
        }
        Ok(())
    }

    pub async fn attached(base: &str, label: &str) -> ConformanceResult<Self> {
        let mut session = Self::open(base, label).await?;
        session.hello().await?;
        session.welcome().await?;
        Ok(session)
    }

    pub async fn replacement(&self, base: &str, label: &str, now: i64) -> ConformanceResult<Self> {
        let admitted = enroll_open_peer_with_label(
            &ConformanceClient::new(base)?,
            &self.service,
            &self.peer.to_string(),
            label,
            label,
            &self.session,
        )
        .await?;
        let connections = observe(
            base,
            json!({"kind":"sessionConnections","value":{"service":self.service}}),
        )
        .await?;
        let connection = connections
            .as_array()
            .ok_or("connections not an array")?
            .iter()
            .find(|item| item.get("registration") == Some(&json!({"kind":"opening"})))
            .and_then(|item| item.get("connection"))
            .and_then(Value::as_str)
            .ok_or("replacement Opening connection missing")?
            .to_owned();
        let mut replacement = Self {
            service: self.service.clone(),
            peer: admitted.peer,
            session: self.session.clone(),
            connection,
            socket: admitted.socket,
        };
        replacement.pin(base, now).await?;
        replacement.hello().await?;
        replacement.pin(base, now).await?;
        replacement.welcome().await?;
        Ok(replacement)
    }

    pub async fn registration(&self, base: &str) -> ConformanceResult<Value> {
        let connections = observe(
            base,
            json!({"kind":"sessionConnections","value":{"service":self.service}}),
        )
        .await?;
        connections
            .as_array()
            .ok_or("connections not an array")?
            .iter()
            .find(|item| item.get("connection") == Some(&json!(self.connection)))
            .and_then(|item| item.get("registration"))
            .cloned()
            .ok_or("normal connection not present".into())
    }

    pub async fn fault(&self, base: &str, command: Value) -> ConformanceResult<Value> {
        observe(
            base,
            json!({"kind":"sessionFault","value":{"service":self.service,"command":command}}),
        )
        .await
    }

    pub async fn control(&self, base: &str, command: Value) -> ConformanceResult<Value> {
        owner_post(
            &bounded_http_client()?,
            base,
            "runtime",
            json!({
                "address":{"kind":"connection","value":self.service},"command":command
            }),
        )
        .await
    }

    pub async fn pin(&self, base: &str, millis: i64) -> ConformanceResult<()> {
        let time = chrono::DateTime::from_timestamp_millis(millis).ok_or("invalid fixed time")?;
        self.control(base, json!({"kind":"setTime","value":{"now":time}}))
            .await?;
        Ok(())
    }

    pub async fn barrier(&mut self, base: &str) -> ConformanceResult<()> {
        self.fault(
            base,
            json!({"kind":"armBarrier","value":{"connection":self.connection}}),
        )
        .await?;
        observe(base, json!({"kind":"sessionBarrier","value":{"service":self.service,"connection":self.connection}})).await?;
        if !matches!(frame(&mut self.socket).await?, Message::Binary(bytes) if bytes.as_ref() == b"session-finalize-barrier")
        {
            return Err(
                "ineligible holder received a frame before its ordered capability barrier".into(),
            );
        }
        Ok(())
    }

    pub async fn close(self) -> ConformanceResult<()> {
        self.socket
            .close(reqwest_websocket::CloseCode::Normal, None)
            .await?;
        Ok(())
    }
}

pub(super) async fn data(socket: &mut WebSocket) -> ConformanceResult<Value> {
    match frame(socket).await? {
        Message::Text(text) => Ok(serde_json::from_str(&text)?),
        Message::Binary(bytes) => Ok(serde_json::from_slice(&bytes)?),
        other => Err(format!("expected a data frame, received {other:?}").into()),
    }
}

pub(super) async fn frame(socket: &mut WebSocket) -> ConformanceResult<Message> {
    tokio::time::timeout(Duration::from_secs(10), socket.next())
        .await?
        .ok_or("normal socket ended before its frame")?
        .map_err(Into::into)
}

pub(super) async fn physical_close(socket: &mut WebSocket) -> ConformanceResult<()> {
    if !matches!(
        tokio::time::timeout(Duration::from_secs(5), socket.next()).await?,
        Some(Ok(Message::Close { .. })) | None
    ) {
        return Err("normal socket did not physically close".into());
    }
    Ok(())
}
