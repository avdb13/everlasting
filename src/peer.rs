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
        watch, RwLock,
    },
};

use color_eyre::Report;

use tokio::{
    io::AsyncWriteExt,
    time::{sleep, timeout},
};
use tracing::debug;

use crate::{
    bitmap::BitMap,
    data::{Peer, Peers, TorrentInfo},
    framing::{FrameReader, ParseCheck, ParseError},
    BLOCK_SIZE, PEER_ID,
};

pub type BitField = Vec<u8>;

pub struct Router {
    pub torrent: TorrentInfo,
    pub bitfield: Vec<u64>,
    pub peers: HashMap<SocketAddr, Connection>,
    pub peer_rx: Receiver<Peers>,
}

impl Router {
    pub fn new(torrent: TorrentInfo, peer_rx: Receiver<Peers>) -> Self {
        Router {
            torrent,
            peer_rx,
            peers: HashMap::new(),
            bitfield: Vec::new(),
        }
    }

    pub async fn run(self, torrent: TorrentInfo) {
        let len = torrent.bitfield_size() as usize;
        let mut rx = Router::connect(self.peer_rx, torrent).await;

        while let Some((task, peer)) = rx.recv().await {
            let f = async move {
                if let Ok(mut conn) = task.await {
                    conn.handle(peer.clone()).await;
                }
            };
            tokio::spawn(f);
        }
    }

    pub async fn connect(
        mut peer_rx: Receiver<Peers>,
        torrent: TorrentInfo,
    ) -> UnboundedReceiver<(impl Future<Output = Result<Connection, Report>>, Peer)> {
        let (tx, rx) = mpsc::unbounded_channel();

        // send connections back from peers that are beyond the handshake
        tokio::spawn(async move {
            let pieces = torrent.info.pieces.len();
            let handshake = Arc::new(Handshake::new(torrent.info.value));

            while let Some(peers) = peer_rx.recv().await {
                for peer in peers.into_iter() {
                    tx.send((
                        Router::handshake(peer.clone(), handshake.clone(), pieces),
                        peer,
                    ))
                    .unwrap();
                }
            }
        });

        rx
    }

    pub async fn handshake(
        peer: Peer,
        handshake: Arc<Handshake>,
        pieces: usize,
    ) -> Result<Connection, Report> {
        let stream = timeout(Duration::from_secs(3), TcpStream::connect(peer.addr)).await??;
        let (r, mut w) = stream.into_split();

        let (conn_tx, conn_rx) = mpsc::channel(100);
        tokio::spawn(Connection::listen(r, conn_tx));

        w.write_all(&handshake.to_request()).await?;
        debug!("handshake was sent to [{}] ...", peer.addr);

        Ok(Connection::new(w, conn_rx, pieces))
    }
}

#[derive(Debug)]
pub struct Connection {
    pub writer: OwnedWriteHalf,
    pub buffer: BytesMut,
    pub rx: Receiver<Message>,
    pub state: Arc<RwLock<State>>,
    pub dht_port: Option<u16>,
}

impl Connection {
    pub fn new(writer: OwnedWriteHalf, rx: Receiver<Message>, pieces: usize) -> Self {
        Self {
            writer,
            rx,
            buffer: BytesMut::new(),
            state: Arc::new(RwLock::new(State::new())),
            dht_port: None,
        }
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
        let mut output = File::create("peers/".to_owned() + &peer_addr.to_string())?;

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

            write!(output, "{:?}\n", frame)?;
            tx.send(frame).await?;
        }

        Ok(())
    }

    pub async fn handle(&mut self, peer: Peer) {
        let mut bitmap = BitMap::new();

        while let Some(message) = self.rx.recv().await {
            match message {
                Message::Choke => {
                    let mut state = self.state.write().await;
                    state.choked = true;
                }
                Message::Unchoke => {
                    let mut state = self.state.write().await;
                    state.choked = false;
                }
                Message::Interested => {
                    let mut state = self.state.write().await;
                    state.interested = true;
                }
                Message::Uninterested => {
                    let mut state = self.state.write().await;
                    state.interested = false;
                }
                Message::Have(idx) => {
                    let _ = bitmap.handle(peer.addr, Message::Have(idx)).await;
                }
                Message::BitField(v) => {
                    let _ = bitmap.handle(peer.addr, Message::BitField(v)).await;
                }
                Message::Request {
                    index: _,
                    begin: _,
                    length: _,
                } => todo!(),
                Message::Piece {
                    index: _,
                    begin: _,
                    block: _,
                } => todo!(),
                Message::Cancel {
                    index: _,
                    begin: _,
                    length: _,
                } => todo!(),
                Message::Port(n) => self.dht_port = Some(n),
                _ => todo!(),
            }

            let state = self.state.read().await;
            if !state.choked && state.interested {
                let (_owner, idx) = bitmap.request_block();
                let msg = Message::Request {
                    index: idx,
                    begin: 0,
                    length: *BLOCK_SIZE,
                };
                self.writer.write_all(&msg.to_request()).await;
            }
        }
    }

    pub async fn download() {}
}

