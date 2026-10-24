//! Existing Codex plugin responses from qualified native package facts.
use super::*;
use std::collections::BTreeMap;

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginListParams {
    cwds: Option<Vec<PathBuf>>,
    marketplace_kinds: Option<Vec<String>>,
    #[serde(default)]
    force_refetch: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PluginReadParams {
    marketplace_path: Option<PathBuf>,
    remote_marketplace_name: Option<String>,
    plugin_name: String,
}

pub(super) async fn list(
    context: &RouterSessionAppServerContext,
    params: &Value,
) -> Result<Value, CapabilityCodecError> {
    let params: PluginListParams =
        serde_json::from_value(params.clone()).map_err(|_| CapabilityCodecError::InvalidParams)?;
    if params
        .marketplace_kinds
        .as_ref()
        .is_some_and(|kinds| kinds.is_empty() || kinds.iter().any(|kind| kind != "local"))
    {
        return Err(CatalogueReadFailure::UnsupportedConsumerFilter.into());
    }
    let queries = match params.cwds {
        Some(cwds) if !cwds.is_empty() => cwds
            .into_iter()
            .map(|cwd| CapabilityPath::new(cwd).map(|cwd| CatalogueQuery::Workspace { cwd }))
            .collect::<Result<Vec<_>, _>>()?,
        _ => vec![CatalogueQuery::Home],
    };
    let mut marketplaces = BTreeMap::<(String, PathBuf), BTreeMap<String, Value>>::new();
    let mut marketplace_errors = Vec::new();
    for query in queries {
        let report = read_catalogue(context, query, params.force_refetch).await?;
        let CatalogueSection::Supported {
            items,
            enumeration,
            errors,
        } = report.plugins
        else {
            return Err(CapabilityCodecError::Unsupported);
        };
        validate_enumeration(&enumeration)?;
        let mut usable = 0;
        for plugin in items {
            if matches!(plugin.origin, NativeOrigin::Bundled { .. }) {
                continue;
            }
            let (name, path) = marketplace(&plugin)?;
            let summary = summary(&plugin)?;
            usable += 1;
            let entries = marketplaces
                .entry((name.to_owned(), path.to_owned()))
                .or_default();
            if let Some(previous) = entries.insert(plugin.id.as_str().to_owned(), summary.clone())
                && previous != summary
            {
                return Err(CatalogueReadFailure::AmbiguousSelector.into());
            }
        }
        marketplace_errors.extend(project_source_errors(
            errors,
            ConsumerCatalogueKind::Plugins,
            usable > 0,
        )?);
    }
    let marketplaces = marketplaces.into_iter().map(|((name, path), plugins)|
        json!({"name":name,"path":path,"interface":null,"plugins":plugins.into_values().collect::<Vec<_>>()})).collect::<Vec<_>>();
    Ok(
        json!({"marketplaces":marketplaces,"marketplaceLoadErrors":marketplace_errors,"featuredPluginIds":[]}),
    )
}

pub(super) async fn read(
    context: &RouterSessionAppServerContext,
    params: &Value,
) -> Result<Value, CapabilityCodecError> {
    let params: PluginReadParams =
        serde_json::from_value(params.clone()).map_err(|_| CapabilityCodecError::InvalidParams)?;
    let marketplace_selector = match (params.marketplace_path, params.remote_marketplace_name) {
        (Some(path), None) => NativeMarketplaceSelector::Filesystem {
            manifest_path: CapabilityPath::new(path)?,
        },
        (None, Some(name)) => NativeMarketplaceSelector::NamedCatalog {
            native_name: CapabilityName::new(name)?,
        },
        _ => return Err(CapabilityCodecError::InvalidParams),
    };
    let selector = NativePluginSelector {
        marketplace: marketplace_selector,
        plugin_name: CapabilityName::new(params.plugin_name)?,
    };
    let port = context
        .native_catalogue
        .as_ref()
        .ok_or(CapabilityCodecError::Unsupported)?;
    let result = tokio::time::timeout(
        CATALOGUE_READ_TIMEOUT,
        port.read_plugin(NativePluginRead {
            endpoint: context.endpoint.clone(),
            selector: selector.clone(),
            actor: context.actor.clone(),
        }),
    )
    .await
    .map_err(|_| CatalogueReadFailure::TimedOut)?;
    let NativePluginReadResult::Ready { scope, plugin } = result else {
        return match result {
            NativePluginReadResult::Failed { reason } => Err(reason.into()),
            _ => Err(CapabilityCodecError::Unsupported),
        };
    };
    if String::from(scope.binding.endpoint.service_id.clone())
        != context.endpoint.service_id.as_str()
        || String::from(scope.binding.endpoint.endpoint_id.clone())
            != context.endpoint.endpoint_id.as_str()
        || scope.view != CatalogueView::FilesystemCatalogues
    {
        return Err(CatalogueReadFailure::InvalidScope.into());
    }
    let (marketplace_name, marketplace_path) = marketplace(&plugin)?;
    if plugin.name != selector.plugin_name
        || !matches!(&selector.marketplace,
        NativeMarketplaceSelector::Filesystem { manifest_path } if manifest_path.as_path() == marketplace_path)
    {
        return Err(CatalogueReadFailure::InvalidScope.into());
    }
    let mut skills = Vec::new();
    for skill in complete(&plugin.components.skills, "skill component coverage")? {
        if skill.plugin_id.as_ref() != Some(&plugin.id) {
            return Err(CatalogueReadFailure::InvalidNativeData.into());
        }
        let enabled = match skill.availability {
            NativeAvailability::Available => true,
            NativeAvailability::Disabled => false,
            _ => return Err(unrepresentable("skill availability unobserved", 1)),
        };
        let path = match &skill.origin {
            NativeOrigin::Filesystem { actual_path } => Some(actual_path.as_path()),
            NativeOrigin::Bundled { .. } => None,
            _ => return Err(unrepresentable("skill origin unobserved", 1)),
        };
        skills.push(
            json!({"name":skill.name.as_str(),"description":skill.description,
            "enabled":enabled,"path":path,"shortDescription":null,"interface":null}),
        );
    }
    // No native hook event has yet been semantically qualified against the
    // consumer enum. Unknown events fail rather than being renamed or dropped.
    let hooks = complete(&plugin.components.hooks, "hook component coverage")?;
    if !hooks.is_empty() {
        return Err(unrepresentable(
            "native hook identity or event",
            hooks.len(),
        ));
    }
    let other = complete(&plugin.components.other, "other component coverage")?;
    if !other.is_empty() {
        return Err(unrepresentable("native component kind", other.len()));
    }
    let mcp = complete(&plugin.components.mcp, "MCP component coverage")?
        .iter()
        .map(|component| {
            if component.availability == NativeComponentAvailability::NotObserved {
                return Err(unrepresentable("MCP component availability unobserved", 1));
            }
            Ok(component.name.as_str())
        })
        .collect::<Result<Vec<_>, _>>()?;
    let commands = complete(&plugin.components.commands, "command component coverage")?;
    if commands
        .iter()
        .any(|command| command.availability == NativeComponentAvailability::NotObserved)
    {
        return Err(unrepresentable(
            "command component availability unobserved",
            commands.len(),
        ));
    }
    Ok(json!({"plugin":{
        "marketplaceName":marketplace_name,"marketplacePath":marketplace_path,"summary":summary(&plugin)?,
        "shareUrl":null,"description":display_description(&plugin)?,"skills":skills,"hooks":[],
        "apps":[],"appTemplates":[],"mcpServers":mcp,"scheduledTasks":null
    }}))
}

fn marketplace(plugin: &NativePlugin) -> Result<(&str, &std::path::Path), CapabilityCodecError> {
    match &plugin.marketplace {
        NativeMarketplaceObservation::Qualified {
            native_name,
            source: NativeMarketplaceSource::Filesystem { manifest_path },
        } => Ok((native_name.as_str(), manifest_path.as_path())),
        _ => Err(unrepresentable(
            "native marketplace identity or manifest unobserved",
            1,
        )),
    }
}

fn complete<'a, T>(
    section: &'a NativeComponentSection<T>,
    category: &'static str,
) -> Result<&'a [T], CapabilityCodecError> {
    match section {
        NativeComponentSection::Observed {
            items,
            coverage: CatalogueCoverage::Complete,
        } => Ok(items),
        _ => Err(CapabilityCodecError::Unobserved { category }),
    }
}

