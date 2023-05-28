#![allow(warnings)]
#![feature(try_blocks)]
#![feature(async_closure)]
use bendy::decoding::{FromBencode, ResultExt};
use router::Tracker;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt};

use crate::app::App;
use crate::peer::PeerRouter;
use crate::router::Message;
use app::Action;

use color_eyre::Report;

use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

use data::{Metadata, Mode, SocketResponse};
use futures_util::future;
use once_cell::sync::OnceCell;

use rand::Rng;

use tokio::time::sleep;
use udp::Response;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};

use std::io;
use std::sync::Arc;
use std::time::Duration;
use std::vec::IntoIter;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{
    self, channel, unbounded_channel, Receiver, Sender, UnboundedReceiver, UnboundedSender,
};
use tokio::sync::Mutex;
use tokio::sync::{oneshot, watch};

use tracing::{debug, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tui::backend::CrosstermBackend;
use tui::Terminal;
use url::Url;
use writer::Writer;

pub mod app;
pub mod bencode;
pub mod data;
pub mod helpers;
pub mod peer;
pub mod router;
pub mod scrape;
pub mod socket;
pub mod state;
pub mod udp;
pub mod writer;

use crate::helpers::*;

#[tokio::main]
async fn main() -> Result<(), Report> {
    color_eyre::install()?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "everlasting=debug".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mut s = File::open("test.torrent")?;
    let mut buf: Vec<u8> = Vec::new();
    s.read_to_end(&mut buf)?;

    let mut torrent = Metadata::from_bencode(&buf).expect("torrent file not found!");

    // let mut s = File::open("magnet")?;
    // let mut buf: Vec<u8> = Vec::new();
    // s.read_to_end(&mut buf)?;

    let trackers = torrent
        .announce_list
        .unwrap()
        .iter()
        .flatten()
        .flat_map(|s| udp_address_to_ip(s))
        .collect();
    let magnet = match torrent.info.mode {
        Mode::Single { name, .. } => MagnetInfo {
            hash: torrent.info.value,
            name,
            trackers,
        },
        Mode::Multi { dir_name, .. } => MagnetInfo {
            hash: torrent.info.value,
            name: dir_name,
            trackers,
        },
    };
    // let magnet = magnet_decoder(std::str::from_utf8(&buf)?)?;

    let peers = match File::open("peers") {
        Ok(mut f) => {
            debug!("peers are already found!");

            let lines = BufReader::new(f).lines();
            lines
                .into_iter()
                .map(|x| x.unwrap().parse::<SocketAddr>().unwrap())
                .collect::<Vec<_>>()
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let (socket, (tx, rx)) = tracker_setup(&magnet).await?;

            let tracker = Tracker::new(&magnet, socket.clone(), rx)?;
            let handle = tokio::spawn(async move { Tracker::listen(socket.clone(), tx).await });

            let peers = tracker.run().await?;

            let mut out = File::create("peers")?;
            for peer in &peers {
                writeln!(out, "{peer:?}")?;
            }

            handle.abort();

            peers
        }
        Err(e) => return Err(e.into()),
    };

    let mut r = PeerRouter::new(magnet);
    r.connect(peers.clone()).await?;

    Ok(())
}

async fn tracker_setup(magnet: &MagnetInfo) -> Result<Artifacts, Report> {
    let map: Vec<_> = magnet
        .trackers
        .iter()
        .map(|&s| {
            let (tx, rx) = channel::<Response>(5);
            ((s, tx), (s, rx))
        })
        .collect();
    let (tx, rx): (HashMap<_, _>, HashMap<_, _>) = map.into_iter().unzip();

    let src: SocketAddr = "0.0.0.0:1317".parse()?;
    let socket = Arc::new(UdpSocket::bind(src).await?);

    Ok((socket, (tx, rx)))
}

type Artifacts = (
    Arc<UdpSocket>,
    (
        HashMap<SocketAddr, Sender<Response>>,
        HashMap<SocketAddr, Receiver<Response>>,
    ),
);