#[derive(Debug)]
pub struct State {
    pub choked: bool,
    pub interested: bool,
    pub peer_choked: bool,
    pub peer_interested: bool,
}

impl State {
    fn new() -> Self {
        Self {
            choked: true,
            interested: false,
            peer_choked: true,
            peer_interested: false,
        }
    }
}

pub trait Request {
    fn to_request(&self) -> Vec<u8>;
    fn from_request(v: &[u8]) -> Option<Self>
    where
        Self: Sized;
}

// the peer id will presumably be sent after the recipient sends its own handshake
#[derive(Clone, Debug)]
pub struct Handshake {
    pstr: Option<String>,
    reserved: [u8; 8],
    hash: [u8; 20],
    peer_id: [u8; 20],
    payload: Option<Vec<u8>>,
}

impl Handshake {
    fn new(hash: [u8; 20]) -> Self {
        let mut reserved = [0u8; 8];

        // DHT protocol
        reserved[7] |= 0x01;
        // extension protocol
        reserved[5] &= 0x20;

        Self {
            pstr: Some("BitTorrent protocol".to_owned()),
            reserved,
            hash,
            peer_id: *PEER_ID,
            payload: None,
        }
    }
}

impl Request for Handshake {
    fn to_request(&self) -> Vec<u8> {
        let pstr = self.pstr.as_ref().unwrap();
        [
            &[pstr.len() as u8],
            pstr.as_bytes(),
            &self.reserved,
            &self.hash,
            &self.peer_id,
        ]
        .concat()
    }

    fn from_request(v: &[u8]) -> Option<Self> {
        let reserved = v[20..28].try_into().ok()?;
        let hash = v[28..48].try_into().ok()?;
        let peer_id = v[48..68].try_into().ok()?;
        let payload = if v[68..].len() != 0 {
            Some(v[68..].to_vec())
        } else {
            None
        };

        Some(Self {
            pstr: None,
            reserved,
            hash,
            peer_id,
            payload,
        })
    }
}

impl ParseCheck for Handshake {
    // <1:pstrlen><19:pstr><8:reserved><20:info_hash><20:peer_id>

    fn check(v: &mut Cursor<&[u8]>) -> Result<(), ParseError> {
        let n: Vec<_> = (0..68).map_while(|_| Self::get_u8(v)).collect();
        if n.len() != 68 {
            return Err(ParseError::Incomplete);
        }

        Ok(())
    }

    fn parse(v: &mut Cursor<&[u8]>) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        let rem: Vec<_> = (0..68).map_while(|_| Self::get_u8(v)).collect();
        if rem.len() != 68 {
            return Err(ParseError::Incomplete);
        }

        let reserved = rem[20..28].try_into().unwrap();
        let hash = rem[28..48].try_into().unwrap();
        let peer_id = rem[48..68].try_into().unwrap();

        Ok(Self {
            pstr: None,
            reserved,
            hash,
            peer_id,
            payload: Some(rem[68..].to_vec()),
        })
    }
}

#[repr(i8)]
#[derive(Clone)]
pub enum Message {
    Handshake(Handshake) = -1,
    Choke,
    Unchoke,
    Interested,
    Uninterested,
    Have(usize),
    BitField(Vec<u64>),
    Request {
        index: usize,
        begin: usize,
        length: usize,
    },
    Piece {
        index: usize,
        begin: usize,
        block: Vec<u8>,
    },
    Cancel {
        index: usize,
        begin: usize,
        length: usize,
    },
    Port(u16),
    Extended = 20,
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Handshake(_) => write!(f, "Handshake"),
            Self::Choke => write!(f, "Choke"),
            Self::Unchoke => write!(f, "Unchoke"),
            Self::Interested => write!(f, "Interested"),
            Self::Uninterested => write!(f, "Uninterested"),
            Self::Have(_) => f.debug_tuple("Have").finish(),
            Self::BitField(_) => f.debug_tuple("BitField").finish(),
            Self::Request { .. } => f.debug_struct("Request").finish(),
            Self::Piece { .. } => f.debug_struct("Piece").finish(),
            Self::Cancel { .. } => f.debug_struct("Cancel").finish(),
            Self::Port(_) => f.debug_tuple("Port").finish(),
            Self::Extended => write!(f, "Extended"),
        }
    }
}

