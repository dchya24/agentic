mod stdio_transport;

use core_agentic::runtime::engine::RuntimeEngine;
use stdio_transport::StdioTransport;
use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("warn")),
        )
        .init();

    let mut engine = RuntimeEngine::new(StdioTransport::new());
    engine.run();
}
