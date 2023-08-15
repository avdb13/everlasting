use bytes::{Buf, BytesMut};
use chrono::Utc;
use futures_util::Future;
use std::{
    collections::HashMap,
    fmt,
    fs::File,
    io::{Cursor, Write},
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream,
    },
    sync::{
        mpsc::{self, Receiver, Sender, UnboundedReceiver},
        RwLock,
    },
    task::JoinSet,
};

use color_eyre::Report;

use tokio::{
    io::AsyncWriteExt,
    time::{sleep, timeout},
};
use tracing::debug;

use crate::{
    data::{Peer, Peers, TorrentInfo},
    extensions,
    framing::FrameReader,
    helpers::Timer,
    piece_manager::BitField,
};

use crate::pwp::*;

pub struct Router {
    pub torrent: Arc<TorrentInfo>,
    pub bitfield: Vec<u64>,
    pub peers: HashMap<SocketAddr, Connection>,
    pub peer_rx: Receiver<Peers>,
}

impl Router {
    pub fn new(torrent: Arc<TorrentInfo>, peer_rx: Receiver<Peers>) -> Self {
        Router {
            peer_rx,
            torrent,
            peers: HashMap::new(),
            bitfield: Vec::new(),
        }
    }

    pub async fn run(mut self) {
        let handshake = Arc::new(Handshake::new(self.torrent.hash));
        let (bitfield_tx, bitfield_rx) = mpsc::channel(100);

        // let manager = PieceManager::new(piece_len, pieces);
        // manager.listen(bitfield_rx);
        // let buffer = Buffer::new(pieces, piece_len);
        // let writer = Writer::new(piece_rx, buffer, piece_map);

        let (pieces, piece_len) = self
            .torrent
            .info
            .as_ref()
            .map(|info| (info.pieces.len(), info.piece_length))
            .unwrap();

        while let Some(peers) = self.peer_rx.recv().await {
            for peer in peers.into_iter() {
                let handshake = handshake.clone();
                let bitfield_tx = bitfield_tx.clone();

                let f = async move {
                    if let Ok(conn) = Connection::handshake(peer, handshake, pieces).await {
                        // if self.torrent.info.is_none() {}

                        conn.handle(bitfield_tx).await;
                    }
                };

                tokio::spawn(f);
            }
        }
    }
}

#[derive(Debug)]
pub struct Connection {
    pub inner: OwnedWriteHalf,
    pub buffer: BytesMut,
    pub state: Arc<RwLock<State>>,
    pub frame_rx: Receiver<Message>,
    pub real_len: usize,
    // pub piece_tx: Sender<Message>,
}

impl Connection {
    pub fn new(inner: OwnedWriteHalf, frame_rx: Receiver<Message>, real_len: usize) -> Self {
        Self {
            inner,
            frame_rx,
            real_len,
            buffer: BytesMut::new(),
            state: Arc::new(RwLock::new(State::default())),
        }
    }

    pub async fn handshake(
        peer: Peer,
        handshake: Arc<Handshake>,
        // piece_tx: Sender<Message>,
        pieces: usize,
    ) -> Result<Connection, Report> {
        let stream = timeout(Duration::from_secs(3), TcpStream::connect(peer.addr)).await??;
        let (r, mut w) = stream.into_split();

        let (frame_tx, frame_rx) = mpsc::channel(100);
        // spawn the FramedReader
        tokio::spawn(Connection::listen(r, frame_tx));

        w.write_all(&handshake.to_request()).await?;
        debug!("handshake was sent to [{}] ...", peer.addr);

        Ok(Connection::new(w, frame_rx, pieces))
    }

    pub async fn keep_alive(mut w: OwnedWriteHalf) {
        loop {
            sleep(Duration::from_secs(120)).await;
            let src = 0u32.to_be_bytes();
            let _ = w.write_all(&src).await;
        }
    }

    pub async fn listen(r: OwnedReadHalf, tx: Sender<Message>) -> Result<(), Report> {
        let peer_addr = r.peer_addr()?;

        let mut reader: FrameReader<Handshake> = FrameReader::new(r);
        if let Some(handshake) = reader.read_frame().await? {
            debug!("[{}] received the handshake ...", peer_addr);

            handshake.reserved;

            if handshake.reserved[7] | 0x01 == 1 {
                debug!("supports DHT: [{}]", peer_addr);
            }
        }
        // let mut reader: FrameReader<extensions::Handshake> = FrameReader::new(r);

        let mut reader: FrameReader<Message> = FrameReader::from(reader.inner, reader.buffer);
        while let Some(frame) = reader.read_frame().await? {
            debug!("received frame from [{}]", peer_addr);

            tx.send(frame).await?;
        }

        Ok(())
    }

    pub async fn handle(mut self, bitfield_tx: Sender<(SocketAddr, BitField)>) {
        let dst = self.inner.peer_addr().unwrap();

        // caching
        let mut have_buffer: Vec<usize> = Vec::with_capacity(64);
        let mut timer = Timer::new(Duration::from_secs(3));

        while let Some(message) = self.frame_rx.recv().await {
            if let Message::Have(idx) = message {
                timer.reset();
                have_buffer.push(idx);
            }

            // check if timer elapsed on all messages
            if timer.elapsed() {
                let bitfield = BitField::from_lazy(have_buffer.clone(), self.real_len);
                let _ = bitfield_tx.send((dst, bitfield)).await;
            }

            // simple state changes
            let mut state = self.state.write().await;
            match message {
                Message::Choke => {
                    state.choked = true;
                }
                Message::Unchoke => {
                    state.choked = false;
                }
                Message::Interested => {
                    state.interested = true;
                }
                Message::Uninterested => {
                    state.interested = false;
                }
                Message::Port(i) => {
                    state.dht_port = Some(i);
                }
                _ => {}
            }
            drop(state);
        }
    }

    pub async fn get_metadata(&self) {
        // self.inner
    }
}
