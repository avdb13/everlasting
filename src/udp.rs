use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use color_eyre::Report;
use tracing::debug;

use crate::data::Event;

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
        info_hash: [u8; 20],
        peer_id: [u8; 20],
        up_down_left: (usize, usize, usize),
        event: Event,
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
                info_hash,
                peer_id,
                up_down_left,
                event: _,
                socket: _,
                key,
                num_want,
                extensions: _,
            } => {
                [
                    cid.to_be_bytes().to_vec(),
                    action.to_be_bytes().to_vec(),
                    tid.to_be_bytes().to_vec(),
                    info_hash.to_vec(),
                    peer_id.to_vec(),
                    up_down_left.0.to_be_bytes().to_vec(),
                    up_down_left.2.to_be_bytes().to_vec(),
                    up_down_left.1.to_be_bytes().to_vec(),
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
