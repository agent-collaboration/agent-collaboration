//! The actual Registry journal and Copy object retain bounded wait semantics.

use collaboration_central_conformance::{ConformanceResult, owner_assertion};
use durable_steps::{
    OperationId,
    storage::{CallReach, StepRecord, StepState},
};
use fleet_sync_protocol::{
    AwaitAcked, AwaitAckedAnswer, CommitSequence, CopyOrigin, CopyUnavailableReason, CreateCopy,
    CreateCopyAnswer, DatabaseId, Epoch, FenceExpectation, FenceRequest, FenceResult, PeerId,
    ReceiverId, ReplicationPosition, RetireCopy, RetireCopyAnswer, SchemaSupport, TransferId,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::task::JoinSet;

const RECEIVER: &str = "019f0000-0000-7000-8000-000000000010";
const HOLDER: &str = "019f0000-0000-7000-8000-000000000901";

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct Observation {
    pub(super) waiting: usize,
    reads: u64,
    pub(super) attempts: u64,
    notifications: u64,
    read_gap_reached: bool,
    pub(super) calls: Vec<durable_steps::CallId>,
}

#[derive(Clone)]
pub(super) struct CopyClient {
    http: reqwest::Client,
    base: String,
    copy: ReceiverId,
}
#[derive(Default)]
struct Caller {
    tasks: JoinSet<ConformanceResult<Value>>,
    response: Option<Value>,
}
impl CopyClient {
    pub(super) fn new(base: &str) -> ConformanceResult<Self> {
        Ok(Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .build()?,
            base: base.into(),
            copy: ReceiverId::try_from(RECEIVER.to_owned())?,
        })
    }
    async fn post(&self, route: &str, body: Value) -> ConformanceResult<Value> {
        Ok(self
            .http
            .post(format!("{}/admin/v1/conformance/{route}", self.base))
            .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }
    pub(super) async fn invoke(&self, request: Value) -> ConformanceResult<Value> {
        self.post(
            "copy-call",
            json!({"attempt":durable_steps::Uuid::now_v7(),"copy":self.copy,"request":request}),
        )
        .await
    }
    pub(super) async fn status(&self, operation: OperationId) -> ConformanceResult<Value> {
        self.post("copy-status", serde_json::to_value(operation)?)
            .await
    }
    pub(super) async fn control(
        &self,
        database: DatabaseId,
        kind: &str,
    ) -> ConformanceResult<Observation> {
        Ok(serde_json::from_value(
            self.post(
                "copy-wait",
                json!({"copy":self.copy,"database":database,"control":{"kind":kind}}),
            )
            .await?,
        )?)
    }
    pub(super) async fn observed(
        &self,
        database: DatabaseId,
        ready: impl Fn(&Observation) -> bool,
    ) -> ConformanceResult<Observation> {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let observed = self.control(database, "observe").await?;
                if ready(&observed) {
                    return Ok(observed);
                }
                // This observer has no mutation or drive. Its next authoritative
                // snapshot establishes readiness, never the backoff itself.
                // The next authoritative response, not a pacing timer, advances this observer.
            }
        })
        .await?
    }
    pub(super) async fn completed(
        &self,
        mut reply: Value,
        expected: Value,
    ) -> ConformanceResult<()> {
        if reply == expected {
            return Ok(());
        }
        let operation = pending_operation(&reply)?;
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                reply = self.status(operation).await?;
                if reply == expected { return Ok(()); }
                if reply.get("kind").and_then(Value::as_str) != Some("pending") {
                    return Err(format!("copy operation did not return its typed answer: {reply}; expected {expected}").into());
                }
                // The next authoritative response, not a pacing timer, advances this observer.
            }
        }).await?
    }
    async fn below_p(
        &self,
        caller: &mut Caller,
        database: DatabaseId,
        ready: impl Fn(&Observation) -> bool,
    ) -> ConformanceResult<Observation> {
        loop {
            if caller.response.is_some() {
                return self.observed(database, ready).await;
            }
            tokio::select! {
                observed = self.observed(database, &ready) => return observed,
                ended = reply(caller) => {
                    let ended = ended?;
                    // Another drive may own the operation, so HTTP can return
                    // Pending before its wait. Preserve that response for real
                    // journal readback; only a terminal answer violates P.
                    if ended.get("kind").and_then(Value::as_str) != Some("pending") {
                        return Err(format!("healthy below-P copy wait ended before its condition: {ended}").into());
                    }
                    caller.response = Some(ended);
                }
            }
        }
    }
    async fn pending_reach(
        &self,
        operation: OperationId,
        reach: CallReach,
    ) -> ConformanceResult<StepRecord> {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let status = self.status(operation).await?;
                let _ = pending_operation(&status)?;
                if let Some(value) = status.pointer("/value/step").filter(|value| !value.is_null()) {
                    let step: StepRecord = serde_json::from_value(value.clone())?;
                    if matches!(&step.state, StepState::CallPending { attempt, .. } if attempt.reach == reach) {
                        return Ok(step);
                    }
                }
                // The next authoritative response, not a pacing timer, advances this observer.
            }
        }).await?
    }
    pub(super) async fn create(&self, database: DatabaseId) -> ConformanceResult<()> {
        let request = CreateCopy {
            database,
            epoch: Epoch::new(1)?,
            holder: Some(PeerId::try_from(HOLDER.to_owned())?),
            origin: CopyOrigin::Empty,
        };
        let reply = self
            .invoke(json!({"kind":"create","value":request}))
            .await?;
        self.completed(reply, expected("created", CreateCopyAnswer::CopyCreated))
            .await
    }
    pub(super) async fn retire(&self, database: DatabaseId, fence: Epoch) -> ConformanceResult<()> {
        let request = RetireCopy {
            database,
            expect_fence: fence,
            expect_acked: position(0)?,
        };
        let reply = self
            .invoke(json!({"kind":"retire","value":request}))
            .await?;
        self.completed(reply, expected("retired", RetireCopyAnswer::Retired))
            .await
    }
    fn start_wait(&self, caller: &mut Caller, database: DatabaseId, through: ReplicationPosition) {
        let client = self.clone();
        caller.tasks.spawn(async move { client.invoke(json!({"kind":"acknowledged","value":{"database":database,"request":AwaitAcked { through }}})).await });
    }
}
pub(super) fn expected(kind: &str, answer: impl Serialize) -> Value {
    json!({"kind":"completed","value":{"kind":kind,"value":answer}})
}
pub(super) fn position(commit: u64) -> ConformanceResult<ReplicationPosition> {
    Ok(ReplicationPosition::new(
        Epoch::new(1)?,
        CommitSequence::new(commit),
    ))
}
pub(super) fn database(suffix: &str) -> ConformanceResult<DatabaseId> {
    Ok(DatabaseId::try_from(format!(
        "019f0000-0000-7000-8000-000000000{suffix}"
    ))?)
}
fn pending_operation(reply: &Value) -> ConformanceResult<OperationId> {
    if reply.get("kind").and_then(Value::as_str) != Some("pending") {
        return Err(format!("copy call completed before its condition: {reply}").into());
    }
    Ok(serde_json::from_value(
        reply
            .pointer("/value/operation")
            .cloned()
            .ok_or("missing caller operation")?,
    )?)
}
async fn reply(caller: &mut Caller) -> ConformanceResult<Value> {
    if let Some(response) = caller.response.take() {
        return Ok(response);
    }
    tokio::time::timeout(Duration::from_secs(8), caller.tasks.join_next())
        .await?
        .ok_or("copy caller task disappeared")??
}

