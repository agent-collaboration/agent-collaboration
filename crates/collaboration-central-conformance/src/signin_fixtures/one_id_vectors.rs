//! Contract 1 vectors supplied by the sign-in lane, independent of central's codec.
use super::ConformanceResult;
use serde_json::{Value, json};

const MACHINE_ID: &str = "019a61bb-0000-7000-8000-000000000001";
const ORGANIZATION_ID: &str = "019a61bb-0000-7000-8000-000000000002";
const PERSON_ID: &str = "019a61bb-0000-7000-8000-000000000003";
const REQUEST_ID: &str = "019a61bb-0000-7000-8000-000000000004";
const ENROLLMENT_ID: &str = "019a61bb-0000-7000-8000-000000000005";
const V4_MACHINE_ID: &str = "019a61bb-0000-4000-8000-000000000001";

type RoundTrip = fn(Value) -> Option<Value>;

pub fn check_machine_identity_vectors(
    device_login: RoundTrip,
    presented_machine: RoundTrip,
    grant: RoundTrip,
    principal: RoundTrip,
) -> ConformanceResult<()> {
    for (name, convert, input, field, legacy) in [
        (
            "DecideDeviceLogin",
            device_login,
            json!({"machineId":MACHINE_ID,"organizationId":ORGANIZATION_ID,"approver":PERSON_ID,
                "permissionChecked":"machine:enroll","requestId":REQUEST_ID,"label":"Proof Mac",
                "allowedWorkspaces":[],"approverSecondFactorAt":null,"codeExpiresAt":2208989100_i64}),
            "machineId",
            "installation",
        ),
        (
            "PresentedMachine",
            presented_machine,
            json!({"machineId":MACHINE_ID}),
            "machineId",
            "peer",
        ),
        (
            "MachineTokenDecision grant",
            grant,
            json!({"result":"grant","machineId":MACHINE_ID,"enrollmentId":ENROLLMENT_ID,
                "decidedAt":2208988800_i64,"grantExpiresAt":2208989400_i64}),
            "machineId",
            "installation",
        ),
        (
            "JWT principal",
            principal,
            json!({"kind":"machine","machine_id":MACHINE_ID,"enrollment_id":ENROLLMENT_ID}),
            "machine_id",
            "installation_id",
        ),
    ] {
        if convert(input.clone()).as_ref() != Some(&input) {
            return Err(format!("{name} must round-trip the sign-in positive vector").into());
        }
        let mut legacy_present = input.clone();
        super::set_claim(&mut legacy_present, legacy, json!(MACHINE_ID))?;
        if convert(legacy_present).is_some() {
            return Err(format!("{name} accepted obsolete field {legacy} beside {field}").into());
        }
        let mut v4 = input;
        super::set_claim(&mut v4, field, json!(V4_MACHINE_ID))?;
        if convert(v4).is_some() {
            return Err(format!("{name} accepted a v4 machine ID").into());
        }
    }
    Ok(())
}

pub fn check_admission_identity_vectors(
    admit: RoundTrip,
    admitted: RoundTrip,
    admission: RoundTrip,
) -> ConformanceResult<()> {
    let request = json!({"organizationId":ORGANIZATION_ID,"enrollmentId":ENROLLMENT_ID,
        "clientId":ENROLLMENT_ID,"presented":{"machineId":MACHINE_ID},"tokenExpiresAt":2208989400_i64});
    let response = json!({"organizationId":ORGANIZATION_ID,"enrollmentId":ENROLLMENT_ID,
        "tokenExpiresAt":2208989400_i64});
    let admission_response = json!({"result":"admitted","machine":response});
    for (name, convert, input, legacy_fields) in [
        (
            "AdmitMachineConnection",
            admit,
            request,
            &["installation"][..],
        ),
        (
            "AdmittedMachine",
            admitted,
            response,
            &["installation", "peer"][..],
        ),
        (
            "MachineConnectionAdmission",
            admission,
            admission_response,
            &["installation", "peer"][..],
        ),
    ] {
        if convert(input.clone()).as_ref() != Some(&input) {
            return Err(
                format!("{name} must round-trip without obsolete machine identities").into(),
            );
        }
        for field in legacy_fields {
            let mut legacy = input.clone();
            let target = if name == "MachineConnectionAdmission" {
                legacy
                    .get_mut("machine")
                    .ok_or("admitted response has no machine")?
            } else {
                &mut legacy
            };
            super::set_claim(target, field, json!(MACHINE_ID))?;
            if convert(legacy).is_some() {
                return Err(format!("{name} accepted obsolete field {field}").into());
            }
        }
    }
    Ok(())
}
