use color_eyre::Report;
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
pub async fn main() -> Result<(), Report> {
    color_eyre::install()?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "daemon=trace".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .with(console_subscriber::spawn())
        .init();

    Ok(())
}
