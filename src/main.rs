#![feature(vec_push_within_capacity)]
#![feature(slice_take)]

use bendy::decoding::FromBencode;
use data::TorrentInfo;

use lazy_static::lazy_static;
use rand::distributions::Alphanumeric;
use rand::Rng;

use tracing::debug;
use tracker::HttpTracker;

use crate::{fs::create_layout, peer::Router};

use color_eyre::Report;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod app;
pub mod bencode;
pub mod data;
pub mod dht;
pub mod extensions;
pub mod framing;
pub mod fs;
pub mod helpers;
pub mod krpc;
pub mod magnet;
pub mod nameless;
pub mod peer;
pub mod piece_manager;
pub mod pwp;
pub mod scrape;
pub mod socket;
pub mod state;
pub mod torrent;
pub mod tracker;
pub mod tracker_session;
pub mod udp;
// pub mod writer;

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
    dbg!("program started");
    color_eyre::install()?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "everlasting=debug".into()),
            // .unwrap_or_else(|_| "everlasting=debug,tokio=trace,runtime=trace".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .with(console_subscriber::spawn())
        .init();
    dbg!("tracing_subscriber and color_eyre done setting up");

    let torrent = std::fs::read("/home/mikoto/everlasting/music.torrent")?;
    let torrent_info = TorrentInfo::from_bencode(&torrent).unwrap();

    let ok = torrent_info.file_layout();
    create_layout(ok)?;

    panic!();

    let (tracker, peer_rx) = HttpTracker::new(&torrent_info);
    tokio::spawn(tracker.run(torrent_info.clone()));

    let router = Router::new(torrent_info.clone(), peer_rx);
    router.run().await;

    loop {}

    Ok(())
}
