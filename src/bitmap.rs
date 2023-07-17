use std::{
    collections::HashMap,
    net::SocketAddr,
    ops::{BitXor, BitXorAssign},
};

use rand::{seq::SliceRandom, Rng};
use tokio::sync::mpsc::{Receiver, Sender};

use crate::{
    data::Peer,
    peer::{BitField, Message},
};

#[derive(Debug, Clone)]
pub struct ByteWrapper(Vec<u64>);

impl ByteWrapper {
    fn new() -> Self {
        ByteWrapper(Vec::new())
    }
}

impl BitXor for ByteWrapper {
    type Output = ByteWrapper;

    fn bitxor(self, rhs: Self) -> Self::Output {
        assert!(self.0.len() == rhs.0.len());

        ByteWrapper(self.0.iter().zip(rhs.0).map(|(x, y)| x ^ y).collect())
    }
}

impl ByteWrapper {
    fn random_piece(&self) -> (usize, usize) {
        let piece_idx = rand::thread_rng().gen_range(0..self.0.len());
        let byte_idx = rand::thread_rng().gen_range(0..8);

        let piece = self.0[piece_idx].to_be_bytes();
        let byte = piece[byte_idx];

        let bits: Vec<_> = (0..8)
            .filter_map(|i| match byte & (1 << i) {
                0 => None,
                x => Some(8 - 1 - i),
            })
            .collect();
        let bit_idx = bits.choose(&mut rand::thread_rng()).unwrap();

        (piece_idx, byte_idx * 8 + bit_idx)
    }

    fn set(&mut self, idx: usize) {
        let n = self.0[idx / 64] as usize;
        let missing = n | (idx % 64);

        let n = self.0.get_mut(idx / 64).unwrap();
        *n |= (idx % 64) as u64;
    }
}

pub struct BitMap {
    map: HashMap<SocketAddr, ByteWrapper>,
}

impl BitMap {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub async fn handle(&mut self, peer: SocketAddr, message: Message) {
        match message {
            Message::BitField(v) => {
                self.map.insert(peer, ByteWrapper(v));
            }
            Message::Have(idx) => {
                let v = self.map.get_mut(&peer).unwrap();
                v.set(idx);
            }
            _ => panic!(),
        }
    }

    pub fn request_block(&self) -> (SocketAddr, usize) {
        let fields: Vec<_> = self.map.keys().collect();
        // let rare_blocks = fields.fold(ByteWrapper::new(), |acc, v| acc ^ v.clone());

        let idx = rand::thread_rng().gen_range(0..fields.len());
        let key = fields[idx];

        let rand_field = self.map.get(key).unwrap();
        let (i, j) = rand_field.random_piece();

        (key.to_owned(), i * 64 + j)
    }
}
