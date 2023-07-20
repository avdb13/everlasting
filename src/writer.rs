use std::collections::HashMap;

use crate::{
    data::Peer,
    piece_map::PieceMap,
    pwp::{Message, Request},
};
use tokio::{io::AsyncWriteExt, net::tcp::OwnedWriteHalf, sync::mpsc::Receiver};
use tracing::debug;

pub struct Buffer {
    pub inner: Box<[Box<[u8]>]>,
    pub pieces: usize,
    pub piece_len: usize,
}

impl Buffer {
    fn new(piece_len: usize, pieces: usize) -> Self {
        let buffer = vec![vec![0u8; piece_len].into_boxed_slice(); pieces];
        Self {
            inner: buffer.into_boxed_slice(),
            pieces,
            piece_len,
        }
    }

    fn write(&mut self, index: usize, begin: usize, block: Vec<u8>) {
        assert!(index >= self.pieces);
        let piece = self.inner.get_mut(index).unwrap();
        assert!(begin >= self.piece_len - begin);
        let mut cut = &mut piece[begin..begin + block.len()];

        for i in 0..block.len() {
            piece[begin + i] = block[i];
        }
    }
}

pub struct Writer {
    writer_rx: Receiver<(Peer, OwnedWriteHalf)>,
    req_rx: Receiver<(Message, Peer)>,
    buffer: Buffer,
    map: PieceMap,
}

impl Writer {
    pub fn new(
        writer_rx: Receiver<(Peer, OwnedWriteHalf)>,
        req_rx: Receiver<(Message, Peer)>,
        buffer: Buffer,
        map: PieceMap,
    ) -> Self {
        Self {
            writer_rx,
            req_rx,
            buffer,
            map,
        }
    }

    pub async fn listen(&mut self) {}

    pub async fn resolve(&mut self) {
        let v = self.map.request_piece().await;
        for (peer, msg) in v {
            let w = self.w.get_mut(&peer).unwrap();
            let _ = w.write_all(&msg.to_request()).await;

            debug!("requested piece");
        }
    }

    pub async fn download(&mut self) {
        while let Some((msg, peer)) = self.rx.recv().await {
            match msg {
                Message::Piece {
                    index,
                    begin,
                    block,
                } => {
                    self.buffer.write(index, begin, block);
                }
                Message::Cancel {
                    index,
                    begin,
                    length,
                } => {}
                _ => panic!(),
            }
        }
    }
}
