#![feature(try_blocks)]

use async_convert::{async_trait, TryFrom};

use crate::app::App;
use crate::udp::{AnnounceReq, ConnectResp, Event};
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

use router::{Announcing, Connected, Disconnected, Router};
use tokio::time::sleep;
use udp::{AnnounceResp, Response};

use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};

use std::io;
use std::sync::Arc;
use std::time::Duration;
use std::vec::IntoIter;
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
    InvalidTracker(String),
    #[error("no active trackers for this torrent")]
    DeadTrackers,
    #[error("timeout")]
    Timeout,
    #[error("reconnect")]
    Reconnect,
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
    // let clone = writer.clone();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .without_time()
                .with_level(false)
                .with_target(false)
                .with_ansi(false), // .with_writer(move || clone.clone()),
        )
        .init();

    let mut s = std::fs::File::open("magnet")?;
    let mut buf: Vec<u8> = Vec::new();
    s.read_to_end(&mut buf)?;

    let magnet = magnet_decoder(std::str::from_utf8(&buf)?)?;

    let src = "0.0.0.0:1317".parse()?;
    let socket = Arc::new(UdpSocket::bind(src).await?);

    let router: Router<Disconnected> = Router::new(src, magnet, socket);

    // let router = Arc::new(Mutex::new(Router {
    //     tid: OnceCell::new(),
    //     target: OnceCell::new(),
    //     cid: OnceCell::new(),
    //     socket,
    //     connected: false,
    //     queue: vec![magnet_info],
    // }));

    //     enable_raw_mode()?;
    //     execute!(&mut io::stdout(), EnterAlternateScreen)?;

    let router: Router<Connected> = <Router<router::Connected> as async_convert::TryFrom<
        Router<Disconnected>,
    >>::try_from(router)
    .await?;

    let router: Router<Announcing> =
        <Router<router::Announcing> as async_convert::TryFrom<Router<Connected>>>::try_from(router)
            .await?;

    // let backend = CrosstermBackend::new(io::stdout());
    // let mut term = Terminal::new(backend)?;
    // term.hide_cursor()?;

    let (tx_actions, mut rx_actions): (mpsc::Sender<Action>, mpsc::Receiver<Action>) =
        mpsc::channel(100);
    let (tx_err, mut rx_err): (mpsc::Sender<Report>, mpsc::Receiver<Report>) = mpsc::channel(1);

    let tick_rate = Duration::from_millis(250);
    let mut app = App::new(tx_actions.clone(), tick_rate);

    // tokio::spawn(async move {
    //     while let Some(msg) = rx_actions.recv().await {
    //         let router = router.lock().await;

    //         let ok = match msg {
    //             Action::Connect => connect(&router, src).await.map(|_| ()),
    //             Action::Announce => announce(&router).await.map(|_| ()),
    //         };

    //         match ok {
    //             Err(e) => {
    //                 let _ = tx_err.send(e).await;
    //             }
    //             Ok(()) => {}
    //             // Ok(resp) => match resp[..4] {
    //             //     [0, 0, 0, 0] => return Err(GeneralError::Reconnect.into()),
    //             //     [0, 0, 0, 1] => {
    //             //         let resp = AnnounceResp::to_response(&resp[..n])?;
    //             //         debug!("[ANNOUNCE] found peers: {:?}", resp.peers);
    //             //     }
    //             //     [0, 0, 0, 3] => {
    //             //         tracing::error!("[ANNOUNCE] error: {:?}", std::str::from_utf8(&resp[8..n])?)
    //             //     }
    //             //     _ => {}
    //             // },
    //         }
    //     }
    // });

    // app.run(&mut term, (rx_stdout, rx_err)).await?;

    // reset_terminal()?;

    if let Some(e) = rx_err.recv().await {
        return Err(e);
    }

    Ok(())
}

// pub async fn announce(router: &Router<Announcing>) -> Result<AnnounceResp, Report> {
//     let peer_id = rand::thread_rng().gen::<[u8; 20]>();
//     let key = rand::thread_rng().gen::<u32>();

//     let req = AnnounceReq {
//         cid: *router.cid.get().unwrap(),
//         action: 1i32,
//         tid: rand::thread_rng().gen::<i32>(),
//         hash: &decode(router.queue[0].hash.clone()),
//         peer_id: &peer_id,
//         downloaded: 0,
//         left: 0,
//         uploaded: 0,
//         event: Event::Inactive,
//         socket: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
//         key,
//         num_want: -1i32,
//         extensions: 0u16,
//     };

//     ConnectResp::to_response(&router.announce(req.clone()).await?)
// }
