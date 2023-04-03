use std::{
    io::ErrorKind,
    net::{SocketAddr, ToSocketAddrs},
    sync::Arc,
    time::Duration,
};

use async_convert::{async_trait, TryFrom};
use chrono::{DateTime, Utc};
use color_eyre::Report;
use either::Either;
use futures::future;
use once_cell::sync::OnceCell;
use rand::Rng;
use reqwest::Url;
use tokio::{net::UdpSocket, sync::mpsc::Receiver, time::timeout};
use tracing::debug;

use crate::{
    helpers::MagnetInfo,
    udp::{
        AnnounceReq, AnnounceResp, ConnectReq, ConnectResp, Request, RespType, Response, ScrapeReq,
        TrackerError,
    },
    GeneralError,
};

#[derive(Clone, Debug)]
pub struct Router<S> {
    pub state: S,
    pub magnet: MagnetInfo,
    src: SocketAddr,
    pub socket: Arc<UdpSocket>,
    // pub tid: OnceCell<i32>,

    // pub connected: bool,
    // pub queue: Vec<MagnetInfo>,
}

#[derive(Clone)]
pub struct Disconnected;
pub struct Connected {
    trackers: Vec<SocketAddr>,
}
pub struct Announcing;
pub struct Scraping;

impl Router<Disconnected> {
    pub fn new(src: SocketAddr, magnet: MagnetInfo, socket: Arc<UdpSocket>) -> Self {
        Self {
            src,
            magnet,
            socket,
            state: Disconnected,
        }
    }
}

#[async_trait]
impl TryFrom<Router<Disconnected>> for Router<Connected> {
    type Error = Report;

    async fn try_from(old: Router<Disconnected>) -> Result<Self, Self::Error> {
        let packet = ConnectReq::build();

        let iter = old.magnet.trackers.iter().map(|s| {
            let packet = packet.clone();
            let router = old.clone();

            Box::pin(async move {
                match try {
                    let url: Url = Url::parse(s).ok()?;
                    debug!("[TRACKERS] trying {} as tracker ...", s);

                    let e = GeneralError::DeadTrackers;
                    let (addr, port) = (url.host_str().unwrap(), url.port().unwrap());
                    let dst = (addr, port).to_socket_addrs().ok()?.next()?;

                    router.connect(dst, packet).await.ok()?
                } {
                    Some(v) => Ok(v),
                    None => Err(GeneralError::DeadTrackers),
                }
            })
        });

        let ((dst, resp), _) = future::select_ok(iter).await?;

        let resp = match resp {
            r if r.action == 0i32 && r.tid == packet.tid => {
                debug!("transaction ID matches: ({}, {})", packet.tid, r.tid);
                debug!("connection ID is {}", r.cid);
                r
            }
            _ => return Err(GeneralError::DeadTrackers.into()),
        };

        Ok(Self {
            src: old.src,
            magnet: old.magnet,
            socket: old.socket,
            state: Connected {
                trackers: vec![dst],
            },
        })
    }
}

#[async_trait]
impl TryFrom<Router<Connected>> for Router<Announcing> {
    type Error = Report;

    async fn try_from(old: Router<Connected>) -> Result<Router<Announcing>, Self::Error> {
        let peer_id = rand::thread_rng().gen::<[u8; 20]>();
        let key = rand::thread_rng().gen::<u32>();

        let packet = AnnounceReq::build(old.magnet.clone(), peer_id, key);
        old.announce(old.state.trackers[0], packet).await?;

        Ok(Self {
            state: Announcing,
            magnet: old.magnet,
            src: old.src,
            socket: old.socket,
        })
    }
}

pub type RoutingTable = Vec<Node>;

pub struct Node {
    pub id: [u8; 20],
    pub routing_table: RoutingTable,
}

pub struct Bucket {
    last_changed: DateTime<Utc>,
}

impl Node {
    fn compare(&self, _hash: [u8; 20]) -> [u8; 20] {
        todo!()
    }
}

impl<S> Router<S> {
    pub async fn announce(&self, dst: SocketAddr, packet: AnnounceReq) -> Result<Vec<u8>, Report> {
        let mut resp = [0; 1024 * 100];
        self.socket.connect(dst).await?;

        match self.socket.send(&packet.to_request()).await {
            Ok(n) => {
                debug!(
                    "[ANNOUNCE] sent {n} bytes: ({:?}) {:?}",
                    std::any::type_name::<AnnounceReq>(),
                    &packet.to_request(),
                );
            }
            Err(e) => {
                return Err(e.into());
            }
        }
        let n = match timeout(Duration::from_secs(3), self.socket.recv(&mut resp[..])).await {
            Err(_) => {
                debug!("[ANNOUNCE] did not receive value within 3 seconds");
                return Err(GeneralError::Timeout.into());
            }
            Ok(r) => match r {
                Ok(n) => n,
                Err(e) => return Err(e.into()),
            },
        };

        debug!(
            "[ANNOUNCE] received {n} bytes: ({:?}) {:?}",
            std::any::type_name::<AnnounceResp>(),
            &resp[..n]
        );

        Ok(resp[..n].to_vec())
    }

    pub async fn connect(
        &self,
        dst: SocketAddr,
        packet: ConnectReq,
    ) -> Result<(SocketAddr, ConnectResp), Report> {
        let mut data = [0; 1024 * 100];
        let mut size = 0;

        for _ in 0..4 {
            match self.socket.send_to(&packet.to_request(), dst).await {
                Ok(n) => {
                    debug!(
                        "[CONNECT] sent {n} bytes: ({:?}) {:?}",
                        std::any::type_name::<ConnectReq>(),
                        &packet.to_request()[..n]
                    );
                }
                Err(e) => match e.kind() {
                    ErrorKind::WouldBlock => continue,
                    _ => return Err(e.into()),
                },
            }

            match timeout(Duration::from_secs(15), self.socket.recv(&mut data[..])).await {
                Err(_) => {
                    debug!("[CONNECT] did not receive value within 15 seconds");
                }
                Ok(r) => match r {
                    Ok(n) => {
                        debug!("[CONNECT] found working tracker: {:?}", dst.clone());
                        size = n;
                        break;
                    }
                    Err(e) => return Err(e.into()),
                },
            };
        }

        ConnectResp::to_response(&data[..size]).map(|ok| (dst, ok))
    }
}
