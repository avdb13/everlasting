use bendy::encoding::ToBencode;
use std::{collections::HashMap, io::ErrorKind, net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::{TcpStream, UdpSocket},
    sync::mpsc::{channel, Receiver, Sender},
    task::JoinSet,
};

use color_eyre::Report;
use rand::Rng;

use tokio::{
    io::{self, split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    time::{sleep, timeout},
};
use tracing::{debug, info};

use crate::{
    data::{GeneralError, MagnetInfo},
    dht::Node,
    helpers::{self, decode, range_to_array},
    krpc::{self, Arguments, Method},
    udp::{Request, Response},
};

pub struct Router {
    pub magnet: MagnetInfo,
    pub bitfield: Vec<u8>,
    pub peers: HashMap<SocketAddr, Connection>,
}

impl Router {
    pub fn new(magnet: MagnetInfo) -> Self {
        Router {
            magnet,
            peers: HashMap::new(),
            bitfield: Vec::new(),
        }
    }

    pub async fn connect(&mut self, peers: Vec<SocketAddr>) -> Result<(), Report> {
        let peer_id = rand::thread_rng().gen::<[u8; 20]>();
        let handshake = Handshake::new(self.magnet.hash, peer_id);

        let mut set = JoinSet::new();

        let socket = Arc::new(UdpSocket::bind("0.0.0.0:6969").await?);
        for &s in peers.iter() {
            set.spawn(Router::handshake(socket.clone(), s, handshake.clone()));
        }

        while let Some(result) = set.join_next().await {
            match result? {
                Ok(()) => {
                    // self.peers.insert(s, Connection::new(writer, rx));
                }
                Err(_) => {
                    continue;
                }
            }
        }

        // for conn in self.peers.values_mut() {
        //     set.spawn(conn.resolve());
        // }
        // while let Some(result) = set.join_next().await {
        //     match result {
        //         Ok(_) => todo!(),
        //         Err(_) => todo!(),
        //     }
        // }

        Ok(())
    }

    pub async fn keep_alive() {
        loop {
            sleep(Duration::from_secs(120)).await;
        }
    }

    pub async fn handshake(
        dht_socket: Arc<UdpSocket>,
        peer: SocketAddr,
        handshake: Handshake<'_>,
    ) -> Result<(), Report> {
        let mut stream = timeout(Duration::from_secs(3), TcpStream::connect(peer)).await??;

        stream.write_all(&handshake.to_request()).await?;
        stream.flush().await?;
        debug!("handshake was sent to [{peer:?}] ...");

        let mut buf = [0u8; 68];

        match timeout(Duration::from_secs(3), stream.read_exact(&mut buf)).await? {
            Ok(n) if n != 0 => {
                debug!("[{peer:?}] received the handshake ...");
                let handshake = Handshake::from_response(&buf[..]);
            }
            _ => return Err(GeneralError::DeadTrackers.into()),
        };

        let remote_peer = stream.peer_addr()?;
        let (reader, writer) = split(stream);
        let (tx, mut rx) = channel(100);

        let handle = tokio::spawn(Connection::listen(reader, tx));
        debug!("[{peer:?}] is listening for messages ...");

        while let Some(Message::Port(n)) = rx.recv().await {
            let s = SocketAddr::new(remote_peer.ip(), n);
            dht_socket.connect(s).await?;

            let args = Arguments {
                method: Method::Ping,
                id: rand::thread_rng().gen(),
                ..Default::default()
            };

            // query: Box::new(krpc::DHTMessage::Ping),
            // node_id: Node(rand::thread_rng().gen()),
            let msg = krpc::Message::Query(args);
            // let buf = msg.to_bencode().unwrap();
            loop {}
            dht_socket.send(&buf).await?;

            let mut buf = Vec::new();
            while let Ok(n) = dht_socket.recv(&mut buf).await {
                if n != 0 {
                    debug!("received: {:?}", &buf[..n]);
                }
            }
        }

        Ok(())
    }
}

pub struct DhtConnection;

pub struct Connection {
    pub writer: WriteHalf<TcpStream>,
    pub rx: Receiver<Message>,
    pub state: State,
}

impl Connection {
    pub fn new(writer: WriteHalf<TcpStream>, rx: Receiver<Message>) -> Self {
        Self {
            writer,
            rx,
            state: State::new(),
        }
    }

    pub async fn listen(
        mut reader: ReadHalf<TcpStream>,
        tx: Sender<Message>,
    ) -> Result<(), Report> {
        loop {
            let mut buf = [0u8; 4];
            reader.read_exact(&mut buf).await?;

            let n = u32::from_be_bytes(range_to_array(&buf));

            let mut buf = vec![0; (n + 1) as usize];
            loop {
                match reader.read_exact(&mut buf).await {
                    // Ok(n) if n != 0 => break,
                    Ok(_) => break,
                    Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
                    Err(e) => return Err(e.into()),
                }
            }
            dbg!(&buf[0]);

            let message = Message::parse(&buf, n as usize);
            match &message {
                Message::Bitfield(_) => {}
                message => {
                    debug!(?message);
                }
            }
            tx.send(message).await?;
        }
    }

    pub async fn resolve(mut self) -> Result<(), Report> {
        // while let Some(message) = self.rx.recv().ok() {
        //     match message {
        //         Message::Choke => {
        //             self.state.choked = true;
        //         }
        //         Message::Unchoke => {
        //             self.state.choked = false;
        //         }
        //         Message::Interested => {
        //             self.state.interested = true;
        //         }
        //         Message::Uninterested => {
        //             self.state.interested = false;
        //         }
        //         Message::Have(idx) => {
        //             let n = self.state.remote_bitfield[idx / 8] as usize;
        //             let missing = n | (idx % 8);
        //             debug!(missing = ?missing);
        //             let n = self.state.remote_bitfield.get_mut(idx / 8).unwrap();
        //             *n |= (idx % 8) as u8;
        //         }
        //         Message::Bitfield(v) => {
        //             self.state.remote_bitfield = v;
        //         }
        //         Message::Request {
        //             index,
        //             begin,
        //             length,
        //         } => todo!(),
        //         Message::Piece {
        //             index,
        //             begin,
        //             block,
        //         } => todo!(),
        //         Message::Cancel {
        //             index,
        //             begin,
        //             length,
        //         } => todo!(),
        //         Message::Port(_) => todo!(),
        //     }
        // }
        Ok(())
    }
}

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

pub enum Extension {
    DHT = 64,
}

impl<'p> Handshake<'p> {
    fn new(hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        let mut reserved = [0u8; 8];
        // DHT protocol
        reserved[7] |= 0x01;
        // extension protocol
        reserved[5] &= 0x20;

        Self {
            pstr: Some("BitTorrent protocol".to_owned()),
            reserved,
            hash,
            peer_id,
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
#[derive(Debug)]
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
    pub fn to_request(&self, payload: Option<&[u8]>) -> Vec<u8> {
        let message = if let Some(payload) = payload {
            [&[self.to_discriminant()], payload].concat()
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

    fn parse(v: &[u8], len: usize) -> Message {
        use Message::*;

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
