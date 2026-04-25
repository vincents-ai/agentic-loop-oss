//! Tracing initialization — fmt layer + optional OTLP exporter.
//!
//! Controlled by environment variables:
//! - `RUST_LOG`: filter directives (e.g. "info,agentic_core=debug")
//! - `OTEL_EXPORTER_OTLP_ENDPOINT`: OTLP endpoint (e.g. "http://localhost:4317")
//! - `AGENTIC_LOOP_TRACING`: "off" to disable, "otel" for OTLP+fmt, "fmt" for fmt only (default)

use std::sync::OnceLock;
use opentelemetry::trace::TracerProvider;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

static PROVIDER: OnceLock<opentelemetry_sdk::trace::SdkTracerProvider> = OnceLock::new();

/// Initialize tracing. Call once at program start.
pub fn init() {
    let mode = std::env::var("AGENTIC_LOOP_TRACING")
        .unwrap_or_else(|_| "fmt".to_string());

    if mode == "off" {
        return;
    }

    if mode == "otel" {
        match init_otel() {
            Ok(()) => {
                tracing::info!("OTel tracing initialized");
            }
            Err(e) => {
                init_fmt_only();
                tracing::warn!("OTel init failed ({ }), falling back to fmt-only", e);
            }
        }
    } else {
        init_fmt_only();
    }
}

/// Shut down tracing providers. Call before program exit to flush pending spans.
pub fn shutdown() {
    if let Some(provider) = PROVIDER.get() {
        let _ = provider.shutdown();
    }
}

fn init_fmt_only() {
    let env_filter = EnvFilter::from_env("RUST_LOG")
        .add_directive("warn".parse().expect("static string parse should not fail"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .init();
}

fn init_otel() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use opentelemetry_otlp::WithExportConfig;

    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4317".to_string());

    // Build OTLP exporter (gRPC/Tonic)
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_tonic()
        .with_endpoint(endpoint)
        .build()?;

    // Build tracer provider with batch exporter
    let provider = opentelemetry_sdk::trace::SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .build();

    // Get a tracer from the provider
    let tracer = provider.tracer("agentic-loop");

    // Store provider for clean shutdown
    let _ = PROVIDER.set(provider);

    // Build OTel tracing layer
    let otel_filter = EnvFilter::from_env("RUST_LOG")
        .add_directive("warn".parse().expect("static string parse should not fail"));
    let otel_layer = tracing_opentelemetry::layer()
        .with_tracer(tracer)
        .with_filter(otel_filter);

    // Build fmt layer
    let fmt_filter = EnvFilter::from_env("RUST_LOG")
        .add_directive("warn".parse().expect("static string parse should not fail"));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .with_filter(fmt_filter);

    // Install both layers
    tracing_subscriber::registry()
        .with(otel_layer)
        .with(fmt_layer)
        .init();

    Ok(())
}
