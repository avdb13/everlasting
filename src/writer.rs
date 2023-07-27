use std::{collections::HashMap, sync::Arc};

use crate::{
    data::Peer,
    piece_map::PieceMap,
    pwp::{Message, Request},
};
use tokio::{
    io::AsyncWriteExt,
    net::tcp::OwnedWriteHalf,
    sync::{mpsc::Receiver, RwLock},
};
use tracing::debug;

pub struct Buffer {
    pub inner: Box<[Box<[u8]>]>,
    pub pieces: usize,
    pub piece_len: usize,
}

impl Buffer {
    pub fn new(piece_len: usize, pieces: usize) -> Self {
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
    pub lock: Arc<RwLock<HashMap<Peer, OwnedWriteHalf>>>,
    pub req_rx: Receiver<(Message, Peer)>,
    pub buffer: Buffer,
}

impl Writer {
    pub fn new(req_rx: Receiver<(Message, Peer)>, buffer: Buffer, map: PieceMap) -> Self {
        Self {
            lock: Arc::new(RwLock::new(HashMap::new())),
            req_rx,
            buffer,
        }
    }

    pub async fn listen(
        lock: Arc<RwLock<HashMap<Peer, OwnedWriteHalf>>>,
        mut writer_rx: Receiver<(Peer, OwnedWriteHalf)>,
    ) {
        while let Some((peer, w)) = writer_rx.recv().await {
            let mut lock = lock.write().await;

            if let Some(_) = lock.insert(peer, w) {
                panic!();
            }
        }
    }

    pub async fn request_piece(&mut self) {
        let mut lock = self.lock.write().await;

        let v = self.map.request_piece().await;
        for (peer, msg) in v {
            let w = lock.get_mut(&peer).unwrap();
            let _ = w.write_all(&msg.to_request()).await;

            debug!("requested piece");
        }
    }

    pub async fn download(&mut self) {
        while let Some((msg, _)) = self.req_rx.recv().await {
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