pub async fn run(base: &str) -> ConformanceResult<()> {
    let client = CopyClient::new(base)?;
    let mut caller = Caller::default();
    let result = cases(&client, &mut caller).await;
    // Unblock a deliberately held read even when an assertion or command
    // fails, before asking the real native server to drain its requests.
    if result.is_err() {
        let _ = client.control(database("906")?, "releaseReadGap").await;
    }
    caller.tasks.abort_all();
    while caller.tasks.join_next().await.is_some() {}
    result
}

async fn cases(client: &CopyClient, caller: &mut Caller) -> ConformanceResult<()> {
    let uninitialized = database("902")?;
    client.start_wait(caller, uninitialized, position(0)?);
    let operation = pending_operation(&reply(caller).await?)?;
    let step = client.pending_reach(operation, CallReach::NoEffect).await?;
    let StepState::CallPending { attempt, .. } = step.state else {
        return Err("uninitialized caller did not retain a pending call".into());
    };
    let reason = attempt
        .retry_reason
        .ok_or("uninitialized caller lost its retry reason")?;
    if attempt.reach != CallReach::NoEffect
        || CopyUnavailableReason::decode_retry(&reason.code, &reason.detail)?
            != CopyUnavailableReason::Uninitialized
    {
        return Err("true uninitialized copy was not a typed safe retry".into());
    }
    client.create(uninitialized).await?;
    client
        .completed(
            client.status(operation).await?,
            expected(
                "acknowledged",
                AwaitAckedAnswer::Acked {
                    through: position(0)?,
                },
            ),
        )
        .await?;
    println!("COPY_WAIT_CASE uninitialized_is_retryable_until_real_creation passed");

    let fenced = database("903")?;
    client.create(fenced).await?;
    client.start_wait(caller, fenced, position(1)?);
    let first = client
        .below_p(caller, fenced, |state| {
            state.waiting == 1 && state.reads >= 1
        })
        .await?;
    let request = FenceRequest {
        database: fenced,
        transfer_id: TransferId::try_from("019f0000-0000-7000-8000-000000000904".to_owned())?,
        new_epoch: Epoch::new(2)?,
        expect: FenceExpectation {
            epoch: Epoch::new(1)?,
            acked: position(0)?,
        },
        target_schema: Some(SchemaSupport { max_version: 0 }),
        holder: PeerId::try_from(HOLDER.to_owned())?,
    };
    let answer = client
        .invoke(json!({"kind":"fence","value":request}))
        .await?;
    client
        .completed(
            answer,
            expected(
                "fenced",
                FenceResult::Fenced {
                    old_epoch: Epoch::new(1)?,
                    new_epoch: Epoch::new(2)?,
                    start: position(0)?,
                },
            ),
        )
        .await?;
    let reread = client
        .below_p(caller, fenced, |state| state.reads > first.reads)
        .await?;
    if reread.attempts != 1 || reread.waiting != 1 || reread.notifications <= first.notifications {
        return Err("fence failed to reread and retain the same below-P wait before expiry".into());
    }
    client.retire(fenced, Epoch::new(2)?).await?;
    client
        .completed(
            reply(caller).await?,
            expected("acknowledged", AwaitAckedAnswer::Retired),
        )
        .await?;
    if client.control(fenced, "observe").await?.waiting != 0 {
        return Err("retired copy leaked a waiter".into());
    }
    println!("COPY_WAIT_CASE fence_signals_without_advancing_ack_and_retire_ends_the_wait passed");

    let expiring = database("905")?;
    client.create(expiring).await?;
    client.start_wait(caller, expiring, position(1)?);
    client
        .observed(expiring, |state| state.attempts == 1 && state.waiting == 1)
        .await?;
    let operation = pending_operation(&reply(caller).await?)?;
    let step = client
        .pending_reach(operation, CallReach::MayHaveReached)
        .await?;
    let StepState::CallPending { attempt, .. } = step.state else {
        return Err("expired wait lost the pending durable call".into());
    };
    if attempt.reach != CallReach::MayHaveReached || attempt.retry_reason.is_some() {
        return Err("expired wait fabricated no-effect certainty or a typed refusal".into());
    }
    let retried = client
        .observed(expiring, |state| state.attempts >= 2 && state.waiting == 1)
        .await?;
    if retried.attempts != 2
        || retried.calls.len() != 2
        || retried.calls.first() != retried.calls.get(1)
    {
        return Err("copy wait retry overlapped or skipped an observed attempt".into());
    }
    client.retire(expiring, Epoch::new(1)?).await?;
    client
        .completed(
            client.status(operation).await?,
            expected("acknowledged", AwaitAckedAnswer::Retired),
        )
        .await?;
    let finished = client.control(expiring, "observe").await?;
    if finished.attempts != 2 || finished.waiting != 0 {
        return Err("copy retry leaked an obsolete waiter or duplicated an attempt".into());
    }
    println!(
        "COPY_WAIT_CASE expiry_is_answer_lost_and_the_real_alarm_retries_without_stale_waiters passed"
    );

    let gap = database("906")?;
    client.create(gap).await?;
    client.control(gap, "armReadGap").await?;
    client.start_wait(caller, gap, position(1)?);
    client.observed(gap, |state| state.read_gap_reached).await?;
    // The stored status has been read below P and its transaction has ended.
    // Commit the terminal change before allowing that old read to return.
    client.retire(gap, Epoch::new(1)?).await?;
    let committed_in_gap = client.control(gap, "observe").await?;
    if committed_in_gap.reads != 1 || committed_in_gap.attempts != 1 {
        return Err("copy read gap was not held at the chosen interleaving".into());
    }
    client.control(gap, "releaseReadGap").await?;
    client
        .completed(
            reply(caller).await?,
            expected("acknowledged", AwaitAckedAnswer::Retired),
        )
        .await?;
    let finished = client.control(gap, "observe").await?;
    if finished.attempts != 1 || finished.reads != 2 || finished.waiting != 0 {
        return Err("the commit in the read/wait gap lost its only signal or left a waiter".into());
    }
    println!(
        "COPY_WAIT_CASE retire_commits_in_the_causally_held_read_wait_gap_without_losing_its_signal passed"
    );
    Ok(())
}
