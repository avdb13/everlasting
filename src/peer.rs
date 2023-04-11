use bytes::BytesMut;
use std::{net::SocketAddr, sync::Arc};

use color_eyre::Report;
use rand::Rng;
use tokio::{
    io::{split, AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
};
use tracing::{debug, field::debug};

use crate::{
    helpers::{decode, MagnetInfo},
    udp::{Request, Response},
};

#[derive(Default)]
pub struct Torrent;

impl Torrent {
    pub fn finished(&self) -> bool {
        todo!()
    }
}

pub struct PeerRouter {
    magnet: MagnetInfo,
    peers: Vec<SocketAddr>,
}

impl PeerRouter {
    pub async fn next(&self) -> Result<(), Report> {
        let pid = rand::thread_rng().gen::<[u8; 20]>();
        let stream = TcpStream::connect(self.peers[0]).await?;
        let (mut rx, mut tx) = split(stream);

        let handshake = Handshake::new(decode(self.magnet.hash.clone()), pid);
        debug!(?handshake);

        tokio::spawn(async move {
            let mut buf = BytesMut::with_capacity(1024);
            while let Ok(n) = rx.read_buf(&mut buf).await {
                debug!("{:?}", &buf[..n]);
            }
        });
        tx.write_all(&handshake.to_request()).await?;

        Ok(())
    }
    pub fn new(magnet: MagnetInfo, peers: Vec<SocketAddr>) -> Self {
        PeerRouter { magnet, peers }
    }

    pub async fn broadcast(&self, peer_id: [u8; 20]) -> Result<(), Report> {
        let socket = Arc::new(UdpSocket::bind("0.0.0.0:6969").await?);

        let handshake = Handshake::new(decode(self.magnet.hash.clone()), peer_id);

        let iter = self.peers.iter().map(|p| {
            Box::pin({
                let handshake = handshake.clone();
                let socket = socket.clone();
                async move { socket.send(&handshake.to_request()).await }
            })
        });

        let ok = futures::future::join_all(iter).await;

        let iter = ok.iter().map(|res| {
            let socket = socket.clone();
            async move {
                match res {
                    Ok(_) => {
                        let mut buf = [0u8; 1024];
                        if let Ok(n) = socket.recv(&mut buf[..]).await {
                            debug!("[PEER] received {n} bytes: {:?}", &buf[..n])
                        }
                    }
                    Err(_) => {}
                };
            }
        });

        let ok = futures::future::join_all(iter).await;

        // loop {
        //     while !magnet.finished() {}
        // }
        Ok(())
    }
}

pub struct Client {
    choked: bool,
    interested: bool,
    peer_choked: bool,
    peer_interested: bool,
}

// the peer id will presumably be sent after the recipient sends its own handshake
#[derive(Clone, Debug)]
pub struct Handshake {
    pstr: String,
    hash: [u8; 20],
    peer_id: [u8; 20],
}

pub enum Message {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    Uninterested,
    Have(usize),
    Bitfield(Vec<usize>),
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

impl Handshake {
    fn new(hash: [u8; 20], peer_id: [u8; 20]) -> Self {
        Self {
            pstr: "Bitmagnet protocol".to_owned(),
            hash,
            peer_id,
        }
    }
}

impl Handshake {
    fn to_request(&self) -> Vec<u8> {
        [
            &[self.pstr.len() as u8],
            self.pstr.as_bytes(),
            &[0u8; 8],
            &self.hash,
            &self.peer_id,
        ]
        .concat()
    }
}

// A block is downloaded by the client when the client is interested in a peer,
// and that peer is not choking the client.
// A block is uploaded by a client when the client is not choking a peer,
// and that peer is interested in the client.
impl Default for Client {
    fn default() -> Self {
        Self {
            choked: true,
            interested: false,
            peer_choked: true,
            peer_interested: false,
        }
    }
}
