use std::{
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs},
    sync::Arc,
    time::Duration,
};

use async_convert::{async_trait, TryFrom};
use chrono::{DateTime, Utc};
use color_eyre::Report;
use either::Either;
use futures::{
    future::{self, BoxFuture},
    FutureExt,
};
use once_cell::sync::OnceCell;
use rand::Rng;
use reqwest::Url;
use tokio::{
    net::UdpSocket,
    sync::mpsc::Receiver,
    time::{sleep, timeout},
};
use tracing::debug;

use crate::{
    helpers::{decode, MagnetInfo},
    udp::{Request, Response, TransferEvent},
    GeneralError, PROTOCOL_ID,
};

#[derive(Clone, Debug)]
pub struct Router {
    pub state: State,
    pub magnet: MagnetInfo,
    pub socket: Arc<UdpSocket>,

    src: SocketAddr,
    dst: Option<SocketAddr>,
}

#[derive(Clone, Debug)]
pub enum State {
    Disconnected,
    Connected { trackers: Vec<SocketAddr>, cid: i64 },
    Announced { peers: Vec<SocketAddr>, cid: i64 },
    Scraped,
}

#[derive(Debug)]
pub enum Event {
    Connect,
    Announce,
}

impl Router {
    pub fn new(src: SocketAddr, magnet: MagnetInfo, socket: Arc<UdpSocket>) -> Self {
        Self {
            src,
            dst: None,
            magnet,
            socket,
            state: State::Disconnected,
        }
    }

    pub async fn dispatch(
        &self,
        packet: Request,
        timeout_n: u64,
        retries: usize,
        dst: SocketAddr,
    ) -> Result<Response, Report> {
        let mut data = [0; 1024 * 100];
        let mut size = 0;

        for _ in 0..retries {
            match self.socket.send_to(&packet.to_request(), dst).await {
                Ok(n) => {
                    debug!("sent {n} bytes: {:?}", &packet.to_request()[..n]);
                }
                Err(e) => match e.kind() {
                    ErrorKind::WouldBlock => continue,
                    _ => return Err(e.into()),
                },
            }

            match timeout(
                Duration::from_secs(timeout_n),
                self.socket.recv(&mut data[..]),
            )
            .await
            {
                Ok(r) => match r {
                    Ok(n) => {
                        debug!("found working tracker: {:?}", dst.clone());
                        size = n;
                        break;
                    }
                    Err(e) => return Err(e.into()),
                },
                Err(_) => {
                    debug!("did not receive value within {timeout_n} seconds");
                }
            };
        }

        debug!("received {size} bytes: {:?}", &data[..size]);

        Response::to_response(&data[..size])
    }

    pub async fn try_tracker(&self, s: String) -> Result<(Response, SocketAddr), Report> {
        let packet = Request::Connect {
            cid: PROTOCOL_ID,
            action: 0_i32,
            tid: rand::thread_rng().gen::<i32>(),
        };

        let url: Url = Url::parse(&s)?;
        debug!("[TRACKERS] trying {} as tracker ...", s);

        let (addr, port) = (url.host_str().unwrap(), url.port().unwrap());
        let dst = (addr, port)
            .to_socket_addrs()?
            .next()
            .ok_or::<Report>(GeneralError::InvalidTracker(s).into())?;

        self.dispatch(packet, 3, 4, dst)
            .await
            .map(|resp| (resp, dst))
    }

