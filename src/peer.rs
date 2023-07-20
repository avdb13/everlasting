use bytes::{Buf, BytesMut};
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
    framing::FrameReader,
    piece_map::PieceMap,
    writer::Writer,
};

use crate::pwp::*;

pub type BitField = Vec<u8>;

pub struct Router {
    pub torrent: Arc<TorrentInfo>,
    pub bitfield: Vec<u64>,
    pub peers: HashMap<SocketAddr, Connection>,
    pub peer_rx: Receiver<Peers>,
}

impl Router {
    pub fn new(torrent: TorrentInfo, peer_rx: Receiver<Peers>) -> Self {
        Router {
            peer_rx,
            torrent: Arc::new(torrent),
            peers: HashMap::new(),
            bitfield: Vec::new(),
        }
    }

    pub async fn run(mut self) {
        let len = self.torrent.info.pieces.len();
        let handshake = Arc::new(Handshake::new(self.torrent.info.value));

        let (piece_tx, piece_rx) = mpsc::channel(100);
        let (bitfield_tx, bitfield_rx) = mpsc::channel(100);
        let (writer_tx, writer_rx) = mpsc::channel(100);

        let piece_map = PieceMap::new(len);
        let writer = Writer::new();

        tokio::spawn(async move { piece_map.listen(piece_rx).await });

        while let Some(peers) = self.peer_rx.recv().await {
            for peer in peers.into_iter() {
                let handshake = handshake.clone();
                let piece_tx = piece_tx.clone();
                let writer_tx = writer_tx.clone();

                tokio::spawn(async move {
                    if let Ok((conn, w)) = Connection::handshake(peer, handshake).await {
                        let _ = writer_tx.send((peer, w)).await;
                        conn.handle(piece_tx, bitfield_tx).await;
                    }
                });
            }
        }
    }
}

#[derive(Debug)]
pub struct Connection {
    pub inner: Peer,
    pub buffer: BytesMut,
    pub state: Arc<RwLock<State>>,
    pub dht_port: Option<u16>,
    pub frame_rx: Receiver<Message>,
    // pub piece_tx: Sender<Message>,
}

impl Connection {
    pub fn new(
        inner: Peer,
        frame_rx: Receiver<Message>,
        // piece_tx: Sender<Message>,
        // pieces: usize,
    ) -> Self {
        Self {
            inner,
            frame_rx,
            dht_port: None,
            buffer: BytesMut::new(),
            state: Arc::new(RwLock::new(State::new())),
        }
    }

    pub async fn handshake(
        peer: Peer,
        handshake: Arc<Handshake>,
        // piece_tx: Sender<Message>,
        // pieces: usize,
    ) -> Result<(Connection, OwnedWriteHalf), Report> {
        let stream = timeout(Duration::from_secs(3), TcpStream::connect(peer.addr)).await??;
        let (r, mut w) = stream.into_split();

        let (frame_tx, frame_rx) = mpsc::channel(100);
        // spawn the FramedReader
        tokio::spawn(Connection::listen(r, frame_tx));

        w.write_all(&handshake.to_request()).await?;
        debug!("handshake was sent to [{}] ...", peer.addr);

        Ok((Connection::new(peer, frame_rx), w))
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

            if handshake.reserved[7] | 0x01 == 1 {
                debug!("supports DHT: [{}]", peer_addr);
            }
        }

        let mut reader: FrameReader<Message> = FrameReader::from(reader.inner, reader.buffer);
        while let Some(frame) = reader.read_frame().await? {
            debug!("received frame from [{}]: {:?}", peer_addr, frame);
            tx.send(frame).await?;
        }

        Ok(())
    }

    pub async fn handle(
        mut self,
        bitfield_tx: Sender<(Message, Peer)>,
        piece_tx: Sender<(Message, Peer)>,
    ) {
        while let Some(message) = self.frame_rx.recv().await {
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
                _ => {}
            }
            drop(state);

            match message {
                Message::Have(idx) => {
                    let _ = bitfield_tx
                        .send((Message::Have(idx), self.inner.clone()))
                        .await;
                }
                Message::BitField(v) => {
                    let _ = bitfield_tx
                        .send((Message::BitField(v), self.inner.clone()))
                        .await;
                }
                Message::Request {
                    index: _,
                    begin: _,
                    length: _,
                } => todo!(),
                Message::Piece {
                    index: i,
                    begin: j,
                    block: v,
                } => {
                    let _ = piece_tx
                        .send((Message::Have(idx), self.inner.clone()))
                        .await;
                }
                Message::Cancel {
                    index: _,
                    begin: _,
                    length: _,
                } => todo!(),
                Message::Port(n) => self.dht_port = Some(n),
                _ => {}
            }
        }
    }
}
