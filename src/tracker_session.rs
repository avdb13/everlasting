use async_trait::async_trait;
use bendy::decoding::FromBencode;
use color_eyre::Report;
use dashmap::DashMap;
use rand::Rng;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::UdpSocket,
    sync::mpsc,
    sync::watch,
    time::{sleep, timeout},
};
use tracing::debug;
use url::Url;

use crate::{
    data::{Event, GeneralError, HttpResponse, Peer, Peers, TorrentInfo, PROTOCOL_ID},
    helpers,
    udp::{Request, Response},
    BITTORRENT_PORT, PEER_ID,
};

pub struct HttpSession {
    param_rx: watch::Receiver<Parameters>,
    peer_tx: mpsc::Sender<Peers>,
    socket: reqwest::Client,
    dst: String,
}

pub struct UdpSession {
    socket: UdpSocket,
    dst: SocketAddr,
}

pub struct Parameters {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub port: u16,
    pub up_down_left: (u64, u64, u64),
    // some trackers only support compact responses
    pub compact: bool,
    pub no_peer_id: bool,
    pub event: Event,
    pub ip: Option<SocketAddr>,
    pub numwant: u8,
    pub key: Option<()>,
    pub tracker_id: Option<String>,
    pub extensions: Option<()>,
}

impl From<&TorrentInfo> for Parameters {
    fn from(torrent: &TorrentInfo) -> Self {
        Parameters {
            info_hash: torrent.info.value,
            peer_id: *PEER_ID,
            port: *BITTORRENT_PORT,
            up_down_left: (0, 0, torrent.length()),
            compact: false,
            no_peer_id: false,
            event: Event::None,
            ip: None,
            numwant: 50,
            key: None,
            tracker_id: None,
            extensions: None,
        }
    }
}

impl HttpSession {
    pub fn connect(
        dst: String,
        param_rx: watch::Receiver<Parameters>,
        peer_tx: mpsc::Sender<Peers>,
    ) -> Result<Self, Report> {
        let socket = reqwest::ClientBuilder::new()
            .connect_timeout(Duration::from_secs(5))
            .build()?;

        Ok(Self {
            socket,
            dst,
            param_rx,
            peer_tx,
        })
    }

    async fn build_request(&self, p: &Parameters) -> Result<Url, Report> {
        let info_hash = helpers::encode(&p.info_hash);
        let peer_id = helpers::encode(&p.peer_id);

        let event = match p.event {
            Event::Started => Some("started"),
            Event::Stopped => Some("stopped"),
            Event::Completed => Some("completed"),
            Event::None => None,
        };

        let mut queries = format!(
                "info_hash={}&peer_id={}&port={}&uploaded={}&downloaded={}&left={}&compact={}&no_peer_id={}&numwant={}",
                info_hash, peer_id, p.port,
                p.up_down_left.0, p.up_down_left.1, p.up_down_left.2,
                p.compact as usize, p.no_peer_id as usize, p.numwant
            );

        if let Some(event) = event {
            queries += &format!("&event={event}");
        }

        let mut url = Url::parse(&self.dst)?;
        url.set_query(Some(&queries));
        debug!("{:?}", url.to_string());
        Ok(url)
    }

    async fn get(&self, parameters: &Parameters) -> Result<HttpResponse, Report> {
        let url = self.build_request(parameters).await?;

        let resp = self.socket.get(url).send().await?;
        let bytes = resp.bytes().await?;

        let bencode = HttpResponse::from_bencode(&bytes).unwrap();
        Ok(bencode)
    }

    pub async fn run(self, torrent: TorrentInfo) {
        let parameters = Parameters::from(&torrent);

        let resp = self.get(&parameters).await;

        if let Ok(resp) = resp {
            self.peer_tx.send(resp.peers).await.unwrap();
            let interval = resp.min_interval.unwrap_or(resp.interval);

            loop {
                sleep(Duration::from_secs(interval)).await;

                // if self.param_rx.changed().await.is_ok() {
                //     let parameters = self.param_rx.borrow();

                //     let resp = self.get(&parameters).await.unwrap();
                //     self.peer_tx.send(resp.peers).await.unwrap();
                // }
            }
        }
    }
}

// impl UdpSession {
//     fn new(socket: UdpSocket, dst: SocketAddr) -> Self {
//         Self { socket, dst }
//     }