    pub fn next(self, event: Event) -> BoxFuture<'static, Result<Self, Report>> {
        debug!(?event, ?self);
        async move {
            match (self.clone().state, event) {
                (State::Disconnected, Event::Connect) => {
                    let iter = self
                        .magnet
                        .trackers
                        .iter()
                        .map(|s| Box::pin(self.try_tracker(s.to_owned())));

                    let ((resp, dst), _) = future::select_ok(iter).await?;

                    // let resp = match resp {
                    //     r if r.action == 0i32 && r.tid == packet.tid => {
                    //         debug!("transaction ID matches: {:?}", packet.tid.to_be_bytes());
                    //         debug!("connection ID is {:?}", r.cid.to_be_bytes());
                    //         r
                    //     }
                    //     _ => return Err(GeneralError::DeadTrackers.into()),
                    // };

                    match resp {
                        Response::Connect { cid, .. } => Ok(Self {
                            src: self.src,
                            dst: Some(dst),
                            magnet: self.magnet,
                            socket: self.socket,
                            state: State::Connected {
                                cid,
                                trackers: vec![dst],
                            },
                        }),
                        x => Err(GeneralError::UnexpectedResponse(x).into()),
                    }
                }
                // (State::Connected { trackers, cid }, Event::Connect) => {
                //     let ok = Self {
                //         src: self.src,
                //         dst: None,
                //         magnet: self.magnet,
                //         socket: self.socket,
                //         state: State::Connected { trackers, cid },
                //     };

                //     Ok(ok.next(Event::Announce).await?)
                // }
                (State::Connected { cid, .. }, Event::Announce) => {
                    let peer_id = rand::thread_rng().gen::<[u8; 20]>();
                    let key = rand::thread_rng().gen::<u32>();

                    let packet = Request::Announce {
                        cid,
                        action: 1i32,
                        tid: rand::thread_rng().gen::<i32>(),
                        hash: decode(self.magnet.hash.clone()),
                        peer_id,
                        downloaded: 0,
                        left: 0,
                        uploaded: 0,
                        event: TransferEvent::Inactive,
                        socket: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
                        key,
                        num_want: -1i32,
                        extensions: 0u16,
                    };

                    let resp = self
                        .dispatch(packet.clone(), 3, 4, self.dst.unwrap())
                        .await?;

                    match resp {
                        Response::Announce { peers, .. } => {
                            debug!(?peers);

                            Ok(Self {
                                state: State::Announced { peers, cid },
                                magnet: self.magnet,
                                dst: self.dst,
                                src: self.src,
                                socket: self.socket,
                            })
                        }
                        Response::Connect { cid, .. } => {
                            let new = Self {
                                state: State::Connected {
                                    trackers: vec![self.dst.unwrap()],
                                    cid,
                                },
                                magnet: self.magnet,
                                dst: self.dst,
                                src: self.src,
                                socket: self.socket,
                            };

                            new.next(Event::Announce).await
                        }
                        x => Err(GeneralError::UnexpectedResponse(x).into()),
                    }
                }
                (State::Announced { peers, cid }, Event::Connect) => todo!(),
                (State::Announced { peers, cid }, Event::Announce) => todo!(),
                (State::Scraped, Event::Connect) => todo!(),
                (State::Scraped, Event::Announce) => todo!(),
                _ => panic!(),
            }
        }
        .boxed()
    }
}

// #[async_trait]
// impl TryFrom<Router<Connected>> for Router<Announced> {
//     type Error = Report;

//     async fn try_from(old: Router<Connected>) -> Result<Router<Announced>, Self::Error> {
//         let peer_id = rand::thread_rng().gen::<[u8; 20]>();
//         let key = rand::thread_rng().gen::<u32>();

//         let packet = AnnounceReq::build(old.magnet.clone(), old.state.cid, peer_id, key);

//         let mut ok = old.announce(old.dst.unwrap(), packet.clone()).await?;

//         loop {
//             match ok[3] {
//                 0 => {
//                     let resp = ConnectResp::to_response(&ok)?;
//                     let packet = AnnounceReq::build(old.magnet.clone(), resp.cid, peer_id, key);
//                     debug!(?resp.cid);

//                     ok = old.announce(old.state.trackers[0], packet.clone()).await?;
//                 }
//                 3 => {
//                     let resp = TrackerError::to_response(&ok)?;
//                     debug!(resp.error);
//                     break;
//                 }
//                 _ => {
//                     break;
//                 }
//             }
//             sleep(Duration::from_millis(500)).await;
//         }

//         let resp = AnnounceResp::to_response(&ok)?;

//         Ok(Self {
//             state: Announced {
//                 peers: resp.peers,
//                 cid: old.state.cid,
//             },
//             magnet: old.magnet,
//             dst: old.dst,
//             src: old.src,
//             socket: old.socket,
//         })
//     }
// }

// #[async_trait]
// impl TryFrom<Router<Announced>> for Router<Scraped> {
//     type Error = Report;

