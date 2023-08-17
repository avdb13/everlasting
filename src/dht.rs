use std::{collections::HashMap, io::ErrorKind, net::SocketAddr, ops::Range, sync::Arc};

use ahash::HashSet;
use bendy::{decoding::FromBencode, encoding::ToBencode};
use chrono::Utc;
use color_eyre::Report;
use lazy_static::lazy_static;
use rand::Rng;
use tokio::{net::UdpSocket, sync::Mutex, task::JoinSet};
use tracing::debug;

use crate::{
    data::GeneralError,
    krpc::{self, Arguments, ExtMessage, Method},
};

const CAPACITY: usize = 8;

#[derive(Default, Clone, Copy, Debug)]
pub struct Node {
    pub id: [u8; 20],
    pub addr: Option<SocketAddr>,
}

impl Node {
    fn new(id: [u8; 20], addr: SocketAddr) -> Self {
        Node {
            id,
            addr: Some(addr),
        }
    }
    fn distance(&self, other: &Node) -> usize {
        self.id
            .into_iter()
            .zip(other.id)
            .map(|(x, y)| (x ^ y) as usize)
            .sum()
    }
}

#[derive(Default, Clone, Copy, Debug)]
pub struct Bucket {
    nodes: [Option<Node>; 8],
    len: usize,
    last_changed: chrono::DateTime<chrono::Utc>,
}

impl Bucket {}

pub struct Table {
    inner: [Option<Bucket>; 160],
}

impl Table {
    fn id(&self) -> Option<&Node> {
        let id = self.inner[0].as_ref()?;
        id.nodes[0].as_ref()
    }

    fn new(id: Node) -> Self {
        let mut inner: [Option<Bucket>; 160] = [None; 160];
        let mut nodes = [None; 8];
        nodes[0] = Some(id);

        inner[0] = Some(Bucket {
            nodes,
            len: 1,
            last_changed: Utc::now(),
        });

        Table { inner }
    }

    fn insert(&mut self, node: Node) -> Result<(), Report> {
        let id = self.id().ok_or(GeneralError::UninitializedNode)?;

        let distance = id.distance(&node);
        let len = distance.next_power_of_two() / 2;
        let n = len.ilog2() as usize;

        if let Some(bucket) = &mut self.inner[n] {
            if bucket.len == CAPACITY {
                // refresh ...
            } else {
                bucket.nodes[bucket.len] = Some(node);
                bucket.len += 1;
            }
        } else {
            let mut nodes = [None; 8];
            nodes[0] = Some(node);

            self.inner[n] = Some(Bucket {
                nodes,
                len: 1,
                last_changed: Utc::now(),
            });
        }

        Ok(())
    }

    // async fn find_torrent(&self, hash: Node) -> Node {
    //     ok.iter().map(|&k| k.distance(hash)).fold();
    //     Node(Default::default())
    // }
}

// pub async fn bootstrap_dht() -> Result<(), Report> {
//     let node_id = Node(rand::thread_rng().gen::<[u8; 20]>());

//     let dht_peers = [
//         "51.222.173.116:56216",
//         "87.10.113.39:50000",
//         "89.149.207.201:20033",
//     ];

//     let mut set = JoinSet::new();
//     let socket = Arc::new(UdpSocket::bind("0.0.0.0:4200").await?);

//     for peer in dht_peers {
//         set.spawn(ping_dht(
//             socket.clone(),
//             peer.parse::<SocketAddr>()?,
//             node_id.clone(),
//         ));
//     }

//     while let Some(ok) = set.join_next().await {
//         ok??;
//     }

//     Ok(())
// }

// async fn ping_dht(socket: Arc<UdpSocket>, peer: SocketAddr, node_id: Node) -> Result<(), Report> {
//     let ping: ExtMessage = krpc::Message::Query(Arguments {
//         method: Method::Ping,
//         id: node_id.0,
//         ..Default::default()
//     })
//     .into();

//     socket.send_to(&ping.to_bencode().unwrap(), peer).await?;

//     let mut buf = Vec::with_capacity(1024);
//     let (n, peer) = loop {
//         match socket.recv_from(&mut buf).await {
//             Ok(n) if n.0 != 0 => break n,
//             Ok(_) => continue,
//             Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
//             Err(e) => return Err(e.into()),
//         }
//     };

//     let resp = ExtMessage::from_bencode(&buf[..n]).unwrap();
//     debug!("{:?}: {:?}", peer, resp);

//     Ok(())
// }

#[cfg(test)]
mod tests {
    use num::BigUint;

    #[test]
    fn test_insert_dht() {
        use super::*;

        let id: [u8; 20] = rand::thread_rng().gen();
        let x = BigUint::new(
            id.chunks(4)
                .map(|x| u32::from_be_bytes(x.try_into().unwrap()))
                .collect(),
        );
        dbg!(x);

        let node = Node::new(id, "127.0.0.1:8080".parse().unwrap());
        let mut table = Table::new(node);

        let id: [u8; 20] = rand::thread_rng().gen();
        let other = Node::new(id, "127.0.0.1:4040".parse().unwrap());
        table.insert(other).unwrap();

        dbg!(table
            .inner
            .into_iter()
            .enumerate()
            .filter_map(|(i, b)| b.map(|b| (i, b.nodes)))
            .collect::<Vec<_>>());
    }
}
