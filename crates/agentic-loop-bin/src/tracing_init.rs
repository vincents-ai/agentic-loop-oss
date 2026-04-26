//! Tracing initialization for the agentic-loop binary.
//!
//! OSS version: basic tracing_subscriber with env filter.
//! Commercial version adds OpenTelemetry export.

use tracing_subscriber::{EnvFilter, fmt};

/// Initialize tracing with RUST_LOG env filter.
pub fn init() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info"));

    fmt::Subscriber::builder()
        .with_env_filter(filter)
        .with_target(false)
        .init();
}

/// Shutdown tracing (no-op for basic subscriber).
pub fn shutdown() {
    // No-op — OTel shutdown would go here in commercial version
}
