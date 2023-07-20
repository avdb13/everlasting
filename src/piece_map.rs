use std::sync::Arc;

use rand::seq::{IteratorRandom, SliceRandom};
use tokio::sync::RwLock;
use tokio::sync::{mpsc::Receiver, watch};
use tracing::{debug, info};

use crate::{data::Peer, pwp::Message, BLOCK_SIZE};

pub struct BitField(Box<[bool]>);

impl BitField {
    fn to_boolean(x: u8) -> impl Iterator<Item = bool> {
        (0..8).map(move |i| x & (1 << i) != 0)
    }

    fn new(v: Vec<u64>, real_len: usize) -> Self {
        let mut bitfield: Vec<_> = v
            .into_iter()
            .map(u64::to_be_bytes)
            .flat_map(|x| x.into_iter().flat_map(BitField::to_boolean))
            .collect();
        bitfield = bitfield[..real_len].to_vec();

        BitField(bitfield.into_boxed_slice())
    }
}

pub type PeerLock = Arc<RwLock<Box<[Vec<Peer>]>>>;

pub struct PieceMap {
    inner: PeerLock,
}

impl PieceMap {
    pub fn new(real_len: usize) -> Self {
        let v = Vec::with_capacity(8);

        let ok = vec![v; real_len];
        PieceMap {
            inner: Arc::new(RwLock::new(ok.into_boxed_slice())),
        }
    }

    pub async fn request_piece(&self) -> Vec<(Peer, Message)> {
        let inner = self.inner.read().await;
        let rng = &mut rand::thread_rng();

        let i = loop {
            let i = (0..inner.len()).choose(rng).unwrap();
            if !inner[i].is_empty() && inner[i].len() <= 3 {
                break i;
            }
            continue;
        };

        let peers = &inner[i];
        let blocks = 2usize.pow(20) / *BLOCK_SIZE;

        peers
            .iter()
            .cycle()
            .take(blocks)
            .enumerate()
            .map(|(i, peer)| {
                (
                    peer.to_owned(),
                    Message::Request {
                        index: i,
                        begin: i * *BLOCK_SIZE,
                        length: *BLOCK_SIZE,
                    },
                )
            })
            .collect()
    }

    pub async fn listen(&self, mut piece_rx: Receiver<(Message, Peer)>) {
        let inner = self.inner.clone();

        let f = async move {
            while let Some((message, owner)) = piece_rx.recv().await {
                match message {
                    Message::Have(i) => PieceMap::have(inner.clone(), i, owner).await,
                    Message::BitField(v) => PieceMap::bitfield(inner.clone(), v, owner).await,
                    _ => panic!(),
                }
            }
        };

        tokio::spawn(f);
    }

    pub async fn have(inner: PeerLock, i: usize, owner: Peer) {
        let mut inner = inner.write().await;

        let e = &mut inner[i];
        e.push(owner);
    }

    pub async fn bitfield(inner: PeerLock, v: Vec<u64>, owner: Peer) {
        let mut inner = inner.write().await;

        let bitfield = BitField::new(v, inner.len());

        for (i, &x) in bitfield.0.iter().enumerate() {
            if x {
                let e = &mut inner[i];
                e.push(owner.clone());
            }
        }
    }
}
