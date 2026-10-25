//! Fresh catalogue reads retain the actual supervisor binding and retirement.
use super::*;
use collaboration_service::provider_capabilities::*;

#[cfg(all(test, target_os = "macos"))]
#[path = "provider_native_catalogue_tests.rs"]
mod tests;

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
                let package_read =
                    match context.claude_plugins(cwd.clone(), cancelled.clone()).await {
                        Ok(plugins) => plugins,
                        Err(reason) => return CatalogueResult::Failed { reason },
                    };
                let skills = match context
                    .claude_skill_section(cwd, package_read.skills, cancelled)
                    .await
                {
                    Ok(section) => section,
                    Err(reason) => CatalogueSection::Supported {
                        items: Vec::new(),
                        enumeration: SourceEnumeration::Incomplete {
                            limitations: CatalogueLimitations::new(
                                CatalogueLimitation::PartialNativeExport,
                                Vec::new(),
                            ),
                        },
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
                        plugins: package_read.plugins,
                    }),
                };
            }
            let skills = match context.skills(cwd, cancelled).await {
                Ok(skills) => skills,
                Err(CatalogueReadFailure::Cancelled) => {
                    return CatalogueResult::Failed {
                        reason: CatalogueReadFailure::Cancelled,
                    };
                }
                Err(reason) => {
                    tracing::warn!(
                        phase = "cursor-catalogue-result",
                        provider = "cursor",
                        category = match reason {
                            CatalogueReadFailure::Unavailable => "unavailable",
                            CatalogueReadFailure::AuthenticationRequired =>
                                "authentication-required",
                            CatalogueReadFailure::InvalidNativeData => "invalid-native-data",
                            CatalogueReadFailure::InvalidScope => "invalid-scope",
                            CatalogueReadFailure::UnsupportedConsumerFilter =>
                                "unsupported-consumer-filter",
                            CatalogueReadFailure::NotFound => "not-found",
                            CatalogueReadFailure::AmbiguousSelector => "ambiguous-selector",
                            CatalogueReadFailure::Cancelled => "cancelled",
                            CatalogueReadFailure::TimedOut => "timed-out",
                        },
                        "native catalogue read failed"
                    );
                    CatalogueSection::Supported {
                        items: Vec::new(),
                        errors: vec![CatalogueSourceError {
                            source: NativeErrorSource::Unobserved,
                            reason,
                        }],
                        enumeration: SourceEnumeration::Incomplete {
                            limitations: CatalogueLimitations::new(
                                CatalogueLimitation::MissingScope,
                                Vec::new(),
                            ),
                        },
                    }
                }
            };
            CatalogueResult::Ready {
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
            // The selector has no conversation/cwd to qualify a native setup.
            NativePluginReadResult::Unsupported {
                reason: CatalogueUnsupported::ScopeUnavailable,
            }
        })
    }
}
