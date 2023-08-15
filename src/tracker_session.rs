use bendy::decoding::FromBencode;
use color_eyre::Report;

use futures_util::TryFutureExt;
use rand::Rng;
use std::{io::ErrorKind, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::UdpSocket,
    sync::mpsc,
    sync::{
        mpsc::{Receiver, Sender},
        watch,
    },
    time::{sleep, timeout},
};
use tracing::debug;
use url::Url;

use crate::{
    data::{Event, GeneralError, HttpResponse, Peers, TorrentInfo, PROTOCOL_ID},
    helpers,
    udp::{Request, Response},
    BITTORRENT_PORT, PEER_ID,
};

pub struct HttpSession {
    param_rx: watch::Receiver<Parameters>,
    peer_tx: Sender<Peers>,
    socket: reqwest::Client,
    dst: String,
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
        Ok(url)
    }

    async fn get(&self, parameters: &Parameters) -> Result<HttpResponse, Report> {
        let url = self.build_request(parameters).await?;
        let f = || self.socket.get(url.clone()).send().map_err(Report::from);

        let resp: reqwest::Response = helpers::attempt(f, 4, 1).await?;
        let bytes = resp.bytes().await?;

        match HttpResponse::from_bencode(&bytes) {
            Ok(resp) => Ok(resp),
            Err(_) => Err(GeneralError::UnexpectedResponse(url.to_string()).into()),
        }
    }

    pub async fn run(self, parameters: Arc<Parameters>) {
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

pub struct UdpSession {
    resp_rx: Receiver<Response>,
    peer_tx: Sender<Peers>,
    socket: Arc<UdpSocket>,
    dst: SocketAddr,
}

impl UdpSession {
    pub fn new(
        socket: Arc<UdpSocket>,
        dst: SocketAddr,
        resp_rx: Receiver<Response>,
        peer_tx: Sender<Peers>,
    ) -> Self {
        debug!(?dst);
        Self {
            resp_rx,
            peer_tx,
            socket,
            dst,
        }
    }

    async fn connect(&mut self) -> Result<Response, Report> {
        let tid = rand::thread_rng().gen();

        let packet = Request::Connect {
            cid: PROTOCOL_ID,
            action: 0_i32,
            tid,
        };

        self.dispatch(packet).await?;
        let resp = self
            .resp_rx
            .recv()
            .await
            .ok_or(GeneralError::Timeout(None))?;

        Ok(resp)
    }

    pub async fn dispatch(&mut self, packet: Request) -> Result<Response, Report> {
        let e = GeneralError::Timeout(Some(self.dst));

        for _ in 0..4 {
            // increase chance of success by randomly choosing another IP at every invocation
            match self.socket.send_to(&packet.to_request(), self.dst).await {
                Ok(_) => {
                    debug!("socket [{}] sent request: {:?}", self.dst, &packet);
                    let resp = self.resp_rx.recv().await.ok_or(e)?;
                    debug!("socket [{}] received response: {:?}", self.dst.ip(), resp);

                    return Ok(resp);
                }
                Err(e) => match e.kind() {
                    ErrorKind::WouldBlock => continue,
                    _ => return Err(e.into()),
                },
            }
        }

        Err(e.into())
    }

    pub async fn announce(
        &mut self,
        cid: i64,
        info_hash: [u8; 20],
        length: usize,
    ) -> Result<Response, Report> {
        let peer_id = rand::thread_rng().gen::<[u8; 20]>();
        let key = rand::thread_rng().gen::<u32>();

        let packet = Request::Announce {
            cid,
            action: 1i32,
            tid: rand::thread_rng().gen::<i32>(),
            info_hash,
            peer_id,
            up_down_left: (0, 0, length),
            event: Event::None,
            socket: self.socket.local_addr().unwrap(),
            key,
            num_want: -1i32,
            extensions: 0u16,
        };

        timeout(Duration::from_secs(3), self.dispatch(packet)).await?
    }

    pub async fn run(mut self, info_hash: [u8; 20], length: usize) -> Result<(), Report> {
        if let Response::Connect { cid, .. } = self.connect().await? {
            for _ in 0..4 {
                match self.announce(cid, info_hash, length).await? {
                    Response::Connect { .. } => {
                        continue;
                    }
                    Response::Announce { peers, .. } => {
                        self.peer_tx
                            .send(peers.iter().map(Into::into).collect())
                            .await?;
                    }
                    _ => {}
                }
            }
        }

        Err(GeneralError::Reconnect.into())
    }
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Parameters {
    pub info_hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub port: u16,
    pub up_down_left: (usize, usize, usize),
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

impl TryFrom<&TorrentInfo> for Parameters {
    type Error = GeneralError;

    fn try_from(info: &TorrentInfo) -> Result<Self, Self::Error> {
        Ok(Parameters {
            info_hash: info.hash,
            peer_id: *PEER_ID,
            port: *BITTORRENT_PORT,
            up_down_left: (0, 0, info.length()),
            compact: false,
            no_peer_id: false,
            event: Event::None,
            ip: None,
            numwant: 50,
            key: None,
            tracker_id: None,
            extensions: None,
        })
    }
}
