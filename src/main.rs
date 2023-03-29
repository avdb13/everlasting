use crate::app::App;
use crate::udp::{AnnounceReq, Event};
use app::Action;

use color_eyre::Report;

use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};

use data::File;
use futures_util::future;
use once_cell::sync::OnceCell;

use rand::Rng;

use router::Router;
use tokio::time::sleep;

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};

use std::io;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::mpsc::{self, unbounded_channel, UnboundedReceiver, UnboundedSender};
use tokio::sync::oneshot;
use tokio::sync::Mutex;

use tracing::debug;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tui::backend::CrosstermBackend;
use tui::Terminal;
use url::Url;
use writer::Writer;

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
    color_eyre::install()?;

    let panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        reset_terminal().unwrap();
        panic(info)
    }));

    let (tx_stdout, rx_stdout): (UnboundedSender<String>, UnboundedReceiver<String>) =
        unbounded_channel();
    let writer = Writer::new(tx_stdout);
    let clone = writer.clone();

    tracing_subscriber::registry()
        // .with(tracing_subscriber::EnvFilter::new(
        //     std::env::var("RUST_LOG").unwrap_or_else(|_| "everlasting=debug".into()),
        // ))
        .with(
            tracing_subscriber::fmt::layer()
                .without_time()
                .with_level(false)
                .with_target(false)
                .with_ansi(false)
                .with_writer(move || clone.clone()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let mut s = std::fs::File::open("magnet")?;
    let mut buf: Vec<u8> = Vec::new();
    s.read_to_end(&mut buf)?;

    let magnet_info = magnet_decoder(std::str::from_utf8(&buf)?)?;

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

    //     enable_raw_mode()?;
    //     execute!(&mut io::stdout(), EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(io::stdout());
    let mut term = Terminal::new(backend)?;
    term.hide_cursor()?;

    let (tx_actions, mut rx_actions): (mpsc::Sender<Action>, mpsc::Receiver<Action>) =
        mpsc::channel(100);
    let (tx_err, rx_err): (oneshot::Sender<Report>, oneshot::Receiver<Report>) = oneshot::channel();

    let tick_rate = Duration::from_millis(250);
    let mut app = App::new(tx_actions.clone(), tick_rate);

    tokio::spawn(async move {
        while let Some(msg) = rx_actions.recv().await {
            let router = router.lock().await;

            if let Err(e) = match msg {
                Action::Connect => connect(&router, src).await,
                Action::Announce => announce(&router).await,
            } {
                tx_err.send(e).unwrap();
                break;
            }
        }
    });

    tx_actions.send(Action::Connect).await?;
    tx_actions.send(Action::Announce).await?;

    app.run(&mut term, (rx_stdout, rx_err)).await?;

    reset_terminal()?;

    Ok(())
}

pub async fn connect(router: &Router, src: SocketAddr) -> Result<(), Report> {
    let packet = ConnectReq::build();
    let magnet_info = &router.queue[0];

    if let Some(dst) = router.target.get() {
        let src = "0.0.0.0:1317".parse()?;

        return match router.connect(src, *dst, packet).await {
            Ok(_) => Ok(()),
            Err(e) => Err(e),
        };
    }

    let iter = magnet_info.trackers.iter().map(|s| {
        let packet = packet.clone();
        let router = router.clone();

        Box::pin(async move {
            debug!("trying {} as tracker ...", s);
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

    let ((resp, dst), _) = future::select_ok(iter).await?;

    tracing::debug!("active tracker found!");

    match resp {
        r if r.action == 0i32 && r.tid == packet.tid => {
            router.target.set(dst).unwrap();
            router.cid.set(r.cid).unwrap();

            debug!("transaction ID matches: ({}, {})", packet.tid, r.tid);
            debug!("connection ID is {}", r.cid);

            Ok(())
        }
        _ => Err(GeneralError::DeadTrackers.into()),
    }
}

pub async fn announce(router: &Router) -> Result<(), Report> {
    let peer_id = rand::thread_rng().gen::<[u8; 20]>();
    let key = rand::thread_rng().gen::<u32>();
    // , , , , , , , , , ]
    let req = AnnounceReq {
        // 226, 191, 106, 55, 0, 203, 11, 102
        cid: *router.cid.get().unwrap(),
        // 0, 0, 0, 1,
        action: 1i32,
        // 187, 240, 248, 25
        tid: rand::thread_rng().gen::<i32>(),
        // 107, 141, 17, 107, 72,
        // 109, 190, 36, 32, 218,
        // 5, 58, 246, 92, 241,
        // 223, 1, 153, 60, 150
        hash: &decode(router.queue[0].hash.clone()),
        // 4, 196, 228, 182, 13,
        // 94, 69, 81, 197, 103,
        // 222, 193, 115, 116, 56,
        // 13, 216, 222, 119, 231
        peer_id: &peer_id,
        // 0, 0, 0, 0, 0, 0, 0, 0
        downloaded: 0,
        // 0, 0, 0, 0, 0, 0, 0, 0
        left: 0,
        // 0, 0, 0, 0, 0, 0, 0, 0
        uploaded: 0,
        // 3, 0, 0, 0, 0
        event: Event::Inactive,
        // 41, 173, 181, 165
        socket: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
        // 255, 255, 255, 255
        key,
        // 0, 0, 0, 0
        num_want: -1i32,
        extensions: 0u16,
    };

    let resp = router.announce(req.clone()).await?;

    if let Some(_) = resp.clone().left() {
        let src = "0.0.0.0:1317".parse()?;
        connect(router, src).await?;
    } else if let Some(resp) = resp.clone().right() {
        debug!("interval: {}", resp.interval);
    }

    Ok(())
}
