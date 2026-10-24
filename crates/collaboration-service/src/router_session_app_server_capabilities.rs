//! Translate qualified native observations into the installed consumer protocol.
use super::*;
use crate::provider_capabilities::*;
use serde::Deserialize;
use std::time::Duration;

#[path = "router_session_app_server_plugins.rs"]
mod plugins;

const CATALOGUE_READ_TIMEOUT: Duration = Duration::from_secs(30);

pub(super) fn is_capability_method(method: &str) -> bool {
    matches!(
        method,
        "skills/list"
            | "plugin/list"
            | "plugin/read"
            | "skills/config/write"
            | "skills/extraRoots/set"
            | "plugin/install"
            | "plugin/uninstall"
            | "plugin/enable"
            | "plugin/disable"
            | "plugin/reconcile"
            | "marketplace/add"
            | "marketplace/remove"
            | "marketplace/upgrade"
    )
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillsListParams {
    #[serde(default)]
    cwds: Vec<PathBuf>,
    #[serde(default)]
    force_reload: bool,
}

pub(super) async fn handle_capability_request(
    context: &RouterSessionAppServerContext,
    id: Value,
    method: &str,
    params: &Value,
) -> Value {
    let result = match method {
        "skills/list" => skills_list(context, params).await,
        "plugin/list" => plugins::list(context, params).await,
        "plugin/read" => plugins::read(context, params).await,
        _ => Err(CapabilityCodecError::ManagementUnsupported),
    };
    match result {
        Ok(result) => json!({"id":id,"result":result}),
        Err(error) => json!({"id":id,"error":{
            "code": if matches!(error, CapabilityCodecError::InvalidParams) { -32602 } else { -32000 },
            "message":error.to_string()
        }}),
    }
}

#[derive(Debug, thiserror::Error)]
enum CapabilityCodecError {
    #[error("invalid native capability parameters")]
    InvalidParams,
    #[error("native capability discovery is unsupported for this provider binding")]
    Unsupported,
    #[error("native capability installation and configuration management is unsupported")]
    ManagementUnsupported,
    #[error(transparent)]
    Read(#[from] CatalogueReadFailure),
    #[error(
        "native filesystem catalogue cannot be represented completely: {category} ({count} entries)"
    )]
    Unrepresentable {
        category: &'static str,
        count: usize,
    },
    #[error(
        "native filesystem catalogue cannot be represented completely: {category} (count unobserved)"
    )]
    Unobserved { category: &'static str },
    #[error(
        "native filesystem catalogue cannot be represented completely: incomplete native filesystem view: {categories} ({count} limitations)"
    )]
    Incomplete { categories: String, count: usize },
}

#[tracing::instrument(name = "native_skills_list", skip_all)]
async fn skills_list(
    context: &RouterSessionAppServerContext,
    params: &Value,
) -> Result<Value, CapabilityCodecError> {
    let params: SkillsListParams =
        serde_json::from_value(params.clone()).map_err(|_| CapabilityCodecError::InvalidParams)?;
    let requested_cwds_present = !params.cwds.is_empty();
    let cwds = if params.cwds.is_empty() {
        vec![
            context
                .catalogue_default_cwd
                .clone()
                .ok_or(CatalogueReadFailure::InvalidScope)?,
        ]
    } else {
        params
            .cwds
            .into_iter()
            .map(CapabilityPath::new)
            .collect::<Result<Vec<_>, _>>()?
    };
    let mut data = Vec::with_capacity(cwds.len());
    for cwd in cwds {
        tracing::info!(
            phase = "native-skills-list-codec",
            category = "decoded-workspace-relation",
            provider = match context.endpoint.endpoint_id.as_str() {
                "cursor-local" => "cursor",
                "claude-local" => "claude-code",
                _ => "other",
            },
            requested_cwds_present,
            requested_equals_captured_default = ?context.catalogue_default_cwd.as_ref()
                .map(|default| default == &cwd),
            "native catalogue workspace relation checked"
        );
        let report = read_catalogue(
            context,
            CatalogueQuery::Workspace { cwd: cwd.clone() },
            params.force_reload,
        )
        .await?;
        let (skills, errors) = project_skills(report)?;
        data.push(json!({"cwd":cwd.as_path(),"skills":skills,"errors":errors}));
    }
    Ok(json!({"data":data}))
}

