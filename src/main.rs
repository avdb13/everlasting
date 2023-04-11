#![feature(try_blocks)]

use async_convert::{async_trait, TryFrom};
use peer::PeerRouter;
use router::{Router, State};

use crate::app::App;
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
pub mod peer;
pub mod router;
pub mod scrape;
pub mod state;
pub mod udp;
pub mod writer;

use crate::helpers::*;

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
    #[error("unexpected response: {0:?}")]
    UnexpectedResponse(Response),
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
        .with(tracing_subscriber::fmt::layer().without_time())
        .init();

    let mut s = std::fs::File::open("magnet")?;
    let mut buf: Vec<u8> = Vec::new();
    s.read_to_end(&mut buf)?;

    let magnet = magnet_decoder(std::str::from_utf8(&buf)?)?;

    let src = "0.0.0.0:1317".parse()?;
    let socket = Arc::new(UdpSocket::bind(src).await?);

    let router: Router = Router::new(src, magnet.clone(), socket);
    let router = router.next(router::Event::Connect).await?;
    let router = router.next(router::Event::Announce).await?;

    if let State::Announced { peers, .. } = router.state {
        let r = PeerRouter::new(magnet.clone(), peers);
        r.next().await?;
    }

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
