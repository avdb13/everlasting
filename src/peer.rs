use bendy::encoding::ToBencode;
use futures_util::Future;
use std::{
    collections::HashMap,
    io::ErrorKind,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{TcpStream, UdpSocket},
    sync::mpsc::{self, channel, Receiver, Sender},
    task::JoinSet,
};

use color_eyre::{owo_colors::OwoColorize, Report};
use rand::Rng;

use tokio::{
    io::{self, split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    time::{sleep, timeout},
};
use tracing::{debug, error, field::debug, info};

use crate::{
    data::{GeneralError, Peer, Peers, TorrentInfo},
    dht::Node,
    extensions::ExtensionHeader,
    helpers::{self, decode, range_to_array},
    krpc::{self, Arguments, ExtMessage, Method},
    magnet::MagnetInfo,
    udp::{Request, Response},
    PEER_ID,
};

type Bitfield = Vec<u8>;
type ExtConnection = (Connection, (ReadHalf<TcpStream>, Sender<Message>, Peer));

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
    ) -> Receiver<(impl Future<Output = Result<Connection, Report>>, Peer)> {
        let (tx, rx) = mpsc::channel(100);

        // send connections back from peers that are beyond the handshake
        tokio::spawn(async move {
            while let Some(peers) = peer_rx.recv().await {
                for peer in peers.into_iter() {
                    let handshake = Handshake::new(torrent.info.value);

                    let _ = tx
                        .send((Router::handshake(peer.clone(), handshake.clone()), peer))
                        .await;
                }
            }
        });

        rx
    }

    pub async fn handshake(peer: Peer, handshake: Handshake<'_>) -> Result<Connection, Report> {
        let mut stream = timeout(Duration::from_secs(3), TcpStream::connect(peer.addr)).await??;

        stream.write_all(&handshake.to_request()).await?;
        stream.flush().await?;
        debug!("handshake was sent to [{}] ...", peer.addr);

        // <1:pstrlen><19:pstr><8:reserved><20:info_hash><20:peer_id>
        let mut buf = [0u8; 68];

        match timeout(Duration::from_secs(3), stream.read_exact(&mut buf)).await? {
            Ok(n) if n == buf.len() => {
                debug!("[{}] received the handshake ...", peer.addr);
                let handshake = Handshake::from_response(&buf);

                if handshake.reserved[7] | 0x01 == 1 {
                    debug!("supports DHT: [{}]", peer.addr);
                }
            }
            _ => return Err(GeneralError::Timeout(Some(peer.addr)).into()),
        };

        let (reader, writer) = split(stream);
        let (conn_tx, conn_rx) = mpsc::channel(100);

        let conn = Connection::new(writer, conn_rx);
        tokio::spawn(Connection::listen(reader, conn_tx, peer.clone()));

        Ok(conn)
    }
}

#[derive(Debug)]
pub struct Connection {
    pub writer: WriteHalf<TcpStream>,
    pub rx: Receiver<Message>,
    pub state: State,
    pub dht_port: Option<u16>,
}

impl Connection {
    pub fn new(writer: WriteHalf<TcpStream>, rx: Receiver<Message>) -> Self {
        Self {
            writer,
            rx,
            state: State::new(),
            dht_port: None,
        }
    }

    pub async fn listen(
        mut reader: ReadHalf<TcpStream>,
        tx: Sender<Message>,
        peer: Peer,
    ) -> Result<(), Report> {
        debug!("listening for [{}]", peer.addr);
        loop {
            let mut buf = Vec::new();

            loop {
                match reader.read_to_end(&mut buf).await {
                    Ok(_) => break,
                    Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
                    Err(e) => return Err(e.into()),
                }
            }

            let messages = Message::parse_all(&buf);
            tx.send(messages[0].clone()).await?;
        }
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
                    let n = self.state.remote_bitfield[idx / 8] as usize;
                    let missing = n | (idx % 8);
                    debug!(missing = ?missing);
                    let n = self.state.remote_bitfield.get_mut(idx / 8).unwrap();
                    *n |= (idx % 8) as u8;
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
    fn new() -> Self {
        Self {
            choked: true,
            interested: false,
            peer_choked: true,
            peer_interested: false,
            remote_bitfield: Vec::new(),
        }
    }
}

// the peer id will presumably be sent after the recipient sends its own handshake
#[derive(Clone, Debug)]
pub struct Handshake<'p> {
    pstr: Option<String>,
    reserved: [u8; 8],
    hash: [u8; 20],
    peer_id: [u8; 20],
    payload: Option<&'p [u8]>,
}

