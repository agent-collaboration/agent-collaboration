//! Opt-in sink for fixed startup events and existing scrubbed child diagnostics.
use tracing_subscriber::{filter::filter_fn, layer::SubscriberExt, util::SubscriberInitExt};

pub(super) fn enable_startup_diagnostics() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("CODEX_ROUTER_DEBUG_READINESS_TIMING").as_deref()
        == Some(std::ffi::OsStr::new("1"))
    {
        tracing_subscriber::registry()
            .with(diagnostic_layer(std::io::stderr))
            .try_init()?;
    }
    Ok(())
}

fn diagnostic_layer<Writer>(
    writer: Writer,
) -> impl tracing_subscriber::Layer<tracing_subscriber::Registry>
where
    Writer: for<'writer> tracing_subscriber::fmt::MakeWriter<'writer> + Send + Sync + 'static,
{
    use tracing_subscriber::Layer;
    tracing_subscriber::fmt::layer()
        .json()
        .with_current_span(false)
        .with_span_list(false)
        .with_writer(writer)
        .with_filter(filter_fn(|metadata| {
            matches!(
                metadata.target(),
                "codex_router_host::startup_diagnostics"
                    | "codex_router_host::child_diagnostics"
                    | "acp_client_runtime::provider_diagnostics"
                    | "collaboration_runtime::native_settings_diagnostic"
            )
        }))
}

#[cfg(test)]
#[path = "startup_diagnostics_test.rs"]
mod tests;
