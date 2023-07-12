use bendy::decoding::{Decoder as BendyDecoder, FromBencode};
use color_eyre::Report;
use dashmap::DashMap;
use rand::Rng;
use std::error::Error as StdError;
use std::net::Ipv4Addr;
use std::{collections::HashMap, io::ErrorKind, net::SocketAddr, sync::Arc, time::Duration};
use tokio::io::AsyncWrite;
use tokio::sync::mpsc::{Receiver, Sender};
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
use crate::tracker_session::{HttpSession, Parameters, UdpSession};
use crate::{
    data::{GeneralError, HttpResponse, ScrapeResponse, TorrentInfo, PROTOCOL_ID},
    helpers::{self, decode, encode},
    magnet::MagnetInfo,
    udp::{Request, Response},
};

type ChannelMap = (
    HashMap<SocketAddr, mpsc::Sender<Response>>,
    HashMap<SocketAddr, mpsc::Receiver<Response>>,
);

pub struct UdpTracker {
    pub socket: Arc<UdpSocket>,
    pub session_map: HashMap<SocketAddr, UdpSession>,
}

impl UdpTracker {
    pub async fn new(torrent: &TorrentInfo) -> Result<(Self, Receiver<Peers>), Report> {
        let trackers = torrent.announce.udp.clone();
        let (peer_tx, peer_rx) = channel(100);

        let src = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 1317u16));
        let socket = Arc::new(UdpSocket::bind(src).await?);

        let map: Vec<_> = trackers
            .into_iter()
            .map(|addr| {
                let (tx, rx) = channel::<Response>(5);
                ((addr, tx), (addr, rx))
            })
            .collect();
        let (tx_map, rx_map): ChannelMap = map.into_iter().unzip();

        let session_map = rx_map
            .into_iter()
            .map(|(addr, resp_rx)| {
                debug!("adding UDP tracker session for [{addr}]");
                (
                    addr,
                    UdpSession::new(socket.clone(), addr, resp_rx, peer_tx.clone()),
                )
            })
            .collect();

        tokio::spawn(UdpTracker::listen(socket.clone(), tx_map));

        Ok((
            Self {
                session_map,
                socket,
            },
            peer_rx,
        ))
    }

    pub async fn run(self, torrent: TorrentInfo) -> Result<(), Report> {
        let mut set = JoinSet::new();

        for (_, session) in self.session_map.into_iter() {
            let torrent = torrent.clone();
            set.spawn(async move { session.run(&torrent).await });
        }

        tokio::spawn(async move {
            while set.join_next().await.is_some() {
                debug!("joined a session!");
            }
        });

        Ok(())
    }
    pub async fn listen(socket: Arc<UdpSocket>, tx: HashMap<SocketAddr, Sender<Response>>) {
        loop {
            let mut buf = [0u8; 1024];

            for _ in 0..4 {
                match timeout(Duration::from_secs(3), socket.clone().recv_from(&mut buf)).await {
                    Ok(res) => match res {
                        Ok((n, peer)) => {
                            let resp = Response::to_response(&buf[..n]).unwrap();

                            let tx = tx.get(&peer).unwrap();
                            tx.send(resp).await.unwrap();
                        }
                        Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
                        Err(e) => panic!("{}", e),
                    },
                    Err(_) => {
                        debug!("trying another time ...");
                        continue;
                    }
                }
            }
        }
    }
}

pub type Message = (SocketAddr, Response);

#[derive(Clone)]
pub enum State {
    Disconnected,
    Connected { cid: i64 },
    Announced { peers: Peers, cid: i64 },
    Scraped,
}

pub struct HttpTracker {
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
