use color_eyre::Report;

use std::net::Ipv4Addr;
use std::{collections::HashMap, io::ErrorKind, net::SocketAddr, sync::Arc, time::Duration};

use std::net::UdpSocket as StdSocket;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{mpsc, watch};
use tokio::{net::UdpSocket, sync::mpsc::channel, task::JoinSet, time::timeout};
use tracing::debug;

use crate::data::{Peers, TorrentInfo};
use crate::tracker_session::{HttpSession, Parameters, UdpSession};
use crate::udp::Response;

type ChannelMap = (
    HashMap<SocketAddr, mpsc::Sender<Response>>,
    HashMap<SocketAddr, mpsc::Receiver<Response>>,
);

pub struct UdpTracker {
    pub socket: Arc<UdpSocket>,
    pub session_map: HashMap<SocketAddr, UdpSession>,
    pub hash: [u8; 20],
    pub length: usize,
}

impl UdpTracker {
    pub fn new(info: &TorrentInfo) -> Result<(Self, Receiver<Peers>), Report> {
        let length = info.length();
        let trackers = info.announce.udp.clone();
        let (peer_tx, peer_rx) = channel(100);

        let addr = SocketAddr::from((Ipv4Addr::UNSPECIFIED, 1317u16));
        let socket = Arc::new(UdpSocket::from_std(StdSocket::bind(addr)?)?);

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
                hash: info.hash,
                length: info.length(),
            },
            peer_rx,
        ))
    }

    pub async fn run(self) -> Result<(), Report> {
        let mut set = JoinSet::new();

        for (_, session) in self.session_map.into_iter() {
            set.spawn(async move { session.run(self.hash, self.length).await });
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
    pub parameters: Arc<Parameters>,
    pub trackers: Vec<HttpSession>,
}

impl Parameters {
    fn tracker_id(&mut self, tracker_id: String) {
        self.tracker_id = Some(tracker_id);
    }
}

// pub type WatchMap = HashMap<String, (watch::Sender<Parameters>, watch::Receiver<Parameters>)>;

impl HttpTracker {
    pub fn new(info: &TorrentInfo) -> Result<(Self, mpsc::Receiver<Peers>), Report> {
        let trackers = info.announce.http.clone();
        let parameters = Parameters::try_from(info)?;

        let (peer_tx, peer_rx): (mpsc::Sender<Peers>, mpsc::Receiver<Peers>) = mpsc::channel(100);

        let (_param_tx, param_rx): (watch::Sender<Parameters>, watch::Receiver<Parameters>) =
            watch::channel(parameters.clone());

        debug!("trackers: {:?}", trackers);
        let trackers: Vec<HttpSession> = info
            .announce
            .http
            .iter()
            .flat_map(|s| HttpSession::connect(s.clone(), param_rx.clone(), peer_tx.clone()).ok())
            .collect();

        Ok((
            Self {
                trackers,
                parameters: Arc::new(parameters),
            },
            peer_rx,
        ))
    }

    pub async fn run(self) -> Result<(), Report> {
        let mut set = JoinSet::new();

        for session in self.trackers {
            set.spawn(session.run(self.parameters.clone()));
        }

        while let Some(resp) = set.join_next().await {
            resp?;
        }

        debug!("sessions have been spawned");

        Ok(())
    }
}
