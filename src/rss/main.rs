use color_eyre::Report;
use rss::Channel;
use tracing::debug;
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
pub async fn main() -> Result<(), Report> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "rss=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let result = reqwest::get("https://subsplease.org/rss/?r=1080").await?;
    let buf = result.bytes().await?;

    let channel = Channel::read_from(&buf[..])?;
    let titles: Vec<_> = channel.items().iter().flat_map(|x| x.title()).collect();
    dbg!(titles);

    Ok(())
}
pub struct RSSFetcher;

impl RSSFetcher {
    async fn fetch() -> Result<(), Report> {
        Ok(())
    }
}
