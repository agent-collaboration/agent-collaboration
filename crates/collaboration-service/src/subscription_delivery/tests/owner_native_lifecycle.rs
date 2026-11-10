//! Release every worker reference before the fixture closes or reopens the native owner.
use super::super::local_mail_proofs::native_mail_owner::NativeMailOwner;
use super::*;

impl OwnerRuntime {
    pub async fn await_mail_outcome(
        &self,
        push: &collaboration_protocol::PushId,
    ) -> fleet_sync_protocol::RecipientOutcome<collaboration_mail_protocol::CarrierAcceptance> {
        let item = fleet_sync_protocol::ItemId::new(uuid::Uuid::parse_str(push.as_str()).unwrap())
            .unwrap();
        (async {
            loop {
                if let Some(outcome) = self.local_mail_outcome(item).await
                    && !matches!(outcome, fleet_sync_protocol::RecipientOutcome::Held)
                {
                    break outcome;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
    }

    pub async fn local_mail_outcome(
        &self,
        item: fleet_sync_protocol::ItemId,
    ) -> Option<fleet_sync_protocol::RecipientOutcome<collaboration_mail_protocol::CarrierAcceptance>>
    {
        self.native
            .as_ref()
            .expect("native fixture")
            .runtime()
            .local_outcome_for_test(item)
            .await
            .unwrap()
    }

    pub fn drive_native_due(
        &self,
    ) -> tokio::task::JoinHandle<Result<(), durable_steps::StoreError>> {
        let runtime = self
            .native
            .as_ref()
            .expect("native fixture")
            .runtime_handle();
        tokio::spawn(async move { runtime.drive_due_for_test().await })
    }
}

impl OwnerFixture {
    /// Stand in for Fleet's lease renewals while a subscription-only proof advances days.
    /// Each renewal precedes its clock step; the final step has no later lease write.
    pub async fn advance_while_renewing_native_lease(&self, duration: std::time::Duration) {
        use fleet_sync_protocol::{DatabaseId, LeaseState, LeaseStatus, PeerId};
        let mut remaining = duration;
        let step_limit = board_fleet_sync::BOARD_LEASE_POLICY.lease_duration / 2;
        while !remaining.is_zero() {
            let database = self.registry.database(&self.project).await.unwrap();
            let epoch = match database.writer_state().await.unwrap() {
                fleet_sync_lease::WriterState::Writing { epoch }
                | fleet_sync_lease::WriterState::Paused { epoch, .. } => epoch,
                state => panic!("subscription fixture lost its native authority: {state:?}"),
            };
            let previous = LeaseStatus {
                database: DatabaseId::try_from(self.project.as_str().to_owned()).unwrap(),
                epoch,
                holder: PeerId::try_from("0192a1b2-c3d4-7000-8000-000000000001".to_owned())
                    .unwrap(),
                state: LeaseState::Held,
                expires_at: runtime_primitives::Clock::now(self.clock.as_ref()),
                remaining_ms: 0,
            };
            crate::test_project_runtime::renew_lease(
                &self.registry,
                &self.project,
                previous,
                self.clock.as_ref(),
            )
            .await
            .unwrap();
            let step = remaining.min(step_limit);
            self.clock.advance(step).unwrap();
            remaining -= step;
        }
    }

    pub async fn await_push_state(
        &self,
        push: &collaboration_protocol::PushId,
        state: collaboration_protocol::PushDeliveryState,
    ) -> collaboration_protocol::PushRecord {
        (async {
            loop {
                let record = self
                    .interactions
                    .get_push_record(push)
                    .await
                    .unwrap()
                    .unwrap();
                if record.delivery_state == state {
                    break record;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
    }

    pub async fn held_native_pushes(
        &self,
        count: usize,
    ) -> Vec<collaboration_protocol::PushRecord> {
        let target = super::super::super::subscription_push::target_session(&self.reader).unwrap();
        (async {
            loop {
                let held = self
                    .interactions
                    .list_held_mail_push_records(&target)
                    .await
                    .unwrap();
                if held.len() == count {
                    break held;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
    }

    pub async fn public_subscription_view(
        &self,
        runtime: &OwnerRuntime,
    ) -> collaboration_protocol::ThreadSubscriptionView {
        let application = (crate::ApplicationFixture::new(SERVICE_ID, SERVICE_ID)
            .unwrap()
            .with_project_admission(self.admission.clone())
            .with_subscription_delivery_service(runtime.service.clone(), self.presence.clone()))
        .build();
        let mut rows = application
            .board()
            .thread_subscriptions(collaboration_protocol::ThreadSubscriptionsRequest {
                actor: self.reader.clone(),
            })
            .await
            .unwrap()
            .subscriptions;
        assert_eq!(rows.len(), 1);
        rows.pop().unwrap()
    }

    pub async fn queued_runtime(&self) -> OwnerRuntime {
        let runtime = self.prepare_runtime_with_availability(
            self.clock.clone(),
            collaboration_protocol::SessionReachability::ProviderAcp,
            BoardAvailability::Available(self.store.clone()),
        );
        self.start_native_runtime(runtime).await
    }

    pub async fn native_runtime(&self) -> OwnerRuntime {
        self.start_native_runtime(self.prepared_runtime()).await
    }

    pub async fn native_runtime_with_clock(
        &self,
        clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> OwnerRuntime {
        let runtime = self.prepare_runtime_with_availability(
            clock,
            collaboration_protocol::SessionReachability::CodexAppServer,
            BoardAvailability::Available(self.store.clone()),
        );
        self.start_native_runtime(runtime).await
    }

    pub async fn native_runtime_without_board(&self) -> OwnerRuntime {
        let runtime = self.prepare_runtime_with_availability(
            self.clock.clone(),
            collaboration_protocol::SessionReachability::CodexAppServer,
            BoardAvailability::Unavailable,
        );
        self.start_native_runtime(runtime).await
    }

    async fn start_native_runtime(&self, mut runtime: OwnerRuntime) -> OwnerRuntime {
        let (application, native, publisher) =
            super::super::local_mail_proofs::native_follow_mail_proofs::follow_application(
                self, &runtime,
            )
            .await;
        runtime.publisher.shutdown().await;
        runtime.publisher = publisher;
        runtime.native = Some(native);
        runtime.application = Some(application);
        runtime.service.start().await.unwrap();
        runtime
    }

    pub fn into_directory(self) -> TempDir {
        self.directory
    }

    pub async fn close_native(self, native: NativeMailOwner) {
        let _directory = self.into_directory();
        native.close().await.unwrap();
    }

    pub async fn reopen_native(self, native: NativeMailOwner) -> Self {
        self.reopen_message_storage(Some(native)).await
    }

    pub async fn reopen_project_and_native(self, native: NativeMailOwner) -> Self {
        self.reopen_native(native)
            .await
            .reopen_project_storage()
            .await
    }

    pub async fn reopen_project_storage(self) -> Self {
        let mut fixture = self;
        drop(fixture.store);
        drop(fixture.registry);
        Arc::try_unwrap(fixture.admission)
            .unwrap_or_else(|_| panic!("fixture retained the project admission owner"))
            .close()
            .await
            .unwrap();
        // C-only seeding uses an SDK-owned system clock. Preserve its native domain until
        // that constructor can take the subscription driver's injected clock too.
        let native_clock: Arc<dyn router_clock::RuntimeClock> =
            Arc::new(router_clock::SystemRuntimeClock::new());
        let registry =
            crate::test_project_runtime::registry(fixture.directory.path(), native_clock.clone())
                .unwrap();
        let admission = Arc::new(
            ProjectAdmission::new(ProjectAdmissionProps {
                clock: native_clock,
                registry: registry.clone(),
                fleet: Arc::new(FleetClient::new(
                    &message_board_storage::FleetStore::open(
                        &(fixture.directory.path())
                            .join(message_board_storage::FleetStore::FILE_NAME),
                    )
                    .await
                    .unwrap(),
                    Arc::new(crate::test_project_runtime::PendingRegistration),
                    None,
                )),
                identity: collaboration_contracts::MachineIdentity::new(
                    collaboration_protocol::UuidIdentity::try_from(SERVICE_ID.to_owned()).unwrap(),
                    Some("Owner-test-machine"),
                )
                .unwrap(),
            })
            .unwrap(),
        );
        fixture.store = Arc::new(Mutex::new(
            admission.subscription_window_store(fixture.interactions.clone()),
        ));
        fixture.registry = registry;
        fixture.admission = admission;
        fixture
    }

    pub async fn reopen_messages(self) -> Self {
        self.reopen_message_storage(None).await
    }

    async fn reopen_message_storage(self, native: Option<NativeMailOwner>) -> Self {
        let Self {
            directory,
            store,
            interactions,
            admission,
            registry,
            automation,
            reader,
            project,
            discussion_subject,
            root,
            clock,
            presence,
            held_mail,
        } = self;
        drop(store);
        if let Some(native) = native {
            drop(interactions);
            native.close().await.unwrap();
        } else {
            Arc::try_unwrap(interactions)
                .unwrap_or_else(|_| panic!("fixture retained the native message owner"))
                .close()
                .await
                .unwrap();
        }
        let interactions = Arc::new(
            interaction_storage::InteractionStore::open(
                directory.path().join("interaction.sqlite"),
            )
            .await
            .unwrap(),
        );
        let store = Arc::new(Mutex::new(
            admission.subscription_window_store(interactions.clone()),
        ));
        Self {
            directory,
            store,
            interactions,
            admission,
            registry,
            automation,
            reader,
            project,
            discussion_subject,
            root,
            clock,
            presence,
            held_mail,
        }
    }
}