//     async fn try_from(old: Router<Announced>) -> Result<Router<Scraped>, Self::Error> {
//         let packet = ScrapeReq::build(old.state.cid, old.magnet.clone());

//         let resp = old.scrape(old.dst.unwrap(), packet).await?;

//         debug!(?resp);

//         Ok(Self {
//             state: Scraped {},
//             magnet: old.magnet,
//             dst: old.dst,
//             src: old.src,
//             socket: old.socket,
//         })
//     }
// }

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

// impl<S> Router<S> {
//     pub async fn connect(
//         &self,
//         dst: SocketAddr,
//         packet: ConnectReq,
//     ) -> Result<(SocketAddr, ConnectResp), Report> {
//         let mut data = [0; 1024 * 100];
//         let mut size = 0;

//         for _ in 0..4 {
//             match self.socket.send_to(&packet.to_request(), dst).await {
//                 Ok(n) => {
//                     debug!(
//                         "[CONNECT] sent {n} bytes: ({:?}) {:?}",
//                         std::any::type_name::<ConnectReq>(),
//                         &packet.to_request()[..n]
//                     );
//                 }
//                 Err(e) => match e.kind() {
//                     ErrorKind::WouldBlock => continue,
//                     _ => return Err(e.into()),
//                 },
//             }

//             match timeout(Duration::from_secs(15), self.socket.recv(&mut data[..])).await {
//                 Err(_) => {
//                     debug!("[CONNECT] did not receive value within 15 seconds");
//                 }
//                 Ok(r) => match r {
//                     Ok(n) => {
//                         debug!("[CONNECT] found working tracker: {:?}", dst.clone());
//                         size = n;
//                         break;
//                     }
//                     Err(e) => return Err(e.into()),
//                 },
//             };
//         }

//         debug!(
//             "[CONNECT] received {size} bytes: ({:?}) {:?}",
//             std::any::type_name::<ConnectResp>(),
//             &data[..size]
//         );

//         ConnectResp::to_response(&data[..size]).map(|ok| (dst, ok))
//     }

//     pub async fn announce(&self, dst: SocketAddr, packet: AnnounceReq) -> Result<Vec<u8>, Report> {
//         let mut resp = [0; 1024 * 100];

//         match self.socket.send_to(&packet.to_request(), dst).await {
//             Ok(n) => {
//                 debug!(
//                     "[ANNOUNCE] sent {n} bytes: ({:?})",
//                     std::any::type_name::<AnnounceReq>(),
//                     // &packet.to_request(),
//                 );
//             }
//             Err(e) => {
//                 return Err(e.into());
//             }
//         }
//         let n = match timeout(Duration::from_secs(3), self.socket.recv(&mut resp[..])).await {
//             Err(_) => {
//                 debug!("[ANNOUNCE] did not receive value within 3 seconds");
//                 return Err(GeneralError::Timeout.into());
//             }
//             Ok(r) => match r {
//                 Ok(n) => n,
//                 Err(e) => return Err(e.into()),
//             },
//         };

//         debug!(
//             "[ANNOUNCE] received {n} bytes: ({:?}) {:?}",
//             std::any::type_name::<AnnounceResp>(),
//             &resp[..n]
//         );

//         Ok(resp[..n].to_vec())
//     }

//     pub async fn scrape(&self, dst: SocketAddr, packet: ScrapeReq) -> Result<ScrapeResp, Report> {
//         let mut resp = [0; 1024 * 100];

//         match self.socket.send_to(&packet.to_request(), dst).await {
//             Ok(n) => {
//                 debug!(
//                     "[SCRAPE] sent {n} bytes: ({:?})",
//                     std::any::type_name::<ScrapeReq>(),
//                     // &packet.to_request(),
//                 );
//             }
//             Err(e) => {
//                 return Err(e.into());
//             }
//         }

//         let n = match self.socket.recv(&mut resp[..]).await {
//             Ok(n) => n,
//             Err(e) => return Err(e.into()),
//         };

//         debug!(
//             "[SCRAPE] received {n} bytes: ({:?}) {:?}",
//             std::any::type_name::<ScrapeReq>(),
//             &resp[..n]
//         );

//         ScrapeResp::to_response(&resp[..n])
//     }
// }
//
