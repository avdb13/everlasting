use crate::app::App;
use crate::udp::{AnnounceReq, Event, Request, Response};
use app::Action;
use color_eyre::{eyre::eyre, Report};
use crossterm::event::DisableMouseCapture;
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crypto::digest::Digest;
use crypto::sha1::Sha1;
use futures_util::future;
use once_cell::sync::OnceCell;
use rand::seq::SliceRandom;
use rand::Rng;
use reqwest::ClientBuilder;
use router::Router;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;
use std::{fs, io};
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, Receiver, Sender};
use tokio::sync::oneshot;
use tokio::sync::Mutex;
use tokio::time::sleep;
use tracing::debug;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tui::backend::CrosstermBackend;
use tui::Terminal;
use url::Url;
use writer::MyWriter;

pub mod app;
pub mod bencode;
pub mod data;
pub mod helpers;
pub mod router;
pub mod scrape;
pub mod state;
pub mod udp;
pub mod writer;

use crate::helpers::*;
use crate::udp::ConnectReq;
use crate::{data::*, scrape::Scraper};

use bendy::{decoding::FromBencode, encoding::Error};

pub struct HttpRequest {
    hash: [u8; 20],
    id: [u8; 20],
    port: u16,
    uploaded: u64,
    downloaded: u64,
    compact: bool,
}

#[derive(Error, Debug)]
pub enum GeneralError {
    #[error("usage: everlasting [torrent file | magnet link]")]
    Usage,
    #[error("magnet link contains no valid trackers: `{0:?}`")]
    InvalidTrackers(Vec<String>),
    #[error("no active trackers for this torrent")]
    DeadTrackers,
    #[error("Timeout")]
    Timeout,
}

const PROTOCOL_ID: i64 = 0x41727101980;

fn reset_terminal() -> Result<(), Report> {
    disable_raw_mode()?;
    crossterm::execute!(io::stdout(), LeaveAlternateScreen)?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Report> {
    let panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        reset_terminal().unwrap();
        panic(info)
    }));

    color_eyre::install()?;

    let writer = MyWriter::default();

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "everlasting=debug".into()),
        ))
        .with(
            tracing_subscriber::fmt::layer()
                .without_time()
                .with_level(false)
                .with_target(false)
                .with_ansi(false)
                .with_writer(move || writer.clone()),
        )
        .init();

    let s = std::env::args().nth(1).ok_or(GeneralError::Usage)?;
    let magnet_info = magnet_decoder(s.clone())?;

    let src = "0.0.0.0:1317".parse()?;
    let socket = Arc::new(UdpSocket::bind(src).await?);
    let router = Arc::new(Mutex::new(Router {
        tid: OnceCell::new(),
        target: OnceCell::new(),
        cid: OnceCell::new(),
        socket,
        connected: false,
        queue: vec![magnet_info],
    }));

    enable_raw_mode()?;
    execute!(&mut io::stdout(), EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut term = Terminal::new(backend)?;
    term.hide_cursor()?;

    let (tx, mut rx): (mpsc::Sender<Action>, mpsc::Receiver<Action>) = mpsc::channel(10);
    let (err_tx, mut err_rx): (oneshot::Sender<Report>, oneshot::Receiver<Report>) =
        oneshot::channel();

    let tick_rate = Duration::from_millis(250);
    let mut app = App::new(tx, tick_rate, writer.clone());

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let router = router.lock().await;

            if let Err(e) = match msg {
                Action::Connect => connect(&router, src).await,
                Action::Announce => announce(&router).await,
            } {
                err_tx.send(e).unwrap();
                break;
            }
        }
    });

    app.run(&mut term).await?;

    reset_terminal()?;

    Ok(())
}

pub async fn connect(router: &Router, src: SocketAddr) -> Result<(), Report> {
    let packet = ConnectReq::build();
    let magnet_info = &router.queue[0];

    let iter = magnet_info.trackers.iter().map(|s| {
        let packet = packet.clone();
        let router = router.clone();

        Box::pin(async move {
            tracing::debug!("trying {} as tracker ...", s);
            let url = Url::parse(s)?;
            let dst = (url.host_str().unwrap(), url.port().unwrap())
                .to_socket_addrs()
                .unwrap()
                .next()
                .unwrap();

            match router.connect(src, dst, packet).await {
                Ok(x) => Ok((x, dst)),
                Err(e) => Err(e),
            }
        })
    });

    let (resp, _, _) = future::select_all(iter).await;

    tracing::debug!("active tracker found!");

    match resp? {
        x if x.0.action == 0i32 && x.0.tid == packet.tid => {
            router.target.set(x.1).unwrap();
            router.cid.set(x.0.cid).unwrap();

            debug!("transaction ID matches: ({}, {})", packet.tid, x.0.tid);
            debug!("connection ID is {}", x.0.cid);

            Ok(())
        }
        _ => Err(GeneralError::DeadTrackers.into()),
    }
}

pub async fn announce(router: &Router) -> Result<(), Report> {
    let peer_id = rand::thread_rng().gen::<[u8; 20]>();
    let key = rand::thread_rng().gen::<u32>();

    let req = AnnounceReq {
        cid: *router.cid.get().unwrap(),
        action: 1i32,
        tid: rand::thread_rng().gen::<i32>(),
        hash: &decode(router.queue[0].hash.clone()),
        peer_id: &peer_id,
        downloaded: 0,
        left: 0,
        uploaded: 0,
        event: Event::Stopped,
        socket: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
        key,
        num_want: -1i32,
        extensions: 0u16,
    };
    debug!("{:?}", req);

    router.announce(req).await?;

    Ok(())
}
