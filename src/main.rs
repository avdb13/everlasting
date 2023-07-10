use crate::magnet::MagnetInfo;
use bendy::decoding::FromBencode;
use data::{File, GeneralError, TorrentInfo};
use dht::bootstrap_dht;
use lazy_static::lazy_static;
use rand::Rng;
use router::UdpTracker;
use std::sync::Arc;
use tracker::HttpTracker;
use url::Url;

use crate::peer::Router;

use color_eyre::Report;

use udp::Response;

use std::collections::HashMap;
use std::fs::read_to_string;
use std::io::{stdin, BufRead, BufReader, ErrorKind, Read, Write};
use std::net::SocketAddr;

use std::path::Path;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{channel, Receiver, Sender};

use tracing::debug;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

pub mod app;
pub mod bencode;
pub mod data;
pub mod dht;
pub mod extensions;
pub mod helpers;
pub mod krpc;
pub mod magnet;
pub mod peer;
pub mod router;
pub mod scrape;
pub mod socket;
pub mod state;
pub mod torrent;
pub mod tracker;
pub mod tracker_session;
pub mod udp;
pub mod writer;

lazy_static! {
    static ref PEER_ID: [u8; 20] = rand::thread_rng().gen();
    static ref BITTORRENT_PORT: u16 = 1317;
}

pub const PEER_ID_PREFIX: &str = "xv";

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

    // let mut input = String::new();
    // stdin().read_line(&mut input)?;

    // if !Path::new(&input).exists() {
    //     return Err(GeneralError::NonExistentFile.into());
    // }

    let torrent = std::fs::read("/home/mikoto/everlasting/test.torrent")?;
    let torrent_info = TorrentInfo::from_bencode(&torrent).unwrap();

    let (tracker, mut peer_rx) = HttpTracker::new(&torrent_info);
    tokio::spawn(tracker.run(torrent_info.clone()));

    while let Some(v) = peer_rx.recv().await {
        debug!(?v);
    }

    Ok(())
}
