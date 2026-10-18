#![allow(dead_code)]
use super::registry_support::*;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;

#[derive(Clone, Debug)]
pub struct PendingFleet {
    pub call_id: ObjectCallId,
    pub call: FleetCall,
}

pub fn fleet_dispatches(effects: &[WireEffect]) -> Vec<PendingFleet> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            WireEffect::Dispatch {
                call_id,
                to: ObjectAddress::Fleet,
                call: WireCall::Fleet(call),
                carry: None,
            } => Some(PendingFleet {
                call_id: call_id.clone(),
                call: call.clone(),
            }),
            _ => None,
        })
        .collect()
}

pub fn only_fleet(effects: &[WireEffect]) -> PendingFleet {
    let calls = fleet_dispatches(effects);
    assert_eq!(calls.len(), 1, "one Fleet dispatch: {effects:?}");
    calls.into_iter().next().expect("dispatch")
}

impl PendingFleet {
    pub fn answer(&self, harness: &mut Harness) -> FleetResult {
        let WireResult::Fleet(result) = harness.call("fleet", WireCall::Fleet(self.call.clone()))
        else {
            panic!("Fleet answer");
        };
        result
    }
    pub fn deliver(&self, harness: &mut Harness, result: FleetResult) -> Vec<WireEffect> {
        harness.run(
            "registry",
            WireInput::CallResult {
                source: CallerKind::Registry,
                call_id: self.call_id.clone(),
                result: Box::new(WireResult::Fleet(result)),
                carry: None,
            },
        )
    }
    pub fn update(&self) -> DirectoryUpdate {
        let FleetCall::DirectoryUpdate(update) = &self.call else {
            panic!("lease update: {:?}", self.call);
        };
        update.clone()
    }
    pub fn allocation_floor(&self) -> u64 {
        let FleetCall::AdvanceLeaseIncarnation { at_least } = self.call else {
            panic!("allocation");
        };
        at_least
    }
}

pub fn begin_renew(harness: &mut Harness, attempt: u64) -> Vec<WireEffect> {
    harness
        .begin(
            "registry",
            WireCall::Registry(RegistryCall::Renew {
                installation: machine(1),
                attempt: request(attempt),
                project: project(),
                epoch: epoch(1),
            }),
        )
        .1
}

pub fn alarm(harness: &mut Harness) -> Vec<WireEffect> {
    harness.run(
        "registry",
        WireInput::Alarm {
            face: CallerKind::Registry,
        },
    )
}

pub fn projection(harness: &mut Harness) -> DirectoryUpdate {
    let record: String = harness
        .store("fleet")
        .connection
        .query_row(
            "SELECT record FROM directory_lease WHERE project=?",
            [project().as_str()],
            |row| row.get(0),
        )
        .expect("stored lease projection");
    serde_json::from_str(&record).expect("valid lease projection")
}

pub fn version(update: &DirectoryUpdate) -> LeaseVersion {
    match update {
        DirectoryUpdate::Lease { lease_version, .. }
        | DirectoryUpdate::ClaimGranted { lease_version, .. } => *lease_version,
        _ => panic!("lease version"),
    }
}

pub fn publication_count(harness: &mut Harness) -> i64 {
    harness
        .store("registry")
        .connection
        .query_row(
            "SELECT count(*) FROM _registry_lease_publications",
            [],
            |row| row.get(0),
        )
        .expect("publication count")
}

pub fn fence(harness: &mut Harness) -> u64 {
    let value: String = harness
        .store("fleet")
        .connection
        .query_row(
            "SELECT incarnation FROM lease_incarnation_fence",
            [],
            |row| row.get(0),
        )
        .expect("durable fence");
    value.parse().expect("valid fence")
}

pub fn lease_update(
    epoch_number: u64,
    incarnation: u64,
    change: u64,
    minute: i64,
) -> DirectoryUpdate {
    DirectoryUpdate::lease(
        project(),
        DirectoryLease::Held {
            holder: machine(1),
            state: LeaseState::Held,
            expires_at: at(minute + 240).0,
        },
        LeaseVersion::new(
            epoch(epoch_number),
            RegistryIncarnation::new(incarnation).expect("incarnation"),
            change,
        )
        .expect("version"),
        at(minute),
    )
    .expect("update")
}
