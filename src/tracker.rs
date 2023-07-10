use bendy::decoding::{Decoder as BendyDecoder, FromBencode};
use color_eyre::Report;
use dashmap::DashMap;
use rand::Rng;
use std::error::Error as StdError;
use std::{collections::HashMap, io::ErrorKind, net::SocketAddr, sync::Arc, time::Duration};
use tokio::io::AsyncWrite;
use tokio::sync::{mpsc, watch};
use tokio::{
    net::{TcpStream, UdpSocket},
    sync::mpsc::channel,
    task::{JoinHandle, JoinSet},
    time::timeout,
};
use tracing::{debug, field::debug};
use url::Url;

use crate::data::{Event, Peers};
use crate::tracker_session::{HttpSession, Parameters};
use crate::{
    data::{GeneralError, HttpResponse, ScrapeResponse, TorrentInfo, PROTOCOL_ID},
    helpers::{self, decode, encode, udp_to_ip},
    magnet::MagnetInfo,
    udp::{Request, Response, TransferEvent},
};

pub enum TrackerMessage {}

pub struct Tracker {
    receiver: mpsc::Receiver<TrackerMessage>,
}

type ChannelMap = (
    HashMap<SocketAddr, mpsc::Sender<Response>>,
    HashMap<SocketAddr, mpsc::Receiver<Response>>,
);

#[allow(clippy::type_complexity)]
// impl Tracker {
//     pub async fn new_udp(
//         torrent_info: &TorrentInfo,
//     ) -> Result<(UdpTracker, JoinHandle<()>), Report> {
//         let map: Vec<(
//             (SocketAddr, mpsc::Sender<Response>),
//             (SocketAddr, mpsc::Receiver<Response>),
//         )> = torrent_info
//             .announce
//             .1
//             // TODO: allow trackerless torrents
//             .iter()
//             .flat_map(|s| {
//                 let s = udp_to_ip(s)?;
//                 let (tx, rx) = channel::<Response>(5);
//                 Some(((s, tx), (s, rx)))
//             })
//             .collect();
//         let (tx_map, rx_map): ChannelMap = map.into_iter().unzip();

//         let src: SocketAddr = "0.0.0.0:1317".parse()?;
//         let socket = Arc::new(UdpSocket::bind(src).await?);

//         let tracker = UdpTracker::new(socket.clone(), rx_map, torrent_info.info.value)?;
//         let handle = tokio::spawn(UdpTracker::listen(socket, tx_map));

//         Ok((tracker, handle))
//     }

//     pub async fn new(torrent: &TorrentInfo) -> Self {
//         // channel to update parameters
//         let (rx, tx) = mpsc::channel(100);

//         if torrent.announce.udp.is_empty() {
//             let tracker = HttpTracker::new();
//         }
//     }

// pub async fn new_tcp(
//     torrent_info: &TorrentInfo,
// ) -> Result<(HttpTracker, JoinHandle<()>), Report> {
//     Ok(HttpTracker::new(torrent_info,)
// }
// }

// impl UdpTracker {
//     pub fn new(
//         socket: Arc<UdpSocket>,
//         rx_map: HashMap<SocketAddr, Receiver<Response>>,
//         hash: [u8; 20],
//     ) -> Result<Self, Report> {
//         let session_map = rx_map
//             .into_iter()
//             .map(|(addr, rx)| (addr, Session::new(hash, addr, socket.clone(), rx)))
//             .collect();

//         Ok(Self {
//             session_map,
//             hash,
//             socket,
//         })
//     }

//     pub async fn run(self) -> Result<Vec<SocketAddr>, Report> {
//         let mut set = JoinSet::new();
//         let mut working = Vec::with_capacity(self.session_map.len());

//         self.session_map.into_iter().for_each(|(ip, session)| {
//             debug!("resolver spawned for [{}]", ip);
//             set.spawn(session.resolve());
//         });

//         while let Some(f) = set.join_next().await {
//             working.push(f?);
//         }

//         Ok(working.into_iter().flatten().flatten().collect())
//     }
//     pub async fn listen(socket: Arc<UdpSocket>, tx: HashMap<SocketAddr, Sender<Response>>) {
//         loop {
//             let mut buf = [0u8; 1024];
//             match socket.clone().recv_from(&mut buf).await {
//                 Ok((n, peer)) => {
//                     let resp = Response::to_response(&buf[..n]).unwrap();
//                     debug!("listener received value: {:?}", &resp);

//                     let tx = tx.get(&peer).unwrap();
//                     tx.send(resp).await.unwrap();
//                 }
//                 Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
//                 Err(e) => panic!("{}", e),
//             }
//         }
//     }
// }

pub type Message = (SocketAddr, Response);

#[derive(Clone)]
pub enum State {
    Disconnected,
    Connected { cid: i64 },
    Announced { peers: Peers, cid: i64 },
    Scraped,
}

pub struct HttpTracker {
    // pub socket: Arc<reqwest::Client>,
    pub trackers: Vec<HttpSession>,
}

impl Parameters {
    fn tracker_id(&mut self, tracker_id: String) {
        self.tracker_id = Some(tracker_id);
    }
}

// pub type WatchMap = HashMap<String, (watch::Sender<Parameters>, watch::Receiver<Parameters>)>;

impl HttpTracker {
    pub fn new(torrent: &TorrentInfo) -> (Self, mpsc::Receiver<Peers>) {
        let trackers = &torrent.announce;

        let (peer_tx, peer_rx): (mpsc::Sender<Peers>, mpsc::Receiver<Peers>) = mpsc::channel(100);

        let (param_tx, param_rx): (watch::Sender<Parameters>, watch::Receiver<Parameters>) =
            watch::channel(Parameters::from(torrent));

        debug!("trackers: {:?}", trackers);
        let trackers: Vec<HttpSession> = trackers
            .http
            .iter()
            .flat_map(|s| HttpSession::connect(s.clone(), param_rx.clone(), peer_tx.clone()).ok())
            .collect();

        debug!("sessions: {}", trackers.len());

        (Self { trackers }, peer_rx)
    }

    pub async fn run(self, torrent: TorrentInfo) -> Result<(), Report> {
        let mut set = JoinSet::new();

        for session in self.trackers {
            set.spawn(session.run(torrent.clone()));
        }

        while let Some(resp) = set.join_next().await {
            resp?;
        }

        debug!("sessions have been spawned");

        Ok(())
    }
}
