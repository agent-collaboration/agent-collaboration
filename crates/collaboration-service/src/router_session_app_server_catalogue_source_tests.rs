//! C1 source-accounting table through actual async facade dispatch.
//! Scripted native facts are boundary controls, not Host/native journey proof.
use super::native_capabilities_tests::{ScriptedCatalogue, file_skill, report_with_skills};
use super::plugin_tests::package;
use super::test_support::ScriptedSessionBackend;
use super::*;
use crate::provider_capabilities::*;
use std::sync::Mutex;

#[derive(Clone, Copy)]
enum Method {
    Skills,
    Plugins,
}
impl Method {
    fn name(self) -> &'static str {
        match self {
            Self::Skills => "skills/list",
            Self::Plugins => "plugin/list",
        }
    }
    fn matching_source(self) -> NativeErrorSource {
        match self {
            Self::Skills => NativeErrorSource::SkillSource {
                actual_path: CapabilityPath::new("/native/skills/broken/SKILL.md".into())
                    .expect("skill source"),
            },
            Self::Plugins => NativeErrorSource::MarketplaceManifest {
                actual_path: CapabilityPath::new(
                    "/native/market/.claude-plugin/marketplace.json".into(),
                )
                .expect("marketplace manifest"),
            },
        }
    }
    fn with_section(
        self,
        report: &mut CatalogueObservation,
        items: bool,
        enumeration: SourceEnumeration,
        errors: Vec<CatalogueSourceError>,
    ) {
        match self {
            Self::Skills => {
                report.skills = CatalogueSection::Supported {
                    items: if items {
                        vec![file_skill()]
                    } else {
                        Vec::new()
                    },
                    enumeration,
                    errors,
                }
            }
            Self::Plugins => {
                report.plugins = CatalogueSection::Supported {
                    items: if items { vec![package()] } else { Vec::new() },
                    enumeration,
                    errors,
                }
            }
        }
    }
    fn rows(self, response: &Value) -> Option<&Vec<Value>> {
        let path = match self {
            Self::Skills => "/result/data/0/skills",
            Self::Plugins => "/result/marketplaces",
        };
        response.pointer(path).and_then(Value::as_array)
    }
    fn errors(self, response: &Value) -> Option<&Vec<Value>> {
        let path = match self {
            Self::Skills => "/result/data/0/errors",
            Self::Plugins => "/result/marketplaceLoadErrors",
        };
        response.pointer(path).and_then(Value::as_array)
    }
}
fn report() -> CatalogueObservation {
    let mut report = report_with_skills(vec![file_skill()]);
    report.plugins = CatalogueSection::Supported {
        items: vec![package()],
        enumeration: SourceEnumeration::Accounted,
        errors: Vec::new(),
    };
    report
}
fn source_error(source: NativeErrorSource) -> CatalogueSourceError {
    CatalogueSourceError {
        source,
        reason: CatalogueReadFailure::InvalidNativeData,
    }
}
fn incomplete() -> SourceEnumeration {
    SourceEnumeration::Incomplete {
        limitations: CatalogueLimitations::new(CatalogueLimitation::MissingOrigin, Vec::new()),
    }
}
async fn dispatch(method: Method, report: CatalogueObservation) -> Value {
    let backend = ScriptedSessionBackend::new().expect("backend");
    let (_, models) = watch::channel(Vec::new());
    let port = Arc::new(ScriptedCatalogue {
        requests: Mutex::new(Vec::new()),
        result: CatalogueResult::Ready {
            report: Box::new(report),
        },
    });
    let context = Arc::new(
        RouterSessionAppServerContext::new(
            backend.endpoint.clone(),
            ScriptedSessionBackend::actor().expect("actor"),
            backend.clone(),
            backend,
            models,
        )
        .with_native_catalogue(port, None),
    );
    handle_app_server_rpc(
        context,
        json!({"id":41,"method":method.name(),"params":{"cwds":["/tmp/workspace"]}}),
    )
    .await
    .expect("async dispatch")
    .response
}

fn assert_whole_method_error(response: &Value, expected_message: &str) {
    assert_eq!(response["error"]["code"], -32000, "{response}");
    assert_eq!(response["error"]["message"], expected_message, "{response}");
    assert!(response.get("result").is_none(), "{response}");
}

#[tokio::test]
async fn native_capabilities_source_table_accounted_located_failures_preserve_usable_rows_and_paths()
 {
    for method in [Method::Skills, Method::Plugins] {
        let mut observed = report();
        let source = method.matching_source();
        method.with_section(
            &mut observed,
            true,
            SourceEnumeration::Accounted,
            vec![source_error(source.clone())],
        );
        let response = dispatch(method, observed).await;
        assert_eq!(method.rows(&response).map(Vec::len), Some(1), "{response}");
        let errors = method.errors(&response).expect("existing per-entry errors");
        assert_eq!(errors.len(), 1, "{response}");
        let expected_path = match source {
            NativeErrorSource::SkillSource { actual_path }
            | NativeErrorSource::MarketplaceManifest { actual_path } => actual_path,
            _ => panic!("matching source role must expose its actual path"),
        };
        let path_key = match method {
            Method::Skills => "path",
            Method::Plugins => "marketplacePath",
        };
        assert_eq!(errors[0][path_key], json!(expected_path.as_path()));
        assert_eq!(
            errors[0]["message"],
            CatalogueReadFailure::InvalidNativeData.to_string()
        );
    }
}

