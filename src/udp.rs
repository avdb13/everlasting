use std::{
    fmt::Display,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

use color_eyre::Report;
use rand::Rng;
use tracing::debug;

use crate::helpers::{decode, MagnetInfo};

#[derive(Clone, Debug)]
pub enum Request {
    Connect {
        cid: i64,
        action: i32,
        tid: i32,
    },
    Announce {
        cid: i64,
        action: i32,
        tid: i32,
        hash: [u8; 20],
        peer_id: [u8; 20],
        downloaded: i64,
        left: i64,
        uploaded: i64,
        event: TransferEvent,
        socket: SocketAddr,
        key: u32,
        num_want: i32,
        extensions: u16,
    },
    Scrape {
        cid: i64,
        action: i32,
        tid: i32,
        hashes: Vec<[u8; 20]>,
    },
}

#[derive(Debug, Clone)]
pub enum Response {
    Connect {
        action: i32,
        tid: i32,
        cid: i64,
    },
    Announce {
        action: i32,
        tid: i32,
        interval: i32,
        leechers: i32,
        seeders: i32,
        peers: Vec<SocketAddr>,
    },
    Scrape {
        action: i32,
        tid: i32,
        hashes: Vec<Status>,
    },
    Error {
        action: i32,
        tid: i32,
        error: String,
    },
}

impl Request {
    // pub fn build(old: Option<(Request, Router)>) -> Request {
    // match old {
    //     _ => Request::Connect {
    //         cid: PROTOCOL_ID,
    //         action: 0_i32,
    //         tid: rand::thread_rng().gen::<i32>(),
    //     },
    //     Some((Request::Connect { cid, tid, .. }, router)) => Request::Announce {
    //         cid,
    //         action: 1i32,
    //         tid: rand::thread_rng().gen::<i32>(),
    //         hash: decode(router.magnet.hash),
    //         peer_id: router.peer_id,
    //         downloaded: 0,
    //         left: 0,
    //         uploaded: 0,
    //         event: Event::Inactive,
    //         socket: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
    //         key: router.,
    //         num_want: -1i32,
    //         extensions: 0u16,
    //     },
    // }
    // }
    //         1 => Request::Connect {
    //     pub fn build(magnet: MagnetInfo, cid: i64, peer_id: [u8; 20], key: u32) -> Self {
    //         Self {
    //             cid,
    //             action: 1i32,
    //             tid: rand::thread_rng().gen::<i32>(),
    //             hash: decode(magnet.hash),
    //             peer_id,
    //             downloaded: 0,
    //             left: 0,
    //             uploaded: 0,
    //             event: Event::Inactive,
    //             socket: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
    //             key,
    //             num_want: -1i32,
    //             extensions: 0u16,
    //         }
    //     }
    //     pub fn build(cid: i64, magnet: MagnetInfo) -> Self {
    //         Self {
    //             cid,
    //             action: 2i32,
    //             tid: rand::thread_rng().gen::<i32>(),
    //             hashes: vec![decode(magnet.hash)],
    //         }
    //     }
    // }
    // cid: PROTOCOL_ID,
    // action: 0_i32,
    // tid: rand::thread_rng().gen::<i32>(),
    // },
    // 2 => Request::Connect {
    // cid: PROTOCOL_ID,
    // action: 0_i32,
    // tid: rand::thread_rng().gen::<i32>(),
    // },
    // }
    // }
}

impl Response {
    pub fn to_response(v: &[u8]) -> Result<Self, Report> {
        match v[3] {
            0 => Ok(Response::Connect {
                action: i32::from_be_bytes(v[0..4].try_into()?),
                tid: i32::from_be_bytes(v[4..8].try_into()?),
                cid: i64::from_be_bytes(v[8..16].try_into()?),
            }),
            1 => {
                let peers = match v.len() {
                    n if n < 20 => Vec::new(),
                    _ => v[20..]
                        .chunks_exact(6)
                        .map(|x| {
                            debug!(?x);
                            SocketAddr::new(
                                IpAddr::V4(Ipv4Addr::new(x[0], x[1], x[2], x[3])),
                                u16::from_be_bytes(x[4..6].try_into().unwrap()),
                            )
                        })
                        .collect(),
                };

                Ok(Response::Announce {
                    action: i32::from_be_bytes(v[0..4].try_into()?),
                    tid: i32::from_be_bytes(v[4..8].try_into()?),
                    interval: i32::from_be_bytes(v[8..12].try_into()?),
                    leechers: i32::from_be_bytes(v[12..16].try_into()?),
                    seeders: i32::from_be_bytes(v[16..20].try_into()?),
                    peers,
                })
            }
            2 => {
                let n = v[8..].len() / (std::mem::size_of::<i32>() * 3);

                Ok(Response::Scrape {
                    action: i32::from_be_bytes(v[0..4].try_into()?),
                    tid: i32::from_be_bytes(v[4..8].try_into()?),
                    hashes: v[8..]
                        .chunks(n)
                        .map(|x| Status {
                            complete: i32::from_be_bytes(x[0..4].try_into().unwrap()),
                            downloaded: i32::from_be_bytes(x[4..8].try_into().unwrap()),
                            incomplete: i32::from_be_bytes(x[8..12].try_into().unwrap()),
                        })
                        .collect(),
                })
            }
            _ => Ok(Response::Error {
                action: i32::from_be_bytes(v[0..4].try_into()?),
                tid: i32::from_be_bytes(v[4..8].try_into()?),
                error: std::str::from_utf8(&v[8..])?.to_owned(),
            }),
        }
    }
}

impl Request {
    pub fn to_request(&self) -> Vec<u8> {
        match self {
            Request::Connect { cid, action, tid } => [
                cid.to_be_bytes().to_vec(),
                [action.to_be_bytes(), tid.to_be_bytes()].concat(),
            ]
            .concat(),
            Request::Announce {
                cid,
                action,
                tid,
                hash,
                peer_id,
                downloaded,
                left,
                uploaded,
                event,
                socket,
                key,
                num_want,
                extensions,
            } => {
                [
                    cid.to_be_bytes().to_vec(),
                    action.to_be_bytes().to_vec(),
                    tid.to_be_bytes().to_vec(),
                    hash.to_vec(),
                    peer_id.to_vec(),
                    downloaded.to_be_bytes().to_vec(),
                    left.to_be_bytes().to_vec(),
                    uploaded.to_be_bytes().to_vec(),
                    [0, 0, 0, 0].to_vec(),
                    [0, 0, 0, 0].to_vec(),
                    key.to_be_bytes().to_vec(),
                    num_want.to_be_bytes().to_vec(),
                    1317u16.to_be_bytes().to_vec(),
                    // self.extensions.to_be_bytes().to_vec(),
                ]
                .concat()
            }
            Request::Scrape {
                cid,
                action,
                tid,
                hashes,
            } => [
                cid.to_be_bytes().as_slice(),
                action.to_be_bytes().as_slice(),
                tid.to_be_bytes().as_slice(),
                &hashes.iter().flat_map(|v| v.to_vec()).collect::<Vec<_>>(),
            ]
            .concat(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Status {
    complete: i32,
    downloaded: i32,
    incomplete: i32,
}

#[derive(Debug, Clone)]
pub enum TransferEvent {
    Inactive = 0,
    Completed,
    Started,
    Stopped,
}
