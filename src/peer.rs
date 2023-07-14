use bendy::encoding::ToBencode;
use bytes::{Buf, BytesMut};
use futures_util::Future;
use std::{
    collections::HashMap,
    fmt,
    io::{Cursor, ErrorKind},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{
        tcp::{OwnedReadHalf, OwnedWriteHalf},
        TcpStream, UdpSocket,
    },
    sync::mpsc::{self, channel, Receiver, Sender, UnboundedReceiver},
    task::JoinSet,
};

use color_eyre::{owo_colors::OwoColorize, Report};
use rand::Rng;

use tokio::{
    io::{self, split, AsyncReadExt, AsyncWriteExt},
    time::{sleep, timeout},
};
use tracing::{debug, error, field::debug, info};

use crate::{
    data::{GeneralError, Peer, Peers, TorrentInfo},
    dht::Node,
    extensions::ExtensionHeader,
    framing::{FrameReader, ParseCheck, ParseError},
    helpers::{self, decode, range_to_array},
    krpc::{self, Arguments, ExtMessage, Method},
    magnet::MagnetInfo,
    udp::{Request, Response},
    PEER_ID,
};

pub struct Router {
    pub torrent: TorrentInfo,
    pub bitfield: Vec<u8>,
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
    pub state: State,
    pub dht_port: Option<u16>,
}

impl Connection {
    pub fn new(writer: OwnedWriteHalf, rx: Receiver<Message>, pieces: usize) -> Self {
        Self {
            writer,
            rx,
            buffer: BytesMut::new(),
            state: State::new(pieces),
            dht_port: None,
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

        let mut reader: FrameReader<Message> = FrameReader::new(reader.take_inner());
        while let Some(frame) = reader.read_frame().await? {
            debug!("received frame from [{}]", peer_addr);
            tx.send(frame).await?;
        }

        Ok(())
    }

    pub async fn handle(&mut self, peer: Peer) {
        debug!("handling for [{}]", peer.addr);

        while let Some(message) = self.rx.recv().await {
            match message {
                Message::Choke => {
                    self.state.choked = true;
                }
                Message::Unchoke => {
                    self.state.choked = false;
                }
                Message::Interested => {
                    self.state.interested = true;
                }
                Message::Uninterested => {
                    self.state.interested = false;
                }
                Message::Have(idx) => {
                    debug!(idx);
                    panic!();
                    // let n = self.state.remote_bitfield[idx / 8] as usize;
                    // let missing = n | (idx % 8);
                    // debug!(missing = ?missing);
                    // let n = self.state.remote_bitfield.get_mut(idx / 8).unwrap();
                    // *n |= (idx % 8) as u8;
                }
                Message::Bitfield(v) => {
                    self.state.remote_bitfield = v;
                }
                Message::Request {
                    index,
                    begin,
                    length,
                } => todo!(),
                Message::Piece {
                    index,
                    begin,
                    block,
                } => todo!(),
                Message::Cancel {
                    index,
                    begin,
                    length,
                } => todo!(),
                Message::Port(n) => self.dht_port = Some(n),
                _ => todo!(),
            }
        }
    }
}

#[derive(Debug)]
pub struct State {
    pub choked: bool,
    pub interested: bool,
    pub peer_choked: bool,
    pub peer_interested: bool,

    pub remote_bitfield: Vec<u8>,
}

impl State {
    fn new(pieces: usize) -> Self {
        let len = (pieces / 8) + 1;
        Self {
            choked: true,
            interested: false,
            peer_choked: true,
            peer_interested: false,
            remote_bitfield: vec![0; len],
        }
    }
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
    fn to_request(&self) -> Vec<u8> {
        let pstr = self.pstr.as_ref().unwrap();
        let handshake = [
            &[pstr.len() as u8],
            pstr.as_bytes(),
            &self.reserved,
            &self.hash,
            &self.peer_id,
        ]
        .concat();

        // let extended_handshake =
        handshake
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
    Bitfield(Vec<u8>),
    Request {
        index: usize,
        begin: usize,
        length: usize,
    },
    Piece {
        index: usize,
        begin: usize,
        block: Vec<usize>,
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
            Self::Bitfield(_) => f.debug_tuple("Bitfield").finish(),
            Self::Request { .. } => f.debug_struct("Request").finish(),
            Self::Piece { .. } => f.debug_struct("Piece").finish(),
            Self::Cancel { .. } => f.debug_struct("Cancel").finish(),
            Self::Port(_) => f.debug_tuple("Port").finish(),
            Self::Extended => write!(f, "Extended"),
        }
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

        let rem: Vec<_> = (0..n).map_while(|_| Self::get_u8(v)).collect();
        if rem.len() != n {
            return Err(ParseError::Incomplete);
        }

        let res = match rem[0] {
            0 => Choke,
            1 => Unchoke,
            2 => Interested,
            3 => Uninterested,
            4 => Have(u32::from_be_bytes(rem[1..].try_into().unwrap()) as usize),
            5 => {
                let payload = rem[..rem.len()]
                    .iter()
                    .flat_map(|n| (0..8).map(|i| (n >> i) & 1).collect::<Vec<_>>())
                    .collect();

                Bitfield(payload)
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

                let block = rem[..rem.len()].iter().map(|&i| i as usize).collect();

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

//impl Message {
//    pub fn to_request<V: AsRef<[u8]>>(&self, payload: Option<V>) -> Vec<u8> {
//        // handshake has a different pattern
//        //
//        // let message = if let Some(payload) = payload {
//        //     [&[self.to_discriminant()], payload.as_ref()].concat()
//        // } else {
//        //     [self.to_discriminant()].to_vec()
//        // };
//        // [[message.len() as u8].to_vec(), message].concat()
//        Vec::new()
//    }
