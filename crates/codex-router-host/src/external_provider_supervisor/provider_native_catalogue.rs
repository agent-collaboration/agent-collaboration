//! Fresh catalogue reads retain the actual supervisor binding and retirement.
use super::*;
use collaboration_service::provider_capabilities::*;

impl ExternalProviderSupervisor {
    pub(crate) fn native_catalogue_default_cwd(
        &self,
        endpoint: &EndpointRef,
    ) -> Option<CapabilityPath> {
        let context = self
            .inner
            .bindings
            .get(endpoint)?
            .runtime
            .native_read_context
            .as_ref()?;
        CapabilityPath::new(context.default_workspace().ok()?).ok()
    }
}

impl NativeCataloguePort for ExternalProviderSupervisor {
    fn read_catalogue(&self, request: CatalogueRead) -> CatalogueFuture<'_> {
        Box::pin(async move {
            let binding = self.inner.bindings.values().find(|binding| {
                String::from(binding.identity.endpoint.service_id.clone())
                    == request.endpoint.service_id.as_str()
                    && String::from(binding.identity.endpoint.endpoint_id.clone())
                        == request.endpoint.endpoint_id.as_str()
            });
            let Some(binding) = binding else {
                return CatalogueResult::Failed {
                    reason: CatalogueReadFailure::InvalidScope,
                };
            };
            if request.view != CatalogueView::FilesystemCatalogues {
                return CatalogueResult::Unsupported {
                    reason: CatalogueUnsupported::NativeOperationMissing,
                };
            }
            let Some(context) = binding.runtime.native_read_context.as_ref() else {
                return CatalogueResult::Unsupported {
                    reason: CatalogueUnsupported::ScopeUnavailable,
                };
            };
            let cwd = match &request.query {
                CatalogueQuery::Workspace { cwd } => cwd.as_path().to_owned(),
                CatalogueQuery::Home
                    if context.provider == collaboration_protocol::ProviderKind::ClaudeCode =>
                {
                    match context.home_directory() {
                        Some(home) => home.to_owned(),
                        None => {
                            return CatalogueResult::Failed {
                                reason: CatalogueReadFailure::InvalidScope,
                            };
                        }
                    }
                }
                _ => {
                    return CatalogueResult::Unsupported {
                        reason: CatalogueUnsupported::ScopeUnavailable,
                    };
                }
            };
            let cancelled = binding.runtime.retirement().child_token();
            let _cancel_on_drop = cancelled.clone().drop_guard();
            if context.provider == collaboration_protocol::ProviderKind::ClaudeCode {
                let plugins = match context.claude_plugins(cwd.clone(), cancelled.clone()).await {
                    Ok(plugins) => plugins,
                    Err(reason) => return CatalogueResult::Failed { reason },
                };
                let skills = match context
                    .claude_skill_section(cwd, plugins.clone(), cancelled)
                    .await
                {
                    Ok(section) => section,
                    Err(reason) => CatalogueSection::Supported {
                        items: Vec::new(),
                        enumeration: SourceEnumeration::Accounted,
                        errors: vec![CatalogueSourceError {
                            source: NativeErrorSource::Unobserved,
                            reason,
                        }],
                    },
                };
                return CatalogueResult::Ready {
                    report: Box::new(CatalogueObservation {
                        scope: CatalogueScope {
                            binding: binding.identity.clone(),
                            query: request.query,
                            view: request.view,
                        },
                        observed_at: chrono::Utc::now(),
                        skills,
                        plugins: CatalogueSection::Supported {
                            items: plugins,
                            enumeration: SourceEnumeration::Accounted,
                            errors: Vec::new(),
                        },
                    }),
                };
            }
            match context.skills(cwd, cancelled).await {
                Ok(skills) => CatalogueResult::Ready {
                    report: Box::new(CatalogueObservation {
                        scope: CatalogueScope {
                            binding: binding.identity.clone(),
                            query: request.query,
                            view: request.view,
                        },
                        observed_at: chrono::Utc::now(),
                        skills,
                        plugins: CatalogueSection::Unsupported {
                            reason: CatalogueUnsupported::NativeOperationMissing,
                        },
                    }),
                },
                Err(reason) => CatalogueResult::Failed { reason },
            }
        })
    }

    fn read_plugin(&self, request: NativePluginRead) -> NativePluginReadFuture<'_> {
        Box::pin(async move {
            let Some(binding) = self.inner.bindings.values().find(|binding| {
                String::from(binding.identity.endpoint.service_id.clone())
                    == request.endpoint.service_id.as_str()
                    && String::from(binding.identity.endpoint.endpoint_id.clone())
                        == request.endpoint.endpoint_id.as_str()
            }) else {
                return NativePluginReadResult::Failed {
                    reason: CatalogueReadFailure::InvalidScope,
                };
            };
            let Some(context) = binding.runtime.native_read_context.as_ref() else {
                return NativePluginReadResult::Unsupported {
                    reason: CatalogueUnsupported::ScopeUnavailable,
                };
            };
            if context.provider != collaboration_protocol::ProviderKind::ClaudeCode {
                return NativePluginReadResult::Unsupported {
                    reason: CatalogueUnsupported::NativeOperationMissing,
                };
            }
            let workspace = match context.plugin_read_workspace() {
                Ok(workspace) => workspace,
                Err(reason) => return NativePluginReadResult::Unsupported { reason },
            };
            let cancelled = binding.runtime.retirement().child_token();
            let _cancel_on_drop = cancelled.clone().drop_guard();
            // Only an independently qualified setup may be re-enumerated.
            // A supplied selector never authorizes opening an arbitrary path.
            let scope_cwd = match CapabilityPath::new(workspace.clone()) {
                Ok(cwd) => cwd,
                Err(reason) => return NativePluginReadResult::Failed { reason },
            };
            let plugins = match context.claude_plugins(workspace, cancelled).await {
                Ok(plugins) => plugins,
                Err(reason) => return NativePluginReadResult::Failed { reason },
            };
            let mut matching = plugins.into_iter().filter(|plugin| {
                plugin.name == request.selector.plugin_name
                    && matches!((&plugin.marketplace, &request.selector.marketplace),
                        (NativeMarketplaceObservation::Qualified { source: NativeMarketplaceSource::Filesystem { manifest_path: observed }, .. },
                         NativeMarketplaceSelector::Filesystem { manifest_path: selected }) if observed == selected)
            });
            let Some(plugin) = matching.next() else {
                return NativePluginReadResult::Failed {
                    reason: CatalogueReadFailure::NotFound,
                };
            };
            if matching.next().is_some() {
                return NativePluginReadResult::Failed {
                    reason: CatalogueReadFailure::AmbiguousSelector,
                };
            }
            NativePluginReadResult::Ready {
                scope: Box::new(CatalogueScope {
                    binding: binding.identity.clone(),
                    query: CatalogueQuery::Workspace { cwd: scope_cwd },
                    view: CatalogueView::FilesystemCatalogues,
                }),
                plugin: Box::new(plugin),
            }
        })
    }
}
