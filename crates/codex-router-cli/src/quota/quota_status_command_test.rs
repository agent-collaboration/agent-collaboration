use super::*;

#[test]
fn tui_weekly_floor_errors_are_closed_and_redacted() {
    let canaries = [
        "sensitive-account",
        "/private/router/state.sqlite",
        "internal sql diagnostic",
    ];
    for canary in canaries {
        let mapped = render_safe_weekly_floor_error(StateStoreError::Sqlite {
            message: canary.to_owned(),
        });
        assert_eq!(mapped, WeeklyQuotaFloorSaveError::StateOperationFailed);
        assert!(!format!("{mapped:?}").contains(canary));
    }
    assert_eq!(
        render_safe_weekly_floor_error(StateStoreError::WeeklyQuotaFloorDatabaseBusy),
        WeeklyQuotaFloorSaveError::DatabaseBusy
    );
    assert_eq!(
        render_safe_weekly_floor_error(StateStoreError::WeeklyQuotaFloorSchemaUpgradeRequired),
        WeeklyQuotaFloorSaveError::SchemaUpgradeRequired
    );
    assert_eq!(
        render_safe_weekly_floor_error(StateStoreError::WeeklyQuotaFloorAccountNotFound),
        WeeklyQuotaFloorSaveError::AccountNotFound
    );
}

#[tokio::test]
async fn interactive_weekly_floor_saver_persists_by_stable_account_id() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "agent-collaboration-tui-weekly-floor-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("temporary router root should be created");
    let database_path = root.join("state.sqlite");
    let account_id = AccountId::new("tui-floor-account").expect("account id should be valid");
    let state = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("state should open");
    state
        .upsert_account(&AccountRecord::new(
            codex_router_core::provider::Provider::Openai,
            account_id.clone(),
            "duplicate",
            AccountStatus::Enabled,
        ))
        .await
        .expect("account should persist");
    state.close().await.expect("state should close");

    weekly_quota_floor_saver(database_path.clone())(account_id.clone(), 15)
        .await
        .expect("TUI saver should persist 15 percent");

    let state = AsyncSqliteStateStore::open_read_only(&database_path)
        .await
        .expect("state should reopen read-only");
    let policies = state
        .list_account_routing_policies()
        .await
        .expect("policy should read");
    assert_eq!(policies.len(), 1);
    assert_eq!(policies[0].account_id(), &account_id);
    assert_eq!(
        policies[0].weekly_quota_floor_basis_points().basis_points(),
        1_500
    );
    state.close().await.expect("read-only state should close");
    std::fs::remove_dir_all(root).expect("temporary router root should be removed");
}

#[tokio::test]
async fn interactive_credit_policy_saver_reads_back_and_rejects_replaced_generation() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock should be after epoch")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "agent-collaboration-tui-credit-policy-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root).expect("temporary router root should be created");
    let database_path = root.join("state.sqlite");
    let account_id = AccountId::new("tui-credit-policy").expect("account id should be valid");
    let state = AsyncSqliteStateStore::open(&database_path)
        .await
        .expect("state should open and migrate before the mutation fixture");
    state
        .upsert_account(
            &AccountRecord::new(
                codex_router_core::provider::Provider::Openai,
                account_id.clone(),
                "credit policy",
                AccountStatus::Disabled,
            )
            .with_active_credential_generation(7),
        )
        .await
        .expect("account should persist");
    state.close().await.expect("state should close");

    let saver = credit_usage_policy_saver(database_path.clone());
    assert_eq!(
        saver(
            account_id.clone(),
            Some(7),
            codex_router_core::credit_usage::CreditUsagePolicy::Allow,
        )
        .await
        .expect("disabled account policy can save and read back"),
        codex_router_core::credit_usage::CreditUsagePolicy::Allow
    );
    assert_eq!(
        saver(
            account_id.clone(),
            Some(6),
            codex_router_core::credit_usage::CreditUsagePolicy::Disallow,
        )
        .await
        .expect_err("old credential pane cannot overwrite the saved policy"),
        CreditUsagePolicySaveError::TargetChanged
    );

    let reopened = AsyncSqliteStateStore::open_read_only(&database_path)
        .await
        .expect("saved policy should reopen read-only");
    assert_eq!(
        reopened
            .load_account_credit_usage_policy(&account_id)
            .await
            .expect("saved policy should read"),
        codex_router_core::credit_usage::CreditUsagePolicy::Allow
    );
    reopened
        .close()
        .await
        .expect("read-only state should close");
    std::fs::remove_dir_all(root).expect("temporary router root should be removed");
}
