use std::{io::ErrorKind, net::SocketAddr, sync::Arc};

use color_eyre::Report;
use dashmap::DashMap;
use futures::{future::BoxFuture, stream::FuturesUnordered, FutureExt, StreamExt};
use rand::Rng;
use tokio::{
    net::UdpSocket,
    sync::watch::{channel, Receiver, Sender},
    task::JoinSet,
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
    trackers: DashMap<SocketAddr, Session>,
    socket: Arc<UdpSocket>,
    tx: Sender<Option<Message>>,

    hash: [u8; 20],
}

impl Tracker {
    pub fn new(magnet: MagnetInfo, src: SocketAddr) -> Result<Self, Report> {
        let socket = std::net::UdpSocket::bind(src)?;
        let socket = Arc::new(UdpSocket::from_std(socket)?);

        let (tx, rx) = channel(None::<Message>);

        let trackers = magnet
            .trackers
            .iter()
            .map(|&s| {
                (
                    s,
                    Session::new(magnet.hash.clone(), s, socket.clone(), rx.clone()),
                )
            })
            .collect();

        Ok(Self {
            trackers,
            tx,
            hash: magnet.hash,
            socket,
        })
    }

    pub async fn run(&mut self) -> Result<Vec<Option<Session>>, Report> {
        let f = self
            .trackers
            .iter_mut()
            .map(|mut session| async move { session.next().await.ok() })
            .collect::<FuturesUnordered<_>>();

        // let g =

        Ok(f.collect().await)
    }

    pub async fn listen(&self) {
        loop {
            let mut buf = [0u8; 1024];
            match self.socket.recv_from(&mut buf).await {
                Ok((n, peer)) => {
                    let resp = Response::to_response(&buf[..n]).unwrap();
                    self.tx.send(Some((peer, resp))).unwrap();
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
                Err(e) => panic!("{}", e),
            }
        }
    }
}

pub struct Session {
    pub state: State,
    pub rx: Receiver<Option<Message>>,
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
        rx: Receiver<Option<Message>>,
    ) -> Self {
        Self {
            rx,
            socket,
            state: State::Disconnected,
            dst,
            hash,
            retries: 3,
            timeout: 5,
        }
    }

    pub async fn dispatch(&mut self, packet: Request) -> Result<Response, Report> {
        for _ in 0..self.retries {
            match self.socket.send_to(&packet.to_request(), self.dst).await {
                Ok(n) => {
                    debug!("socket sent {n} bytes: {:?}", &packet.to_request()[..n]);

                    let resp = loop {
                        let tmp = &*self.rx.borrow();

                        match tmp {
                            Some((s, resp)) if s == &self.dst => {
                                break resp.clone();
                            }
                            _ => {
                                continue;
                            }
                        }
                    };

                    self.rx.borrow_and_update();
                    debug!("socket received response: {:?}", resp);

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

    pub fn next(mut self) -> BoxFuture<'static, Result<Self, Report>> {
        async move {
            match self.state {
                State::Disconnected => {
                    let tid = rand::thread_rng().gen::<i32>();
                    let packet = Request::Connect {
                        cid: PROTOCOL_ID,
                        action: 0_i32,
                        tid,
                    };

                    let resp = self.dispatch(packet).await?;
                    if let Response::Connect { cid, .. } = resp {
                        self.state = State::Connected { cid };
                        self.next().await
                    } else {
                        Err(GeneralError::Timeout.into())
                    }
                }
                State::Connected { cid } => {
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
                        socket: self.socket.local_addr()?,
                        key,
                        num_want: -1i32,
                        extensions: 0u16,
                    };

                    let resp = self.dispatch(packet).await?;

                    match resp {
                        Response::Connect { cid, .. } => {
                            self.state = State::Connected { cid };
                            self.next().await
                        }
                        Response::Announce { peers, .. } => {
                            self.state = State::Announced { peers, cid };
                            self.next().await
                        }
                        _ => {
                            panic!()
                        }
                    }
                }
                State::Announced { .. } => return Ok(self),
                State::Scraped => todo!(),
            }
        }
        .boxed()
    }
}

#[derive(Clone, Debug)]
pub enum Event {
    Connect,
    Announce,
}