async fn read_catalogue(
    context: &RouterSessionAppServerContext,
    query: CatalogueQuery,
    refresh_native: bool,
) -> Result<CatalogueObservation, CapabilityCodecError> {
    let port = context
        .native_catalogue
        .as_ref()
        .ok_or(CapabilityCodecError::Unsupported)?;
    let result = tokio::time::timeout(
        CATALOGUE_READ_TIMEOUT,
        port.read_catalogue(CatalogueRead {
            endpoint: context.endpoint.clone(),
            query: query.clone(),
            view: CatalogueView::FilesystemCatalogues,
            refresh_native,
            actor: context.actor.clone(),
        }),
    )
    .await
    .map_err(|_| CatalogueReadFailure::TimedOut)?;
    match result {
        CatalogueResult::Ready { report } => {
            let binding_endpoint = &report.scope.binding.endpoint;
            if String::from(binding_endpoint.service_id.clone())
                != context.endpoint.service_id.as_str()
                || String::from(binding_endpoint.endpoint_id.clone())
                    != context.endpoint.endpoint_id.as_str()
                || report.scope.query != query
                || report.scope.view != CatalogueView::FilesystemCatalogues
            {
                return Err(CatalogueReadFailure::InvalidScope.into());
            }
            Ok(*report)
        }
        CatalogueResult::Unsupported { .. } => Err(CapabilityCodecError::Unsupported),
        CatalogueResult::Failed { reason } => Err(reason.into()),
    }
}

fn project_skills(
    report: CatalogueObservation,
) -> Result<(Vec<Value>, Vec<Value>), CapabilityCodecError> {
    let CatalogueSection::Supported {
        items,
        enumeration,
        errors,
    } = report.skills
    else {
        return Err(CapabilityCodecError::Unsupported);
    };
    validate_enumeration(&enumeration)?;
    let mut rows = Vec::new();
    let mut unrepresentable = 0;
    for skill in items {
        let actual_path = match skill.origin {
            NativeOrigin::Filesystem { actual_path } => actual_path,
            NativeOrigin::Bundled { .. } => continue,
            NativeOrigin::Other { .. } => {
                unrepresentable += 1;
                continue;
            }
        };
        let scope = match skill.native_scope {
            NativeScope::User => "user",
            NativeScope::Workspace => "repo",
            NativeScope::System => "system",
            NativeScope::Managed => "admin",
            NativeScope::Unknown => {
                unrepresentable += 1;
                continue;
            }
        };
        let enabled = match skill.availability {
            NativeAvailability::Available => true,
            NativeAvailability::Disabled => false,
            NativeAvailability::Unavailable | NativeAvailability::Unknown => {
                unrepresentable += 1;
                continue;
            }
        };
        rows.push(
            json!({"name":skill.name.as_str(),"description":skill.description,
            "path":actual_path.as_path(),"scope":scope,"enabled":enabled,
            "pluginId":skill.plugin_id.as_ref().map(NativePluginId::as_str)}),
        );
    }
    if unrepresentable > 0 {
        return Err(CapabilityCodecError::Unrepresentable {
            category: "origin, scope or availability unobserved",
            count: unrepresentable,
        });
    }
    let errors = project_source_errors(errors, ConsumerCatalogueKind::Skills, !rows.is_empty())?;
    Ok((rows, errors))
}

#[derive(Clone, Copy)]
enum ConsumerCatalogueKind {
    Skills,
    Plugins,
}

fn project_source_errors(
    errors: Vec<CatalogueSourceError>,
    kind: ConsumerCatalogueKind,
    has_usable_items: bool,
) -> Result<Vec<Value>, CapabilityCodecError> {
    if !errors.is_empty() && !has_usable_items {
        return Err(CapabilityCodecError::Unrepresentable {
            category: "all native filesystem candidates are unrepresentable",
            count: errors.len(),
        });
    }
    let mut projected = Vec::new();
    let mut unrepresentable = 0;
    for error in errors {
        match (kind, error.source) {
            (ConsumerCatalogueKind::Skills, NativeErrorSource::SkillSource { actual_path }) => {
                projected
                    .push(json!({"path":actual_path.as_path(),"message":error.reason.to_string()}))
            }
            (
                ConsumerCatalogueKind::Plugins,
                NativeErrorSource::MarketplaceManifest { actual_path },
            ) => projected.push(
                json!({"marketplacePath":actual_path.as_path(),"message":error.reason.to_string()}),
            ),
            _ => unrepresentable += 1,
        }
    }
    if unrepresentable > 0 {
        return Err(CapabilityCodecError::Unrepresentable {
            category: "native source-error role or location is unrepresentable",
            count: unrepresentable,
        });
    }
    Ok(projected)
}

fn validate_enumeration(enumeration: &SourceEnumeration) -> Result<(), CapabilityCodecError> {
    if let SourceEnumeration::Incomplete { limitations } = enumeration {
        return Err(CapabilityCodecError::Incomplete {
            categories: limitations
                .iter()
                .map(|reason| match reason {
                    CatalogueLimitation::PartialNativeExport => "partial native export",
                    CatalogueLimitation::SessionOnly => "only session data observed",
                    CatalogueLimitation::MissingOrigin => "source origin unobserved",
                    CatalogueLimitation::MissingScope => "source scope unobserved",
                    CatalogueLimitation::MissingEnablement => "native enablement unobserved",
                    CatalogueLimitation::AmbiguousNativeIdentity => "ambiguous native identity",
                    CatalogueLimitation::ConsumerCannotRepresent => "native source unrepresentable",
                })
                .collect::<Vec<_>>()
                .join(", "),
            count: limitations.iter().count(),
        });
    }
    Ok(())
}
