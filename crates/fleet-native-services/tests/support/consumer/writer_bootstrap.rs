use crate::consumer_fixture::{SampleProtocol, SampleSchema, TestResult};
use fleet_project_lease::LeasePolicy;
use fleet_project_writer::{BootstrapGrant, ReplicatedDatabase};
use fleet_protocol::*;
use fleet_session_client::SessionRequests;
use runtime_primitives::{Clock, SystemClock};
use std::path::Path;

pub(crate) async fn open(
    path: &Path,
    database: DatabaseId,
    requests: &SessionRequests<SampleProtocol>,
    policy: LeasePolicy,
    schema: SampleSchema,
) -> TestResult<ReplicatedDatabase<SampleSchema>> {
    let source = match requests
        .request(OperationRequest::Replication(
            ReplicationRequest::CopyStatus { database },
        ))
        .await?
    {
        OperationAnswer::Replication(ReplicationAnswer::CopyStatus(CopyStatusAnswer::Status(
            status,
        ))) => status,
        _ => return Err("bootstrap source status unavailable".into()),
    };
    let request_started = SystemClock.now();
    let lease = match requests
        .request(OperationRequest::Lease(LeaseRequest::Renew(RenewRequest {
            attempt: RequestId::new(durable_steps::Uuid::now_v7())?,
            database,
            epoch: source.fence,
        })))
        .await?
    {
        OperationAnswer::Lease(LeaseAnswer::Renew(RenewAnswer::Renewed { current, .. })) => current,
        _ => return Err("bootstrap writer grant unavailable".into()),
    };
    Ok(ReplicatedDatabase::seeding(
        path,
        schema,
        BootstrapGrant {
            lease,
            request_started,
            start: source.start,
        },
        policy,
    )
    .await?)
}