#[tokio::test]
async fn native_capabilities_source_table_wrong_role_unlocated_mixed_and_all_unrepresentable_fail_whole_method()
 {
    for method in [Method::Skills, Method::Plugins] {
        let other_role = match method {
            Method::Skills => Method::Plugins.matching_source(),
            Method::Plugins => Method::Skills.matching_source(),
        };
        for source in [
            other_role.clone(),
            NativeErrorSource::OtherFilesystem {
                actual_path: CapabilityPath::new("/native/real-package-directory".into())
                    .expect("real other role"),
            },
            NativeErrorSource::Unobserved,
        ] {
            for include_matching in [false, true] {
                let mut observed = report();
                let mut errors = vec![source_error(source.clone())];
                if include_matching {
                    errors.push(source_error(method.matching_source()));
                }
                method.with_section(&mut observed, true, SourceEnumeration::Accounted, errors);
                let response = dispatch(method, observed).await;
                assert_whole_method_error(
                    &response,
                    "native filesystem catalogue cannot be represented completely: native source-error role or location is unrepresentable (1 entries)",
                );
            }
        }
        let mut observed = report();
        method.with_section(
            &mut observed,
            true,
            SourceEnumeration::Accounted,
            vec![
                source_error(other_role),
                source_error(NativeErrorSource::OtherFilesystem {
                    actual_path: CapabilityPath::new("/native/other-unrepresentable-source".into())
                        .expect("actual other source"),
                }),
                source_error(NativeErrorSource::Unobserved),
                source_error(method.matching_source()),
            ],
        );
        let response = dispatch(method, observed).await;
        assert_whole_method_error(
            &response,
            "native filesystem catalogue cannot be represented completely: native source-error role or location is unrepresentable (3 entries)",
        );
        let mut observed = report();
        method.with_section(
            &mut observed,
            false,
            SourceEnumeration::Accounted,
            vec![
                source_error(method.matching_source()),
                source_error(NativeErrorSource::Unobserved),
            ],
        );
        let response = dispatch(method, observed).await;
        assert_whole_method_error(
            &response,
            "native filesystem catalogue cannot be represented completely: all native filesystem candidates are unrepresentable (2 entries)",
        );
    }
}

#[tokio::test]
async fn native_capabilities_source_table_empty_success_incomplete_and_section_independence_both_directions()
 {
    for method in [Method::Skills, Method::Plugins] {
        for has_items in [false, true] {
            let mut observed = report();
            method.with_section(
                &mut observed,
                has_items,
                SourceEnumeration::Accounted,
                Vec::new(),
            );
            let response = dispatch(method, observed).await;
            assert_eq!(
                method.rows(&response).map(Vec::len),
                Some(usize::from(has_items)),
                "{response}"
            );
            assert_eq!(method.errors(&response).map(Vec::len), Some(0));
            for errors in [Vec::new(), vec![source_error(method.matching_source())]] {
                let mut observed = report();
                method.with_section(&mut observed, has_items, incomplete(), errors);
                let response = dispatch(method, observed).await;
                assert_whole_method_error(
                    &response,
                    "native filesystem catalogue cannot be represented completely: incomplete native filesystem view: source origin unobserved (1 limitations)",
                );
            }
        }
        let other = match method {
            Method::Skills => Method::Plugins,
            Method::Plugins => Method::Skills,
        };
        for unsupported in [false, true] {
            let mut observed = report();
            if unsupported {
                match other {
                    Method::Skills => {
                        observed.skills = CatalogueSection::Unsupported {
                            reason: CatalogueUnsupported::ScopeUnavailable,
                        }
                    }
                    Method::Plugins => {
                        observed.plugins = CatalogueSection::Unsupported {
                            reason: CatalogueUnsupported::ScopeUnavailable,
                        }
                    }
                }
            } else {
                other.with_section(&mut observed, false, incomplete(), Vec::new());
            }
            let response = dispatch(method, observed).await;
            assert_eq!(method.rows(&response).map(Vec::len), Some(1), "{response}");
        }
        let mut observed = report();
        match method {
            Method::Skills => {
                observed.skills = CatalogueSection::Unsupported {
                    reason: CatalogueUnsupported::NotAdvertised,
                }
            }
            Method::Plugins => {
                observed.plugins = CatalogueSection::Unsupported {
                    reason: CatalogueUnsupported::NotAdvertised,
                }
            }
        }
        let response = dispatch(method, observed).await;
        assert_whole_method_error(
            &response,
            "native capability discovery is unsupported for this provider binding",
        );
    }
}
