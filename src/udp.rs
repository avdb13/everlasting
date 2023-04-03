use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use color_eyre::Report;
use rand::Rng;
use tracing::debug;

use crate::{
    helpers::{decode, MagnetInfo},
    PROTOCOL_ID,
};

pub trait Response {
    fn to_response(v: &[u8]) -> Result<Self, Report>
    where
        Self: Sized + Response;
}

pub enum RespType {
    Connect = 0,
    Announce,
    Scrape,
    Error,
}

pub trait Request {
    fn to_request(&self) -> Vec<u8>;
}

#[derive(Debug, Clone)]
pub struct ConnectReq {
    pub cid: i64,
    pub action: i32,
    pub tid: i32,
}

#[derive(Debug, Clone)]
pub struct ConnectResp {
    pub action: i32,
    pub tid: i32,
    pub cid: i64,
}

#[derive(Debug, Clone)]
pub struct AnnounceReq {
    pub cid: i64,
    pub action: i32,
    pub tid: i32,
    pub hash: [u8; 20],
    pub peer_id: [u8; 20],
    pub downloaded: i64,
    pub left: i64,
    pub uploaded: i64,
    pub event: Event,
    pub socket: SocketAddr,
    pub key: u32,
    pub num_want: i32,
    pub extensions: u16,
}

#[derive(Debug, Clone)]
pub struct AnnounceResp {
    pub action: i32,
    pub tid: i32,
    pub interval: i32,
    pub leechers: i32,
    pub seeders: i32,
    pub peers: Vec<SocketAddr>,
}

#[derive(Debug, Clone)]
pub struct ScrapeReq<'a> {
    pub cid: i64,
    pub action: i32,
    pub tid: i32,
    pub hashes: Vec<&'a [u8; 20]>,
}

#[derive(Debug, Clone)]
pub struct ScrapeResp {
    pub action: i32,
    pub tid: i32,
    pub hashes: Vec<Status>,
}

#[derive(Debug, Clone)]
pub struct Status {
    complete: i32,
    downloaded: i32,
    incomplete: i32,
}

#[derive(Debug, Clone)]
pub struct TrackerError {
    action: i32,
    tid: i32,
    error: String,
}

#[derive(Debug, Clone)]
pub enum Event {
    Inactive = 0,
    Completed,
    Started,
    Stopped,
}

impl ConnectReq {
    pub fn build() -> Self {
        Self {
            cid: 0x41727101980_i64,
            action: 0_i32,
            tid: rand::thread_rng().gen::<i32>(),
        }
    }
}

impl Request for ConnectReq {
    fn to_request(&self) -> Vec<u8> {
        [
            self.cid.to_be_bytes().to_vec(),
            [self.action.to_be_bytes(), self.tid.to_be_bytes()].concat(),
        ]
        .concat()
    }
}

impl Response for ConnectResp {
    fn to_response(v: &[u8]) -> Result<Self, Report> {
        Ok(Self {
            action: i32::from_be_bytes(v[0..4].try_into()?),
            tid: i32::from_be_bytes(v[4..8].try_into()?),
            cid: i64::from_be_bytes(v[8..16].try_into()?),
        })
    }
}

impl AnnounceReq {
    pub fn build(magnet: MagnetInfo, cid: i64, peer_id: [u8; 20], key: u32) -> Self {
        debug!("magnet is {:?}", decode(magnet.hash.clone()));
        Self {
            cid,
            action: 1i32,
            tid: rand::thread_rng().gen::<i32>(),
            hash: decode(magnet.hash),
            peer_id,
            downloaded: 0,
            left: 0,
            uploaded: 0,
            event: Event::Inactive,
            socket: SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0),
            key,
            num_want: -1i32,
            extensions: 0u16,
        }
    }
}

impl Request for AnnounceReq {
    fn to_request(&self) -> Vec<u8> {
        let ok = [
            self.cid.to_be_bytes().to_vec(),
            self.action.to_be_bytes().to_vec(),
            self.tid.to_be_bytes().to_vec(),
            self.hash.to_vec(),
            self.peer_id.to_vec(),
            self.downloaded.to_be_bytes().to_vec(),
            self.left.to_be_bytes().to_vec(),
            self.uploaded.to_be_bytes().to_vec(),
            [0, 0, 0, 0].to_vec(),
            [0, 0, 0, 0].to_vec(),
            self.key.to_be_bytes().to_vec(),
            self.num_want.to_be_bytes().to_vec(),
            1317u16.to_be_bytes().to_vec(),
            // self.extensions.to_be_bytes().to_vec(),
        ];
        dbg!(&ok);

        ok.concat()
    }
}

impl Response for AnnounceResp {
    fn to_response(v: &[u8]) -> Result<Self, Report>
    where
        Self: Sized,
    {
        let peers = match v.len() {
            n if n < 20 => Vec::new(),
            _ => v[20..]
                .chunks(6)
                .map(|x| {
                    SocketAddr::new(
                        IpAddr::V4(Ipv4Addr::new(x[0], x[1], x[2], x[3])),
                        u16::from_be_bytes(x[4..6].try_into().unwrap()),
                    )
                })
                .collect(),
        };

        Ok(Self {
            // 0, 0, 0, 0
            action: i32::from_be_bytes(v[0..4].try_into()?),
            // 240, 23, 226, 211
            tid: i32::from_be_bytes(v[4..8].try_into()?),
            // 117, 26, 194, 111
            interval: i32::from_be_bytes(v[8..12].try_into()?),
            // 6, 180, 52, 126
            leechers: i32::from_be_bytes(v[12..16].try_into()?),
            seeders: i32::from_be_bytes(v[16..20].try_into()?),
            peers,
        })
    }
}

impl Request for ScrapeReq<'_> {
    fn to_request(&self) -> Vec<u8> {
        [
            self.cid.to_be_bytes().as_slice(),
            self.action.to_be_bytes().as_slice(),
            self.tid.to_be_bytes().as_slice(),
            &self
                .hashes
                .iter()
                .flat_map(|v| v.to_vec())
                .collect::<Vec<_>>(),
        ]
        .concat()
    }
}

impl Response for ScrapeResp {
    fn to_response(v: &[u8]) -> Result<Self, Report>
    where
        Self: Sized,
    {
        let n = v[8..].len() / (std::mem::size_of::<i32>() * 3);

        Ok(Self {
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
}

impl Response for TrackerError {
    fn to_response(v: &[u8]) -> Result<Self, Report>
    where
        Self: Sized,
    {
        Ok(Self {
            action: i32::from_be_bytes(v[0..4].try_into()?),
            tid: i32::from_be_bytes(v[4..8].try_into()?),
            error: std::str::from_utf8(&v[8..])?.to_owned(),
        })
    }
}
