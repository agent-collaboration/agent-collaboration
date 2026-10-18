use crate::{CallEdgeProbe, ConformanceClient, ConformanceResult, ControlRequest, ControlResult};
use collaboration_central_core::{CallEdgeRefused, CallerKind};
pub async fn run_call_edge_conformance(client: &ConformanceClient) -> ConformanceResult<()> {
    let result = client.control(ControlRequest::ProbeForbiddenEdge).await?;
    if !matches!(
        result,
        ControlResult::CallEdgeProbe(CallEdgeProbe::Refused(CallEdgeRefused {
            from: CallerKind::Copy,
            to: CallerKind::Registry
        }))
    ) {
        return Err("receiver accepted or misclassified a forged Copy -> Registry envelope".into());
    }
    println!(
        "DAG wire: forged Copy -> Registry refused with typed CallEdgeRefused before dispatch"
    );
    Ok(())
}
