use std::{io::ErrorKind, net::SocketAddr, time::Duration};

use chrono::{DateTime, Utc};
use color_eyre::Report;
use rand::Rng;
use tokio::{
    io,
    net::UdpSocket,
    time::{sleep, Instant},
};

pub type RoutingTable = Vec<Node>;

#[derive(Default)]
pub struct DHT {
    // pub node: Node,
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
    pub async fn connect(&self, src: SocketAddr, dst: SocketAddr) -> Result<(), Report> {
        let (cid, action, tid) = (
            0x41727101980_i64.to_be_bytes(),
            0_i32.to_be_bytes(),
            rand::thread_rng().gen::<i32>().to_be_bytes(),
        );
        let packet = [cid.to_vec(), [action, tid].concat()].concat();
        dbg!(&packet);
        let socket = UdpSocket::bind(src).await?;

        dbg!(&dst);
        socket.connect(dst).await?;

        match socket.try_send(&packet) {
            Ok(n) => {
                println!("sent {n} bytes");
            }
            // False-positive, continue
            Err(ref e) if e.kind() == tokio::io::ErrorKind::WouldBlock => {}
            Err(e) => {
                return Err(e.into());
            }
        }

        let mut data = [0; 1024 * 100];
        match socket.try_recv(&mut data[..]) {
            Ok(n) => {
                println!("received {n} bytes");
            }
            // False-positive, continue
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                println!("sleeping ...");
                sleep(Duration::from_secs(15)).await;
            }
            Err(e) => {
                return Err(e.into());
            }
        }

        Ok(())
    }
}
