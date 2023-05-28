use std::{collections::HashMap, io::ErrorKind, net::SocketAddr, sync::Arc, time::Duration};

use color_eyre::Report;
use dashmap::DashMap;
use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use rand::Rng;
use tokio::{
    net::UdpSocket,
    sync::mpsc::{self, Receiver, Sender},
    task::JoinSet,
    time::{sleep, timeout},
};
use tracing::debug;

use crate::{
    data::{GeneralError, PROTOCOL_ID},
    helpers::MagnetInfo,
    udp::{Request, Response, TransferEvent},
};

pub type Message = (SocketAddr, Response);

#[derive(Clone)]
pub enum State {
    Disconnected,
    Connected { cid: i64 },
    Announced { peers: Vec<SocketAddr>, cid: i64 },
    Scraped,
}

pub struct Tracker {
    pub trackers: DashMap<SocketAddr, Session>,
    pub socket: Arc<UdpSocket>,

    pub hash: [u8; 20],
}

impl Tracker {
    pub fn new(
        magnet: &MagnetInfo,
        socket: Arc<UdpSocket>,
        mut rx: HashMap<SocketAddr, Receiver<Response>>,
    ) -> Result<Self, Report> {
        let trackers = magnet
            .trackers
            .iter()
            .map(|&s| {
                let rx = rx.remove(&s).unwrap();
                (s, Session::new(magnet.hash, s, socket.clone(), rx))
            })
            .collect();

        Ok(Self {
            trackers,
            hash: magnet.hash,
            socket,
        })
    }

    pub async fn run(self) -> Result<Vec<SocketAddr>, Report> {
        let mut set = JoinSet::new();
        let len = self.trackers.len();
        let mut working = Vec::with_capacity(len);

        self.trackers.into_iter().for_each(|(ip, session)| {
            debug!("resolver spawned for [{}]", ip);
            set.spawn(session.resolve());
        });

        while let Some(f) = set.join_next().await {
            working.push(f?);
        }

        Ok(working.into_iter().flatten().flatten().collect())
    }

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
}

pub struct Session {
    pub state: State,
    pub rx: Receiver<Response>,
    pub socket: Arc<UdpSocket>,
    pub dst: SocketAddr,
    pub hash: [u8; 20],

    pub retries: usize,
    pub timeout: usize,
}

impl Session {
    pub fn new(
        hash: [u8; 20],
        dst: SocketAddr,
        socket: Arc<UdpSocket>,
        rx: Receiver<Response>,
    ) -> Self {
        Self {
            state: State::Disconnected,
            rx,
            socket,
            dst,
            hash,
            retries: 3,
            timeout: 5,
        }
    }

    pub async fn dispatch(&mut self, packet: Request) -> Result<Response, Report> {
        for _ in 0..self.retries {
            match self.socket.send_to(&packet.to_request(), self.dst).await {
                Ok(_) => {
                    debug!("socket [{}] sent request: {:?}", self.dst.ip(), &packet);

                    let resp = self.rx.recv().await.ok_or(GeneralError::Timeout)?;

                    debug!("socket [{}] received response: {:?}", self.dst.ip(), resp,);

                    return Ok(resp);
                }
                Err(e) => match e.kind() {
                    ErrorKind::WouldBlock => continue,
                    _ => return Err(e.into()),
                },
            }
        }

        Err(GeneralError::InvalidTracker(self.dst.to_string()).into())
    }

    // TODO: make this recursive
    pub async fn connect(&mut self) -> Result<Response, Report> {
        let tid = rand::thread_rng().gen::<i32>();
        let packet = Request::Connect {
            cid: PROTOCOL_ID,
            action: 0_i32,
            tid,
        };

        timeout(Duration::from_secs(3), self.dispatch(packet)).await?
    }

    pub async fn announce(&mut self, cid: i64) -> Result<Response, Report> {
        let peer_id = rand::thread_rng().gen::<[u8; 20]>();
        let key = rand::thread_rng().gen::<u32>();

        let packet = Request::Announce {
            cid,
            action: 1i32,
            tid: rand::thread_rng().gen::<i32>(),
            hash: self.hash,
            peer_id,
            downloaded: 0,
            left: 0,
            uploaded: 0,
            event: TransferEvent::Inactive,
            socket: self.socket.local_addr().unwrap(),
            key,
            num_want: -1i32,
            extensions: 0u16,
        };

        timeout(Duration::from_secs(3), self.dispatch(packet)).await?
    }
    pub async fn resolve(mut self) -> Option<Vec<SocketAddr>> {
        if let Response::Connect { cid, .. } = self.connect().await.ok()? {
            for _ in 0..self.retries {
                match self.announce(cid).await.ok()? {
                    Response::Connect { action, tid, cid } => {
                        // sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    Response::Announce { peers, .. } => return Some(peers),
                    _ => return None,
                }
            }
            None
        } else {
            None
        }
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    Connect,
    Announce,
}
