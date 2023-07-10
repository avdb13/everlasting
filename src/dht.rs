use std::{collections::HashMap, io::ErrorKind, net::SocketAddr, sync::Arc};

use bendy::{decoding::FromBencode, encoding::ToBencode};
use chrono::Utc;
use color_eyre::Report;
use lazy_static::lazy_static;
use rand::Rng;
use redb::TableDefinition;
use tokio::{net::UdpSocket, sync::Mutex, task::JoinSet};
use tracing::debug;

use crate::krpc::{self, Arguments, ExtMessage, Method};

const CAPACITY: usize = 8;

lazy_static! {
    static ref TABLE: Mutex<HashMap<[u8; 20], Node>> = Mutex::new(HashMap::new());
}

pub struct Table {
    id: Node,
    inner: Vec<Bucket>,
}
pub struct Bucket {
    nodes: [Node; 8],
    range: (usize, usize),
    last_changed: chrono::DateTime<chrono::Utc>,
}

#[derive(Default, Clone, Debug)]
pub struct Node(pub [u8; 20]);

impl Node {
    fn distance(&self, other: Node) -> u64 {
        self.0
            .into_iter()
            .zip(other.0)
            .map(|(x, y)| (x ^ y) as u64)
            .sum()
    }
}

// When a bucket is full of known good nodes,
// no more nodes may be added unless our own node ID falls within the range of the bucket.
// In that case, the bucket is replaced by two new buckets each with half the range of
// the old bucket and the nodes from the old bucket are distributed among the two new ones.
// For a new table with only one bucket,
// the full bucket is always split into two new buckets covering
// the ranges 0..2159 and 2159..2160.
//
impl Table {
    fn new(id: Node) -> Self {
        let bucket = Bucket {
            nodes: Default::default(),
            range: (0, 160),
            last_changed: Utc::now(),
        };

        Table {
            id,
            inner: vec![bucket],
        }
    }

    fn split(&mut self) {
        let left = Bucket {
            nodes: Default::default(),
            range: (0, 159),
            last_changed: Utc::now(),
        };
        let right = Bucket {
            nodes: Default::default(),
            range: (159, 160),
            last_changed: Utc::now(),
        };

        if self.inner.len() == 1 {
            self.inner = vec![left, right];
        }
    }

    fn add(&mut self, node: Node, i: usize) {
        let v = &self.inner[i];

        todo!()
    }
}

pub async fn bootstrap_dht() -> Result<(), Report> {
    let node_id = Node(rand::thread_rng().gen::<[u8; 20]>());

    let dht_peers = [
        "51.222.173.116:56216",
        "87.10.113.39:50000",
        "89.149.207.201:20033",
    ];

    let mut set = JoinSet::new();
    let socket = Arc::new(UdpSocket::bind("0.0.0.0:4200").await?);

    for peer in dht_peers {
        set.spawn(ping_dht(
            socket.clone(),
            peer.parse::<SocketAddr>()?,
            node_id.clone(),
        ));
    }

    while let Some(ok) = set.join_next().await {
        ok??;
    }

    Ok(())
}

async fn ping_dht(socket: Arc<UdpSocket>, peer: SocketAddr, node_id: Node) -> Result<(), Report> {
    let ping: ExtMessage = krpc::Message::Query(Arguments {
        method: Method::Ping,
        id: node_id.0,
        ..Default::default()
    })
    .into();

    socket.send_to(&ping.to_bencode().unwrap(), peer).await?;

    let mut buf = Vec::with_capacity(1024);
    let (n, peer) = loop {
        match socket.recv_from(&mut buf).await {
            Ok(n) if n.0 != 0 => break n,
            Ok(_) => continue,
            Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
            Err(e) => return Err(e.into()),
        }
    };

    let resp = ExtMessage::from_bencode(&buf[..n]).unwrap();
    debug!("{:?}: {:?}", peer, resp);

    Ok(())
}
