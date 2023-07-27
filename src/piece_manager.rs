use std::collections::VecDeque;
use std::fs::{create_dir, File};
use std::io::{Cursor, Seek, SeekFrom, Write};
use std::net::SocketAddr;
use std::ops::{BitAnd, BitAndAssign, BitXor};
use std::sync::Arc;

use ahash::{HashMap, HashMapExt};
use color_eyre::Report;
use rand::seq::{IteratorRandom, SliceRandom};
use tokio::sync::RwLock;
use tokio::sync::{mpsc::Receiver, watch};
use tracing::{debug, info};

use crate::data::Layout;
use crate::pwp::Message;

#[derive(Debug, Clone)]
pub struct BitField(Box<[usize]>);

impl BitField {
    pub fn new(v: Vec<usize>) -> Self {
        BitField(v.into_boxed_slice())
    }

    pub fn from_lazy(value: Vec<usize>, real_len: usize) -> Self {
        let mut v = vec![0usize; real_len];

        for x in value {
            let index = x / std::mem::size_of::<usize>();
            let offset = x % std::mem::size_of::<usize>();

            let i = v.get_mut(index).unwrap();
            *i |= 1 << offset;
        }

        BitField(v.into_boxed_slice())
    }
}

impl BitAndAssign for BitField {
    fn bitand_assign(&mut self, rhs: Self) {
        let left = &mut self.0;
        let right = rhs.0;

        assert_eq!(left.len(), right.len());

        left.iter_mut().zip(right.iter()).for_each(|(x, y)| *x &= y);
    }
}

impl BitXor for BitField {
    type Output = BitField;

    fn bitxor(self, rhs: Self) -> Self::Output {
        let left = self.0;
        let right = rhs.0;

        assert_eq!(left.len(), right.len());

        let inner = left
            .into_iter()
            .zip(right.iter())
            .map(|(x, y)| *x ^ y)
            .collect();

        BitField(inner)
    }
}

pub struct PieceManager {
    piece_len: usize,
    inner: Arc<RwLock<HashMap<SocketAddr, BitField>>>,
    // actual_pieces: PieceWrapper,
    rare_pieces: Arc<RwLock<BitField>>,
}

impl PieceManager {
    pub fn new(piece_len: usize, pieces: usize) -> Self {
        // amount of usizes we need to preserve the bitfield
        let v = vec![0usize, pieces / std::mem::size_of::<usize>() + 1];
        // let actual_pieces = Vec::with_capacity(pieces).into_boxed_slice();

        PieceManager {
            piece_len,
            // actual_pieces,
            inner: Arc::new(RwLock::new(HashMap::new())),
            rare_pieces: Arc::new(RwLock::new(BitField(v.into_boxed_slice()))),
        }
    }

    pub async fn listen(&self, mut rx: Receiver<(SocketAddr, BitField)>) {
        let inner = self.inner.clone();
        let rare_pieces = self.rare_pieces.clone();

        let f = async move {
            let mut buffer = Vec::with_capacity(4);

            while let Some((peer, bitfield)) = rx.recv().await {
                let mut inner = inner.write().await;

                if let Some(v) = inner.get_mut(&peer) {
                    *v &= bitfield.clone();
                } else {
                    inner.insert(peer, bitfield.clone());
                }

                if let Err(_) = buffer.push_within_capacity(bitfield) {
                    let reduced = buffer.clone().into_iter().reduce(|b, acc| b ^ acc).unwrap();
                    buffer.clear();

                    let mut rare_pieces = rare_pieces.write().await;
                    *rare_pieces &= reduced;
                }
            }
        };

        tokio::spawn(f);
    }

    // piece: <len=0009+X><id=7><index><begin><block>
    pub async fn write(&mut self, index: usize, begin: usize, block: &[u8]) -> Result<(), Report> {
        Ok(())
        // if this piece hasn't been initialized yet, do so
        // if self.actual_pieces.get(index).is_none() {
        //     let mut piece = self.actual_pieces.get_mut(index);
        //     piece = Some(&mut Some(vec![0u8; self.piece_len].into_boxed_slice()));
        // }

        // let piece = self.actual_pieces.get_mut(index).unwrap();
        // let piece = piece.as_mut().unwrap();

        // let mut piece = Cursor::new(piece as &mut [u8]);
        // piece.seek(SeekFrom::Start(begin as u64));
        // piece.write_all(block).map_err(Into::into)?;
    }

    // pub async fn validate()
}

type Block = Option<Box<[u8]>>;
// the boolean indicates whether this piece was flushed to the disk
type Piece = (Option<Box<[Block]>>, bool);

pub struct PieceWrapper(Box<[Piece]>);

// impl PieceWrapper {
//     pub async fn write(&mut self, index: usize, begin: usize, block: &[u8]) -> Result<(), Report> {}

//     pub async fn flush_piece(&mut self, index: usize) -> Result<(), Report> {
//         // first unwrap implies that the piece is within bounds and
//         // the second one implies that the piece is definitely available
//         let (piece, flushed) = self.0.get(index).unwrap();
//         let complete = piece.unwrap().iter().all(Option::is_some);
//         drop(piece);

//         if !flushed && complete {}
//     }
// }
