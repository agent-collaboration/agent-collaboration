#![allow(clippy::expect_used, clippy::panic, dead_code)]
#[path = "../support/mod.rs"]
pub mod sql_support;
use chrono::{DateTime, TimeDelta, Utc};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use sql_support::SqlTxFake;
use std::collections::BTreeMap;
pub fn identity(n: u64) -> String {
    format!("019f0000-0000-7000-8000-{n:012x}")
}
pub fn apply_call(piece: Piece) -> WireCall {
    WireCall::Copy(CopyCall::ApplyPiece {
        piece,
        sender: machine(1),
    })
}
pub fn project() -> ProjectId {
    ProjectId::new(identity(1)).expect("project")
}
pub fn machine(n: u64) -> InstallationId {
    InstallationId::new(identity(10 + n)).expect("machine")
}
pub fn connection(n: u64) -> ConnectionId {
    ConnectionId::new(identity(20 + n)).expect("connection")
}
pub fn session(n: u64) -> SessionId {
    SessionId::new(identity(30 + n)).expect("session")
}
pub fn transfer(n: u64) -> TransferId {
    TransferId::new(identity(40 + n)).expect("transfer")
}
pub fn request(n: u64) -> RequestId {
    RequestId::new(identity(50 + n)).expect("request")
}
pub fn epoch(n: u64) -> Epoch {
    Epoch::new(n).expect("epoch")
}
pub fn position(e: u64, c: u64) -> ReplicationPosition {
    ReplicationPosition::new(epoch(e), c.into())
}
pub fn start() -> RegistryTime {
    RegistryTime(
        DateTime::parse_from_rfc3339("2026-10-12T12:00:00Z")
            .expect("time")
            .with_timezone(&Utc),
    )
}
pub fn at(minutes: i64) -> RegistryTime {
    RegistryTime(start().0 + TimeDelta::minutes(minutes))
}
pub fn admin(request: AdminRequest) -> WireCall {
    WireCall::Registry(RegistryCall::Admin {
        authority: AdminAuthority::Admin {
            subject: "owner".into(),
        },
        request,
    })
}
pub struct Harness {
    pub stores: BTreeMap<String, SqlTxFake>,
    pub now: RegistryTime,
    next_id: u64,
    pub registry_run: RegistryRunId,
}
impl Harness {
    pub fn new() -> Self {
        Self {
            stores: BTreeMap::new(),
            now: start(),
            next_id: 100,
            registry_run: RegistryRunId::new(identity(90_000)).expect("registry run"),
        }
    }
    pub fn store(&mut self, address: &str) -> &mut SqlTxFake {
        self.stores
            .entry(address.into())
            .or_insert_with(SqlTxFake::new)
    }
    pub fn run(&mut self, address: &str, input: WireInput) -> Vec<WireEffect> {
        self.next_id += 32;
        let ids = IdBatch {
            registry_run: Some(self.registry_run.clone()),
            call_ids: (self.next_id..self.next_id + 32)
                .map(|n| ObjectCallId::new(identity(n)).expect("call id"))
                .collect(),
        };
        let now = self.now;
        let tx = self.store(address);
        tx.connection
            .execute_batch("BEGIN IMMEDIATE")
            .expect("begin");
        let result = step(tx, input, now, ids);
        tx.connection
            .execute_batch(if result.is_ok() { "COMMIT" } else { "ROLLBACK" })
            .expect("finish");
        result.expect("core step").into_trace()
    }
    pub fn restart_registry(&mut self) {
        self.next_id += 1;
        self.registry_run =
            RegistryRunId::new(identity(90_000 + self.next_id)).expect("new registry run");
    }
    pub fn input(&mut self, call: WireCall) -> WireInput {
        self.next_id += 1;
        WireInput::Call {
            from: ObjectAddress::Registry,
            call_id: ObjectCallId::new(identity(self.next_id)).expect("id"),
            call: Box::new(call),
        }
    }
    pub fn begin(&mut self, address: &str, call: WireCall) -> (WireInput, Vec<WireEffect>) {
        let input = self.input(call);
        let effects = self.run(address, input.clone());
        (input, effects)
    }
    pub fn target(to: ObjectAddress) -> String {
        match to {
            ObjectAddress::Registry => "registry".into(),
            ObjectAddress::Copy(_) => "copy".into(),
            ObjectAddress::Connection(i) => format!("session:{}", i.as_str()),
            ObjectAddress::Fleet => "fleet".into(),
        }
    }
    pub fn finish(
        &mut self,
        address: &str,
        input: &WireInput,
        mut effects: Vec<WireEffect>,
    ) -> (WireResult, Vec<WireEffect>) {
        let mut trace = effects.clone();
        loop {
            let dispatches: Vec<_> = effects
                .into_iter()
                .filter_map(|e| match e {
                    WireEffect::Dispatch {
                        call_id,
                        to,
                        call,
                        carry,
                    } => Some((call_id, to, call, carry)),
                    _ => None,
                })
                .collect();
            if dispatches.is_empty() {
                break;
            }
            effects = Vec::new();
            for (call_id, to, call, carry) in dispatches {
                let target = Self::target(to);
                let result = self.call(&target, call);
                let next = self.run(
                    address,
                    WireInput::CallResult {
                        source: match input {
                            WireInput::Call { call, .. } => call.kind(),
                            _ => panic!("call input"),
                        },
                        carry,
                        call_id,
                        result: Box::new(result),
                    },
                );
                trace.extend(next.clone());
                effects.extend(next);
            }
        }
        validate_call_returns(input, &trace).expect("exactly one Return across invocation");
        let result = trace
            .iter()
            .find_map(|e| match e {
                WireEffect::Return { result, .. } => Some(result.clone()),
                _ => None,
            })
            .expect("Return");
        (result, trace)
    }
    pub fn call(&mut self, address: &str, call: WireCall) -> WireResult {
        let (input, effects) = self.begin(address, call);
        self.finish(address, &input, effects).0
    }
    pub fn enroll(&mut self, n: u64) {
        assert_eq!(
            self.call(
                "registry",
                admin(AdminRequest::EnrollMachine {
                    label: "test machine".into(),
                    installation: machine(n),
                    access_client_id: AccessClientId::new(format!("client-{n}")).expect("client"),
                    credential_hash: format!("hash-{n}"),
                    expires_at: at(1440).0
                })
            ),
            WireResult::Registry(RegistryResult::Admin(AdminResult::MachineEnrolled))
        );
    }
    pub fn register(&mut self, n: u64, max_version: u64) {
        assert!(matches!(
            self.call(
                "registry",
                WireCall::Registry(RegistryCall::RegisterSession(SessionRegistration {
                    installation: machine(n),
                    session_id: session(n),
                    connection: connection(n),
                    schema_support: SchemaSupport { max_version }
                }))
            ),
            WireResult::Registry(RegistryResult::Registered(
                RegistrationResult::Registered { .. }
            ))
        ));
    }
    pub fn setup() -> Self {
        let mut h = Self::new();
        h.enroll(1);
        h.enroll(2);
        h.register(1, 10);
        h.register(2, 10);
        assert!(
            matches!(h.call("registry",WireCall::Registry(RegistryCall::ReserveFirstEpoch{project:project(),source:ReservationSource::FleetAcceptance{installation:machine(1)}})),WireResult::Registry(RegistryResult::FirstLease(SessionAnswer::FirstLease{epoch:e,..})) if e==epoch(1))
        );
        h
    }
    pub fn renew(&mut self, n: u64, attempt: u64, e: u64) -> SessionAnswer {
        match self.call(
            "registry",
            WireCall::Registry(RegistryCall::Renew {
                installation: machine(n),
                attempt: request(attempt),
                project: project(),
                epoch: epoch(e),
            }),
        ) {
            WireResult::Registry(RegistryResult::Renew(answer)) => answer,
            _ => panic!("renew answer"),
        }
    }
    pub fn preview(&mut self) -> TakeoverPreview {
        match self.call(
            "registry",
            admin(AdminRequest::TakeoverPreview { project: project() }),
        ) {
            WireResult::Registry(RegistryResult::Admin(AdminResult::Preview(p))) => p,
            _ => panic!("preview"),
        }
    }
    pub fn takeover_call(&mut self, t: u64) -> WireCall {
        let p = self.preview();
        admin(AdminRequest::Takeover {
            transfer_id: transfer(t),
            project: project(),
            to_installation: machine(2),
            reason: "replace".into(),
            confirm_token: p.confirm_token,
        })
    }
    pub fn schema_piece(&mut self, commit: u64) {
        let p = Piece::new(PieceContents {
            project: project(),
            epoch: epoch(1),
            continues_from: position(1, 0),
            through: position(1, commit),
            transactions: vec![ReplicatedTransaction::SchemaStep {
                commit_id: commit.into(),
                migration_version: 1,
                statements: vec!["CREATE TABLE records(id INTEGER PRIMARY KEY)".into()],
            }],
            part: None,
            summary: Some(ProjectSummary {
                status: "active".into(),
                latest_record_at: self.now.0,
            }),
        })
        .expect("piece");
        assert_eq!(
            self.call("copy", apply_call(p)),
            WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Acked {
                through: position(1, commit)
            }))
        );
    }
}