//     async fn try_connect(&self) -> Result<(), Report> {
//         let tid = rand::thread_rng().gen::<i32>();
//         let packet = Request::Connect {
//             cid: PROTOCOL_ID,
//             action: 0_i32,
//             tid,
//         };

//         timeout(Duration::from_secs(15), self.dispatch(packet)).await?
//     }
// }

// pub async fn dispatch(&mut self, packet: Request) -> Result<Response, Report> {
//     // If no response to a request is received within 15 seconds, resend the request.
//     // If no reply has been received after 60 seconds, stop retrying.

//     for _ in 0..4 {
//         match self.socket.send_to(packet.to_request()).await {
//             Ok(_) => {
//                 debug!("socket [{}] sent request: {:?}", self.1, &packet);

//                 let resp = self.rx.recv().await.ok_or(GeneralError::Timeout)?;

//                 debug!("socket [{}] received response: {:?}", self.dst.ip(), resp,);

//                 return Ok(resp);
//             }
//             Err(e) => match e.kind() {
//                 ErrorKind::WouldBlock => continue,
//                 _ => return Err(e.into()),
//             },
//         }
//     }

//     Ok(())
// }
// }

// #[async_trait]
// pub trait Socket {
// async fn try_connect(&self) -> Result<(), Report>;
// async fn send(&self, message: Vec<u8>) -> Result<usize, Report>;
// }

// #[async_trait]
// impl Socket for UdpSocket {
// async fn send(&self, message: Vec<u8>) -> Result<usize, Report> {
//     self.0
//         .clone()
//         .send_to(message.as_slice(), self.1)
//         .await
//         .map_err(|e| e.into())
// }
// }

// pub struct UdpTracker {
//     pub socket: Box<dyn Socket>,
//     pub session_map: DashMap<SocketAddr, Session>,
//     pub hash: [u8; 20],
// }

// pub struct UdpSession {
//     pub state: State,
//     pub rx: Receiver<Response>,
//     pub socket: Arc<UdpSocket>,
//     pub dst: SocketAddr,
//     pub hash: [u8; 20],

//     pub retries: usize,
//     pub timeout: usize,
// }

// impl UdpSession {
//     pub fn new(
//         hash: [u8; 20],
//         dst: SocketAddr,
//         socket: Arc<UdpSocket>,
//         rx: Receiver<Response>,
//     ) -> Self {
//         Self {
//             state: State::Disconnected,
//             rx,
//             socket,
//             dst,
//             hash,
//             retries: 3,
//             timeout: 5,
//         }
//     }

//         Err(GeneralError::InvalidUdpTracker(self.dst.to_string()).into())
//     }

//     // TODO: make this recursive
//     pub async fn connect(&mut self) -> Result<Response, Report> {
//         let tid = rand::thread_rng().gen::<i32>();
//         let packet = Request::Connect {
//             cid: PROTOCOL_ID,
//             action: 0_i32,
//             tid,
//         };

//         timeout(Duration::from_secs(3), self.dispatch(packet)).await?
//     }

//     pub async fn announce(&mut self, cid: i64) -> Result<Response, Report> {
//         let peer_id = rand::thread_rng().gen::<[u8; 20]>();
//         let key = rand::thread_rng().gen::<u32>();

//         let packet = Request::Announce {
//             cid,
//             action: 1i32,
//             tid: rand::thread_rng().gen::<i32>(),
//             hash: self.hash,
//             peer_id,
//             downloaded: 0,
//             left: 0,
//             uploaded: 0,
//             event: TransferEvent::Inactive,
//             socket: self.socket.local_addr().unwrap(),
//             key,
//             num_want: -1i32,
//             extensions: 0u16,
//         };

//         timeout(Duration::from_secs(3), self.dispatch(packet)).await?
//     }
//     pub async fn resolve(mut self) -> Option<Vec<SocketAddr>> {
//         if let Response::Connect { cid, .. } = self.connect().await.ok()? {
//             for _ in 0..self.retries {
//                 match self.announce(cid).await.ok()? {
//                     Response::Connect { action, tid, cid } => {
//                         // sleep(Duration::from_secs(1)).await;
//                         continue;
//                     }
//                     Response::Announce { peers, .. } => return Some(peers),
//                     _ => return None,
//                 }
//             }
//             None
//         } else {
//             None
//         }
//     }
