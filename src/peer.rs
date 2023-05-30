use bytes::BytesMut;
use dashmap::DashMap;
use std::collections::VecDeque;
use std::io::Write;
use std::{
    collections::HashMap, io::ErrorKind, mem::discriminant, net::SocketAddr, sync::Arc,
    time::Duration,
};
use tokio::sync::mpsc::{channel, Receiver, Sender};
use tokio::task::JoinHandle;

use color_eyre::Report;
use rand::Rng;
use std::fs::File;
use tokio::{
    io::{self, split, AsyncReadExt, AsyncWriteExt, ReadHalf, WriteHalf},
    net::{TcpListener, TcpSocket, TcpStream, UdpSocket},
    sync::{mpsc, RwLock},
    task::JoinSet,
    time::{sleep, timeout},
};
use tracing::{debug, field::debug};

use crate::{
    data::GeneralError,
    helpers::{self, decode, range_to_array, MagnetInfo},
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

        for &s in peers.iter() {
            set.spawn(Router::handshake(s, handshake.clone()));
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

    pub async fn handshake(peer: SocketAddr, handshake: Handshake<'_>) -> Result<(), Report> {
        let mut stream = timeout(Duration::from_secs(3), TcpStream::connect(peer)).await??;

        stream.write_all(&handshake.to_request()).await?;
        stream.flush().await?;
        debug!("handshake was sent to {peer:?} ...");

        let mut buf = [0u8; 68];

        match timeout(Duration::from_secs(3), stream.read_exact(&mut buf)).await? {
            Ok(n) if n != 0 => {
                let handshake = Handshake::from_response(&buf[..]);
            }
            _ => return Err(GeneralError::DeadTrackers.into()),
        };

        let s = stream.peer_addr()?;
        let (reader, writer) = split(stream);
        let (tx, mut rx) = channel(100);

        debug!("spawned tasks for [{:?}]", s);

        tokio::spawn(async move {
            Connection::listen(reader, tx);
        });
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                debug!(?msg);
            }
        });

        loop {}

        Ok(())
    }
}

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
            let mut buf = Vec::new();
            match reader.read_to_end(&mut buf).await {
                Ok(n) => {
                    debug!("received something");
                    for message in Message::parse_all(&buf) {
                        tx.send(message).await?;
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    debug!("blocked!");
                    continue;
                }
                Err(e) => {
                    return Err(e.into());
                }
            }
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

impl<'p> Handshake<'p> {
    fn new(hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        Self {
            pstr: Some("BitTorrent protocol".to_owned()),
            reserved: [0u8; 8],
            hash,
            peer_id,
            payload: None,
        }
    }
    fn to_request(&self) -> Vec<u8> {
        let pstr = self.pstr.as_ref().unwrap();
        [
            &[pstr.len() as u8],
            pstr.as_bytes(),
            &[0u8; 8],
            &self.hash,
            &self.peer_id,
        ]
        .concat()
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
        }
    }

    fn parse_all(v: &[u8]) -> Vec<Message> {
        use Message::*;
        let u32_len = std::mem::size_of::<u32>() as u32;

        let mut offset = 0u32;

        let mut result = Vec::new();
        while offset < (v.len() - 1) as u32 {
            let message = match v[4] {
                0 => {
                    offset += u32_len + 1;
                    Choke
                }
                1 => {
                    offset += u32_len + 1;
                    Unchoke
                }
                2 => {
                    offset += u32_len + 1;
                    Interested
                }
                3 => {
                    offset += u32_len + 1;
                    Uninterested
                }
                4 => {
                    offset += u32_len + 5;
                    Have(usize::from_be_bytes(range_to_array(&v[1..])))
                }
                5 => {
                    let n = u32::from_be_bytes(range_to_array(&v[0..4]));
                    debug!(?n);
                    offset += u32_len + 1 + n;
                    let v = v[(u32_len + 1) as usize..]
                        .iter()
                        .flat_map(|n| (0..8).map(|i| (n >> i) & 1).collect::<Vec<_>>())
                        .collect();
                    Bitfield(v)
                }
                6 => {
                    offset += u32_len + 13;
                    Request {
                        index: 0,
                        begin: 0,
                        length: 0,
                    }
                }
                7 => {
                    offset += u32_len + 9;
                    Piece {
                        index: 0,
                        begin: 0,
                        block: Vec::new(),
                    }
                }
                8 => {
                    offset += u32_len + 13;
                    Cancel {
                        index: 0,
                        begin: 0,
                        length: 0,
                    }
                }
                9 => {
                    offset += u32_len + 3;
                    Port(0)
                }
                _ => panic!(),
            };

            debug!(message = ?message);

            result.push(message);
        }

        result
    }
}
