use std::{net::SocketAddr, sync::Arc, time::Duration};

use chrono::{DateTime, Utc};
use color_eyre::Report;
use tokio::{net::UdpSocket, time::timeout};

use crate::udp::{AnnounceReq, AnnounceResp, ConnectReq, ConnectResp, Request, Response};

pub type RoutingTable = Vec<Node>;

#[derive(Clone)]
pub struct DHT {
    pub socket: Arc<UdpSocket>,
    pub target: Option<SocketAddr>,
}

pub struct Node {
    pub id: [u8; 20],
    pub routing_table: RoutingTable,
}

pub struct Bucket {
    last_changed: DateTime<Utc>,
}

impl Node {
    fn compare(&self, hash: [u8; 20]) -> [u8; 20] {
        todo!()
    }
}

impl DHT {
    pub async fn announce(&self, packet: AnnounceReq<'_>) -> Result<AnnounceResp, Report> {
        let mut i = 0;
        let mut resp = [0; 1024 * 100];
        self.socket.connect(self.target.unwrap()).await?;

        match self.socket.send(&packet.to_request()).await {
            Ok(n) => {
                println!("sent {n} bytes");
            }
            Err(e) => {
                return Err(e.into());
            }
        }
        match timeout(Duration::from_secs(3), self.socket.recv(&mut resp[..])).await {
            Err(_) => {
                println!("did not receive value within 3 seconds");
            }
            Ok(r) => match r {
                Ok(n) => {
                    i = n;
                }
                Err(e) => return Err(e.into()),
            },
        };

        AnnounceResp::to_response(&resp[..i])
    }

    pub async fn connect(
        &self,
        src: SocketAddr,
        dst: SocketAddr,
        packet: ConnectReq,
    ) -> Result<ConnectResp, Report> {
        let mut data = [0; 1024 * 100];
        let mut i = 0;

        for _ in 0..4 {
            match self.socket.send_to(&packet.to_request(), dst).await {
                Ok(n) => {
                    println!("sent {n} bytes");
                }
                Err(e) => {
                    return Err(e.into());
                }
            }

            match timeout(Duration::from_secs(15), self.socket.recv(&mut data[..])).await {
                Err(_) => {
                    println!("did not receive value within 15 seconds");
                }
                Ok(r) => match r {
                    Ok(n) => {
                        i = n;
                        break;
                    }
                    Err(e) => return Err(e.into()),
                },
            };
        }

        ConnectResp::to_response(data[..i].try_into()?)
    }
}