impl<'p> Handshake<'p> {
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
    fn from_response(v: &'p [u8]) -> Self {
        let reserved = range_to_array(&v[20..28]);
        let hash = range_to_array(&v[28..48]);
        let peer_id = range_to_array(&v[48..68]);
        Self {
            pstr: None,
            reserved,
            hash,
            peer_id,
            payload: Some(&v[68..]),
        }
    }

    // fn validate_response(&self, v: &[u8]) -> bool {
    //     let (hash, peer_id) = v[1 + 19 + 8..].split_at(20);
    //     hash == self.hash && peer_id == self.peer_id
    // }
}

#[repr(u8)]
#[derive(Debug, Clone)]
pub enum Message {
    Choke = 0,
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

impl Message {
    pub fn to_request<V: AsRef<[u8]>>(&self, payload: Option<V>) -> Vec<u8> {
        let message = if let Some(payload) = payload {
            [&[self.to_discriminant()], payload.as_ref()].concat()
        } else {
            [self.to_discriminant()].to_vec()
        };
        [[message.len() as u8].to_vec(), message].concat()
    }

    pub fn to_discriminant(&self) -> u8 {
        use Message::*;

        match self {
            Choke => 0,
            Unchoke => 1,
            Interested => 2,
            Uninterested => 3,
            Have(_) => 4,
            Bitfield(_) => 5,
            Request { .. } => 6,
            Piece { .. } => 7,
            Cancel { .. } => 8,
            Port(_) => 9,
            Extended => 20,
        }
    }

    fn parse_all(mut v: &[u8]) -> Vec<Message> {
        let mut res = Vec::new();

        while !v.is_empty() {
            let len = u32::from_be_bytes(range_to_array(&v[..4])) as usize;
            let msg = Message::parse(&v[4..4 + len], len);
            v = &v[4 + len..];

            res.push(msg);
        }

        res
    }

    fn parse(v: &[u8], len: usize) -> Message {
        use Message::*;
        debug!(message_type = v[0]);

        match v[0] {
            0 => Choke,
            1 => Unchoke,
            2 => Interested,
            3 => Uninterested,
            4 => Have(usize::from_be_bytes(range_to_array(&v[1..]))),
            5 => {
                let payload = v[..len]
                    .iter()
                    .flat_map(|n| (0..8).map(|i| (n >> i) & 1).collect::<Vec<_>>())
                    .collect();

                Bitfield(payload)
            }
            6 => {
                let index = u32::from_be_bytes(range_to_array(&v[1..5])) as usize;
                let begin = u32::from_be_bytes(range_to_array(&v[5..9])) as usize;
                let length = u32::from_be_bytes(range_to_array(&v[9..13])) as usize;

                Request {
                    index,
                    begin,
                    length,
                }
            }
            7 => {
                let index = u32::from_be_bytes(range_to_array(&v[1..5])) as usize;
                let begin = u32::from_be_bytes(range_to_array(&v[5..9])) as usize;

                let block = v[..len].iter().map(|&i| i as usize).collect();

                Piece {
                    index,
                    begin,
                    block,
                }
            }
            8 => {
                let index = u32::from_be_bytes(range_to_array(&v[1..5])) as usize;
                let begin = u32::from_be_bytes(range_to_array(&v[5..9])) as usize;
                let length = u32::from_be_bytes(range_to_array(&v[9..13])) as usize;

                Cancel {
                    index,
                    begin,
                    length,
                }
            }
            9 => {
                let port = u16::from_be_bytes(range_to_array(&v[1..3]));

                Port(port)
            }
            20 => Extended,
            _ => panic!(),
        }
    }
}
