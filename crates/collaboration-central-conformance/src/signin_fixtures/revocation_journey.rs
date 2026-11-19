//! Shared wire oracles exercised against both hosts' private sign-in listeners.
use super::{ConformanceResult, OWNER, OWNER_SUBJECT, POLICY, Person, fresh_request_id};
use serde_json::{Value, json};

const REVISIONS: &str = "/internal/organization-authorization-revisions";
pub const REVOKE_MACHINE: &str = "/organization-admin/v1/revoke-machine-enrollment";

pub struct RevocationJourney<'fixture> {
    pub client: &'fixture reqwest::Client,
    pub origin: &'fixture str,
    pub organization: &'fixture str,
    pub now: i64,
}

impl RevocationJourney<'_> {
    pub async fn prepare(&self) -> ConformanceResult<Person> {
        for floor in [
            json!({"kind":"organization","organizationId":self.organization,"revision":3,"state":"active","permissionPolicyDigest":POLICY}),
            json!({"kind":"person","organizationId":self.organization,"personId":OWNER,"subjectId":OWNER_SUBJECT,"revision":4,"state":"active"}),
        ] {
            let response = self
                .client
                .post(format!("{}{REVISIONS}", self.origin))
                .json(&floor)
                .send()
                .await?;
            if response.status().as_u16() != 200 {
                return Err(format!("fixture floor refused: {}", response.status()).into());
            }
        }
        let mut owner = Person::owner(self.organization, self.now)?;
        owner.permissions.push("machine:enroll".to_owned());
        Ok(owner)
    }

    pub async fn check_refusals(
        &self,
        owner: &mut Person,
        enrollment: &str,
    ) -> ConformanceResult<()> {
        let unpermitted = Person::owner(self.organization, self.now)?;
        self.expect(
            &unpermitted,
            enrollment,
            json!({"result":"refused","reason":"notPermitted"}),
        )
        .await?;
        let factor = owner.second_factor_at;
        owner.second_factor_at = Some(self.now - 901);
        self.expect(
            owner,
            enrollment,
            json!({"result":"refused","reason":"factorNotRecent"}),
        )
        .await?;
        owner.second_factor_at = factor;
        let body = json!({"organizationId":self.organization,"enrollmentId":enrollment});
        for (field, forged) in [
            ("actor", json!(OWNER)),
            ("secondFactorAt", json!(self.now - 10)),
        ] {
            let mut input = body.clone();
            input[field] = forged;
            let reply = self.post(owner, enrollment, input).await?;
            if reply.status().as_u16() != 401 {
                return Err(format!("an authenticated body with {field} must be refused").into());
            }
        }
        self.expect(
            owner,
            "019a61bb-0000-7000-8000-000000000099",
            json!({"result":"notFound"}),
        )
        .await
    }

    pub async fn expect(
        &self,
        person: &Person,
        enrollment: &str,
        expected: Value,
    ) -> ConformanceResult<()> {
        let body = json!({"organizationId":self.organization,"enrollmentId":enrollment});
        let reply = self.post(person, enrollment, body).await?;
        let status = reply.status().as_u16();
        if status != 200 || reply.json::<Value>().await? != expected {
            return Err(format!(
                "revocation reply did not match expected {expected}: HTTP {status}"
            )
            .into());
        }
        Ok(())
    }

    async fn post(
        &self,
        person: &Person,
        enrollment: &str,
        body: Value,
    ) -> ConformanceResult<reqwest::Response> {
        let intent = json!({
            "v":1,
            "operation":"RevokeMachineEnrollment",
            "organization":self.organization,
            "target":enrollment,
            "operationId":null,
            "params":{"actor":person.id,"secondFactorAt":person.second_factor_at},
        });
        let headers = person.headers(
            REVOKE_MACHINE,
            &intent,
            &fresh_request_id(self.now)?,
            self.now,
        )?;
        let mut request = self
            .client
            .post(format!("{}{REVOKE_MACHINE}", self.origin))
            .json(&body);
        for (header, value) in headers {
            request = request.header(header, value);
        }
        Ok(request.send().await?)
    }
}
