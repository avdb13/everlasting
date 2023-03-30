use std::{io::ErrorKind, net::SocketAddr, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use color_eyre::Report;
use either::Either;
use once_cell::sync::OnceCell;
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

pub type RoutingTable = Vec<Node>;

#[derive(Clone, Debug)]
pub struct Router {
    pub target: OnceCell<SocketAddr>,
    pub tid: OnceCell<i32>,
    pub cid: OnceCell<i64>,

    pub socket: Arc<UdpSocket>,
    pub connected: bool,
    pub queue: Vec<MagnetInfo>,
}

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

impl<'a> Router {
    pub async fn announce(&self, packet: AnnounceReq<'_>) -> Result<Vec<u8>, Report> {
        let mut resp = [0; 1024 * 100];
        self.socket.connect(self.target.get().unwrap()).await?;

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
        _src: SocketAddr,
        dst: SocketAddr,
        packet: ConnectReq,
    ) -> Result<(ConnectResp, SocketAddr), Report> {
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

        ConnectResp::to_response(&data[..size]).map(|c| (c, dst))
    }
}
