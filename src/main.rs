use bendy::decoding::FromBencode;
use data::TorrentInfo;

use lazy_static::lazy_static;
use rand::distributions::Alphanumeric;
use rand::Rng;

use tokio::fs::create_dir;
use tracing::debug;
use tracker::HttpTracker;

use crate::peer::Router;

use color_eyre::Report;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod app;
pub mod bencode;
pub mod data;
pub mod dht;
pub mod extensions;
pub mod framing;
pub mod helpers;
pub mod krpc;
pub mod magnet;
pub mod nameless;
pub mod peer;
pub mod piece_map;
pub mod pwp;
pub mod scrape;
pub mod socket;
pub mod state;
pub mod torrent;
pub mod tracker;
pub mod tracker_session;
pub mod udp;
pub mod writer;

lazy_static! {
    static ref BLOCK_SIZE: usize = 2 ^ 14;
    static ref BITTORRENT_PORT: u16 = 1317;
    static ref PEER_ID_PREFIX: &'static str = "XV";
    static ref SUFFIX: Vec<char> = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(16)
        .map(Into::<char>::into)
        .collect();
    static ref PEER_ID: [u8; 20] = format!(
        "-XV{}-{}",
        SUFFIX[..4].iter().collect::<String>(),
        SUFFIX[4..].iter().collect::<String>()
    )
    .as_bytes()
    .try_into()
    .unwrap();
}

#[tokio::main]
async fn main() -> Result<(), Report> {
    color_eyre::install()?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "everlasting=debug".into()),
            // .unwrap_or_else(|_| "everlasting=debug,tokio=trace,runtime=trace".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .with(console_subscriber::spawn())
        .init();

    let torrent = std::fs::read("/home/mikoto/everlasting/test.torrent")?;
    let torrent_info = TorrentInfo::from_bencode(&torrent).unwrap();

    create_dir("./pieces").await?;

    let (tracker, peer_rx) = HttpTracker::new(&torrent_info);
    tokio::spawn(tracker.run(torrent_info.clone()));

    let router = Router::new(torrent_info.clone(), peer_rx);
    router.run().await;

    loop {}

    Ok(())
}
