use crate::data::MagnetInfo;
use dht::Node;
use kv::{Config, Store};
use router::Tracker;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracker::to_torrent;

use crate::peer::Router;

use color_eyre::Report;

use udp::Response;

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
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
pub mod helpers;
pub mod krpc;
pub mod peer;
pub mod router;
pub mod scrape;
pub mod socket;
pub mod state;
mod tracker;
pub mod udp;
pub mod writer;

use crate::helpers::*;

#[tokio::main]
async fn main() -> Result<(), Report> {
    color_eyre::install()?;

    let store = Store::new(Config::new("./kv.conf"))?;
    // store.bucket::<Node, Vec<SocketAddr>>(Some("DHT"));

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "everlasting=debug".into()),
            // .unwrap_or_else(|_| "everlasting=debug,tokio=trace,runtime=trace".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .with(console_subscriber::spawn())
        .init();

    let mut s = File::open("magnet")?;
    let mut buf: Vec<u8> = Vec::new();
    s.read_to_end(&mut buf)?;

    let magnet = magnet_decoder(std::str::from_utf8(&buf)?)?;
    // let ok = to_torrent(magnet.clone()).await?;

    let peers = match File::open("peers") {
        Ok(f) => {
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
            let handle = tokio::spawn(listen(socket.clone(), tx));

            let peers = tracker.run().await?;

            let mut out = File::create("peers")?;
            for peer in &peers {
                writeln!(out, "{peer:?}")?;
            }

            peers
        }
        Err(e) => return Err(e.into()),
    };

    let mut r = Router::new(magnet);
    r.connect(peers.clone()).await?;

    Ok(())
}

async fn tracker_setup(magnet: &MagnetInfo) -> Result<Artifacts, Report> {
    let map: Vec<_> = magnet
        .trackers
        .iter()
        .flat_map(|s| {
            let s = udp_to_ip(&s)?;
            let (tx, rx) = channel::<Response>(5);
            Some(((s, tx), (s, rx)))
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

pub async fn listen(socket: Arc<UdpSocket>, tx: HashMap<SocketAddr, Sender<Response>>) {
    loop {
        let mut buf = [0u8; 1024];
        match socket.clone().recv_from(&mut buf).await {
            Ok((n, peer)) => {
                let resp = Response::to_response(&buf[..n]).unwrap();
                debug!("listener received value: {:?}", &resp);

                let tx = tx.get(&peer).unwrap();
                tx.send(resp).await.unwrap();
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
            Err(e) => panic!("{}", e),
        }
    }
}
