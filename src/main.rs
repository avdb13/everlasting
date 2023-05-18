use router::Tracker;

use crate::app::App;
use app::Action;

use color_eyre::Report;

use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

use data::{File, SocketResponse};
use futures_util::future;
use once_cell::sync::OnceCell;

use rand::Rng;

use tokio::time::sleep;
use udp::Response;

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};

use std::io;
use std::sync::Arc;
use std::time::Duration;
use std::vec::IntoIter;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, unbounded_channel, UnboundedReceiver, UnboundedSender};
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
        .with(tracing_subscriber::fmt::layer().without_time())
        .init();

    let mut s = std::fs::File::open("magnet")?;
    let mut buf: Vec<u8> = Vec::new();
    s.read_to_end(&mut buf)?;

    let magnet = magnet_decoder(std::str::from_utf8(&buf)?)?;

    let src: SocketAddr = "0.0.0.0:1317".parse()?;
    let socket = Arc::new(UdpSocket::bind(src).await?);

    let (tx, rx) = watch::channel(None::<router::Message>);
    let tracker = Tracker::new(magnet, socket.clone(), rx)?;

    tokio::spawn(async move { Tracker::listen(socket.clone(), tx).await });
    let peers = tracker.run().await?;
    debug!("peers found: {:?}", peers);

    Ok(())
}