fn unrepresentable(category: &'static str, count: usize) -> CapabilityCodecError {
    CapabilityCodecError::Unrepresentable { category, count }
}

fn display_description(plugin: &NativePlugin) -> Result<String, CapabilityCodecError> {
    if !matches!(
        plugin.installation_policy,
        NativePolicyObservation::Undeclared
    ) || !matches!(
        plugin.authentication_policy,
        NativePolicyObservation::Undeclared
    ) {
        return Err(unrepresentable(
            "native installation or authentication policy",
            1,
        ));
    }
    let native = plugin.description.as_deref().unwrap_or("");
    let state = match plugin.state {
        NativePluginState::InstalledEnabled {
            loading: NativeLoadingState::Loaded,
        } => "Installed and enabled; native loading observed.",
        NativePluginState::InstalledEnabled {
            loading: NativeLoadingState::NotLoaded,
        } => "Installed and enabled; native not-loaded state observed.",
        NativePluginState::InstalledEnabled {
            loading: NativeLoadingState::NotObserved,
        } => "Installed and enabled; native loading not observed.",
        NativePluginState::InstalledDisabled => "Installed and disabled.",
        _ => {
            return Err(unrepresentable(
                "native installation or enablement unobserved",
                1,
            ));
        }
    };
    Ok(format!(
        "{native}\n{state}\nCodex display defaults: installation AVAILABLE; authentication ON_INSTALL. Native policy is undeclared."
    ))
}

fn summary(plugin: &NativePlugin) -> Result<Value, CapabilityCodecError> {
    let (installed, enabled) = match plugin.state {
        NativePluginState::InstalledEnabled { .. } => (true, true),
        NativePluginState::InstalledDisabled => (true, false),
        _ => {
            return Err(unrepresentable(
                "native installation or enablement unobserved",
                1,
            ));
        }
    };
    let NativeOrigin::Filesystem { actual_path } = &plugin.origin else {
        return Err(unrepresentable("native plugin origin unobserved", 1));
    };
    Ok(
        json!({"id":plugin.id.as_str(),"name":plugin.name.as_str(),"remotePluginId":null,
        "version":null,"localVersion":plugin.display_version.as_ref().map(CapabilityName::as_str),
        "shareContext":null,"source":{"type":"local","path":actual_path.as_path()},
        "installed":installed,"enabled":enabled,"installedAt":null,"installPolicy":"AVAILABLE",
        "installPolicySource":null,"authPolicy":"ON_INSTALL","availability":"AVAILABLE",
        "mustShowInstallationInterstitial":null,"disabledReason":null,"eligiblePlanTypes":null,
        "interface":{"shortDescription":display_description(plugin)?,"capabilities":[],"screenshots":[],"screenshotUrls":[]},"keywords":[]}),
    )
}
