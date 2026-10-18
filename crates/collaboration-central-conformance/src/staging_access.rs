use crate::ConformanceResult;
use collaboration_central_core::AccessClientId;
use collaboration_replication_protocol::InstallationId;

/// One enrolled staging installation and what it presents to Access and to
/// central. There is deliberately no `Debug`: every field but the label and
/// installation is a credential.
pub struct StagingInstallation {
    pub label: String,
    pub installation: InstallationId,
    pub machine_credential: String,
    pub access_client_id: AccessClientId,
    pub access_client_secret: String,
}

/// Real Cloudflare Access for a staging central. Access issues the assertion
/// central verifies: from the owner's app token on admin and control routes,
/// and from an installation's service token on its session upgrade.
///
/// The shared scenarios name holders by fixture label. Staging has two
/// enrolled installations, so `new` plays the label "new" and `old` plays
/// every other label; scenarios that hold more than two installations at once
/// don't run on staging. An optional third, temporary installation plays the
/// label "temporary" for the rows that replace or revoke it (S4, A10).
pub struct StagingAccess {
    pub owner_app_token: String,
    pub old: StagingInstallation,
    pub new: StagingInstallation,
    pub temporary: Option<StagingInstallation>,
}

impl StagingAccess {
    /// Reads both installations from the environment (the 1Password item's
    /// fields, injected by `op run`) and the owner's current app token from
    /// `cloudflared`, which holds it after the owner's own sign-in.
    pub async fn from_environment(owner_app_url: &str) -> ConformanceResult<Self> {
        Ok(Self {
            owner_app_token: owner_app_token(owner_app_url).await?,
            old: installation_from_environment("STAGING_A")?,
            new: installation_from_environment("STAGING_B")?,
            temporary: if std::env::var_os("STAGING_T_INSTALLATION").is_some() {
                Some(installation_from_environment("STAGING_T")?)
            } else {
                None
            },
        })
    }

    #[must_use]
    pub fn for_fixture(&self, label: &str) -> Option<&StagingInstallation> {
        match label {
            "temporary" => self.temporary.as_ref(),
            "new" => Some(&self.new),
            _ => Some(&self.old),
        }
    }

    pub fn installations(&self) -> impl Iterator<Item = &StagingInstallation> {
        [Some(&self.old), Some(&self.new), self.temporary.as_ref()]
            .into_iter()
            .flatten()
    }

    #[must_use]
    pub fn for_credential(&self, machine_credential: &str) -> Option<&StagingInstallation> {
        self.installations()
            .find(|installation| installation.machine_credential == machine_credential)
    }
}

/// Asks `cloudflared` for the owner's app token for one Access application.
/// The token stays in memory; a missing sign-in fails without echoing output.
pub async fn owner_app_token(owner_app_url: &str) -> ConformanceResult<String> {
    let output = tokio::process::Command::new("cloudflared")
        .args(["access", "token", &format!("-app={owner_app_url}")])
        .output()
        .await?;
    let token = String::from_utf8(output.stdout)?.trim().to_owned();
    if !output.status.success() || token.split('.').count() != 3 {
        return Err(
            "owner sign-in missing: run `cloudflared access login` for the admin app".into(),
        );
    }
    Ok(token)
}

fn installation_from_environment(prefix: &str) -> ConformanceResult<StagingInstallation> {
    let read = |name: &str| {
        std::env::var(format!("{prefix}_{name}")).map_err(|_| format!("{prefix}_{name} is not set"))
    };
    Ok(StagingInstallation {
        label: read("LABEL")?,
        installation: InstallationId::new(read("INSTALLATION")?)?,
        machine_credential: read("MACHINE_CREDENTIAL")?,
        access_client_id: AccessClientId::new(read("ACCESS_CLIENT_ID")?)?,
        access_client_secret: read("ACCESS_CLIENT_SECRET")?,
    })
}