impl Request for Message {
    fn to_request(&self) -> Vec<u8> {
        use Message::*;

        match self {
            Handshake(handshake) => handshake.to_request(),
            Choke => [0, 0, 0, 1, 0].to_vec(),
            Unchoke => [0, 0, 0, 1, 1].to_vec(),
            Interested => [0, 0, 0, 1, 2].to_vec(),
            Uninterested => [0, 0, 0, 1, 3].to_vec(),
            Have(x) => [[0, 0, 0, 5, 4].as_slice(), &(*x as u32).to_be_bytes()].concat(),
            BitField(v) => {
                let v: Vec<_> = v.iter().flat_map(|v| v.to_be_bytes()).collect();
                [
                    &(v.len() as u32 * 8 + 1).to_be_bytes(),
                    [5u8].as_slice(),
                    v.as_slice(),
                ]
                .concat()
            }
            Request {
                index,
                begin,
                length,
            } => [
                &(13u32).to_be_bytes(),
                [6u8].as_slice(),
                &(*index as u32).to_be_bytes(),
                &(*begin as u32).to_be_bytes(),
                &(*length as u32).to_be_bytes(),
            ]
            .concat(),
            Piece {
                index,
                begin,
                block,
            } => [
                &((9 + block.len()) as u32).to_be_bytes(),
                [7u8].as_slice(),
                &(*index as u32).to_be_bytes(),
                &(*begin as u32).to_be_bytes(),
                block.as_slice(),
            ]
            .concat(),
            Cancel {
                index,
                begin,
                length,
            } => [
                &(13u32).to_be_bytes(),
                [8u8].as_slice(),
                &(*index as u32).to_be_bytes(),
                &(*begin as u32).to_be_bytes(),
                &(*length as u32).to_be_bytes(),
            ]
            .concat(),
            Port(i) => [&(3u32).to_be_bytes(), [9u8].as_slice(), &i.to_be_bytes()].concat(),
            Extended => todo!(),
        }
    }

    fn from_request(v: &[u8]) -> Option<Self>
    where
        Self: Sized,
    {
        let mut v = Cursor::new(v);
        Message::parse(&mut v).ok()
    }
}
impl ParseCheck for Message {
    fn check(v: &mut Cursor<&[u8]>) -> Result<(), ParseError> {
        let n: Vec<_> = (0..4).map_while(|_| Self::get_u8(v)).collect();
        if n.len() != 4 {
            return Err(ParseError::Incomplete);
        }
        let n = u32::from_be_bytes(n.try_into().unwrap()) as usize;

        let rem: Vec<_> = (0..n).map(|_| Self::get_u8(v)).collect();
        if rem.len() != n {
            return Err(ParseError::Incomplete);
        }

        Ok(())
    }

    fn parse(v: &mut Cursor<&[u8]>) -> Result<Self, ParseError>
    where
        Self: Sized,
    {
        use Message::*;

        let n: Vec<_> = (0..4).map_while(|_| Self::get_u8(v)).collect();
        if n.len() != 4 {
            return Err(ParseError::Incomplete);
        }
        let n = u32::from_be_bytes(n.try_into().unwrap()) as usize;
        debug!(n);

        let mut rem: Vec<_> = (0..n).map_while(|_| Self::get_u8(v)).collect();
        if rem.len() != n {
            return Err(ParseError::Incomplete);
        }

        let res = match rem[0] {
            0 => Choke,
            1 => Unchoke,
            2 => Interested,
            3 => Uninterested,
            4 => Have(u32::from_be_bytes(rem[1..5].try_into().unwrap()) as usize),
            5 => {
                let tail = (rem.len() - 1) % 8;
                if tail != 0 {
                    rem.append(&mut vec![0u8; 8 - tail]);
                }

                let payload = rem[1..rem.len()]
                    .chunks(8)
                    .map(|c| u64::from_be_bytes(c.try_into().unwrap()))
                    .collect();

                BitField(payload)
            }
            6 => {
                let index = u32::from_be_bytes(rem[1..5].try_into().unwrap()) as usize;
                let begin = u32::from_be_bytes(rem[5..9].try_into().unwrap()) as usize;
                let length = u32::from_be_bytes(rem[9..13].try_into().unwrap()) as usize;

                Request {
                    index,
                    begin,
                    length,
                }
            }
            7 => {
                let index = u32::from_be_bytes(rem[1..5].try_into().unwrap()) as usize;
                let begin = u32::from_be_bytes(rem[5..9].try_into().unwrap()) as usize;

                let block = rem[..rem.len()].to_vec();

                Piece {
                    index,
                    begin,
                    block,
                }
            }
            8 => {
                let index = u32::from_be_bytes(rem[1..5].try_into().unwrap()) as usize;
                let begin = u32::from_be_bytes(rem[5..9].try_into().unwrap()) as usize;
                let length = u32::from_be_bytes(rem[9..13].try_into().unwrap()) as usize;

                Cancel {
                    index,
                    begin,
                    length,
                }
            }
            9 => {
                let port = u16::from_be_bytes(rem[1..3].try_into().unwrap());

                Port(port)
            }
            20 => Extended,
            _ => panic!(),
        };

        Ok(res)
    }
}
