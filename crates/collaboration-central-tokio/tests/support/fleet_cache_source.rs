//! Cache component inputs come from the actual async Fleet owner, never a Router authority.
use super::native_source::Fixture;
use board_sync_protocol::{BoardReadPage, FleetFeedRead};
use fleet_admission::{FleetFeedEntry, FleetRequest, FleetRequestResult};
use message_board::{FleetPosition, Record, RecordRef, RequestDecision};
use message_board_storage::FleetRecordSnapshot;
use serde_json::{Value, json};
use std::path::Path;

pub type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
pub struct NativeFleetSource(pub(super) Fixture);
pub struct FeedFragment {
    pub entries: Vec<FleetFeedEntry>,
    pub covered_through: FleetPosition,
    pub high_water: FleetPosition,
}
impl NativeFleetSource {
    pub async fn open(root: &Path) -> TestResult<Self> {
        Ok(Self(Fixture::open(root).await?))
    }
    pub async fn decide(&self, request: &FleetRequest) -> TestResult<RequestDecision> {
        match self.0.fleet_request(request).await? {
            FleetRequestResult::Decided(decision @ RequestDecision::Accepted { .. }) => {
                Ok(decision)
            }
            _ => Err("native cache source did not accept its metadata request".into()),
        }
    }
    async fn read(&self, request: Value) -> TestResult<BoardReadPage> {
        let bytes = self.0.runtime.on_call(serde_json::to_vec(&json!({
            "source":{"kind":"connection","value":"550e8400-e29b-41d4-a716-4466554400a9"},"sourceFace":"session","authority":{"kind":"object"},
            "destination":{"kind":"fleet"},"destinationFace":"fleet",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"cache_source_read"},
            "kind":"central.fleet-read","target":null,"request":request
        }))?).await.map_err(|error|format!("native cache source read: {error:?}"))?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub async fn snapshot(&self) -> TestResult<FleetRecordSnapshot> {
        let mut cursor = None;
        let mut records = Vec::<Record>::new();
        let through = loop {
            let BoardReadPage::FleetSnapshot {
                records: page,
                next,
                snapshot_at,
                ..
            } = self
                .read(json!({"kind":"snapshot","value":{"cursor":cursor}}))
                .await?
            else {
                return Err("native cache source snapshot refused".into());
            };
            for payload in page {
                records.push(payload.decode()?);
            }
            cursor = next;
            if cursor.is_none() {
                break snapshot_at;
            }
        };
        // Cache snapshots expose kind/ID order; the server's cursor uses ID/kind order.
        let mut keyed = records
            .into_iter()
            .map(|record| {
                let reference = record.reference();
                Ok((
                    serde_json::to_string(&reference.kind)?,
                    reference.id.as_str().to_owned(),
                    record,
                ))
            })
            .collect::<TestResult<Vec<_>>>()?;
        keyed.sort_by(|left, right| (&left.0, &left.1).cmp(&(&right.0, &right.1)));
        Ok(FleetRecordSnapshot {
            through,
            records: keyed.into_iter().map(|(_, _, record)| record).collect(),
        })
    }
    #[allow(
        dead_code,
        reason = "The projection-only test binary shares this native source fixture."
    )]
    pub async fn record(&self, reference: &RecordRef) -> TestResult<Option<Record>> {
        Ok(self
            .snapshot()
            .await?
            .records
            .into_iter()
            .find(|record| &record.reference() == reference))
    }
    /// Partition a genuine native page at the cache input seam. This does not
    /// claim that the server offers a configurable record limit.
    pub async fn feed_fragment(
        &self,
        after: FleetPosition,
        limit: usize,
    ) -> TestResult<FeedFragment> {
        let BoardReadPage::FleetFeed(FleetFeedRead::Page { records, head, .. }) = self
            .read(json!({"kind":"feed","value":{"after":after}}))
            .await?
        else {
            return Err("native cache source feed refused".into());
        };
        let mut entries = records
            .into_iter()
            .map(|payload| {
                let record: Record = payload.decode()?;
                let position = record
                    .envelope()
                    .position
                    .fleet_position()
                    .ok_or("native feed record has no Fleet position")?;
                Ok(FleetFeedEntry { position, record })
            })
            .collect::<TestResult<Vec<_>>>()?;
        entries.truncate(limit);
        let covered_through = entries.last().map_or(after, |entry| entry.position);
        Ok(FeedFragment {
            entries,
            covered_through,
            high_water: head,
        })
    }
    pub async fn close(self) -> TestResult {
        Ok(self.0.objects.close().await?)
    }
}
