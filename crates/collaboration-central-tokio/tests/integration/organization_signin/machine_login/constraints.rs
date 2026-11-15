//! Engine-only negative probes: planted rows are constraints, not business proof.
//! Carries historical engineRefusesSecondEnrolled / duplicate_request_or_client.
use super::support::*;
use central_storage::{
    BatchOutcome, ConstraintKind, GuardedStatement, RowExpectation, SqlValue, Statement,
};

const INSERT: &str = "INSERT INTO machine_approvals(approval_id,request_id,machine_id,label,allowed_workspaces,state,end_reason,approved_by_person_id,permission_checked,base_generation,code_expires_at,machine_client_id,last_grant_at,ended_at,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)";
fn row(
    pk: u16,
    request: String,
    machine: &str,
    state: &str,
    client: Option<String>,
    now: i64,
) -> Result<GuardedStatement> {
    let at = chrono::DateTime::from_timestamp(now, 0)
        .ok_or("constraint fixture time is outside the timestamp range")?
        .to_rfc3339();
    let deadline = chrono::DateTime::from_timestamp(
        now.checked_add(600)
            .ok_or("constraint fixture deadline overflows seconds")?,
        0,
    )
    .ok_or("constraint fixture deadline is outside the timestamp range")?
    .to_rfc3339();
    let ended = state == "ended";
    Ok(GuardedStatement {
        statement: Statement {
            sql: INSERT,
            args: vec![
                text(id(pk)),
                text(request),
                text(machine),
                text("Constraint fixture"),
                text("[]"),
                text(state),
                if ended { text("owner") } else { SqlValue::Null },
                text(signed::OWNER),
                text("machine:enroll"),
                number(0)?,
                text(deadline),
                client.map_or(SqlValue::Null, text),
                if state == "enrolled" {
                    text(&at)
                } else {
                    SqlValue::Null
                },
                if ended { text(&at) } else { SqlValue::Null },
                text(&at),
                text(at),
            ],
        },
        expect: RowExpectation::Exactly(1),
    })
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Actual engine Unique outcomes and unchanged rows must reject only the separately targeted constraint, then accept a valid full-row control."
)]
#[tokio::test]
async fn engine_refuses_second_enrolled_and_duplicate_request_or_client_with_fresh_primary_keys()
-> Result<()> {
    for constraint in ["one enrolled", "request identity", "client identity"] {
        let f = MachineFixture::new().await?;
        // The existing owner and active row are produced by real approval/Grant.
        let request_id = id(0xd301);
        let enrollment_id = enrollment(
            &f.post(
                DEVICE,
                &approval(0xd301, INSTALLATION, "Existing", &[WORKSPACE], f.now()),
            )
            .await?
            .1,
        )?;
        assert_eq!(
            f.post(TOKEN, &token(&enrollment_id, &enrollment_id))
                .await?
                .0,
            200
        );
        let machine_rows = f.rows("SELECT machine_id FROM machines").await?;
        let machine = match machine_rows.first().and_then(|row| row.first()) {
            Some(SqlValue::Text(value)) => value.clone(),
            _ => return Err("real machine owner absent".into()),
        };
        let (probe, control) = match constraint {
            // New PK/request/client are all distinct. Only the same machine's second
            // enrolled row hits the partial unique index; approved is its valid control.
            "one enrolled" => (
                row(
                    0xd310,
                    id(0xd311),
                    &machine,
                    "enrolled",
                    Some(id(0xd312)),
                    f.now(),
                )?,
                row(0xd313, id(0xd314), &machine, "approved", None, f.now())?,
            ),
            // An approved row has no client and is outside the enrolled index. Only
            // request_id repeats. Changing request_id makes the control admissible.
            "request identity" => (
                row(
                    0xd320,
                    request_id.clone(),
                    &machine,
                    "approved",
                    None,
                    f.now(),
                )?,
                row(0xd321, id(0xd322), &machine, "approved", None, f.now())?,
            ),
            // An otherwise valid ended row is outside the enrolled partial index, has
            // its own request and PK, and repeats only the existing machine client.
            _ => (
                row(
                    0xd330,
                    id(0xd331),
                    &machine,
                    "ended",
                    Some(enrollment_id.clone()),
                    f.now(),
                )?,
                row(
                    0xd332,
                    id(0xd333),
                    &machine,
                    "ended",
                    Some(id(0xd334)),
                    f.now(),
                )?,
            ),
        };
        let before = snapshot(&f).await?;
        let rejected = f.raw_batch(probe).await?;
        assert_eq!(
            rejected,
            BatchOutcome::ConstraintViolated {
                index: 0,
                kind: ConstraintKind::Unique
            },
            "{constraint}: a different failure is not the intended constraint proof"
        );
        assert!(
            snapshot(&f).await? == before,
            "{constraint}: rejected row changed owning state"
        );
        let accepted = f.raw_batch(control).await?;
        assert!(
            matches!(accepted, BatchOutcome::Applied { .. }),
            "{constraint}: independently valid control row did not apply"
        );
        assert_ne!(
            snapshot(&f).await?,
            before,
            "{constraint}: valid control inserted no row"
        );
        assert_eq!(
            f.rows("SELECT COUNT(*) FROM machine_approvals").await?,
            vec![vec![number(2)?]]
        );
        f.close().await?;
    }
    Ok(())
}
